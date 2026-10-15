//! Issue #904：正文吞吐动画的唯一本地计划。
//!
//! 本模块取代 #901 的整章 `visual_canvas` 视觉状态机。它只回答一个问题：
//! **这一次编辑，哪些局部字形片段需要在若干毫秒内从中途显示出来**
//! （吐字 / 吞字 / 位移），并按同一个 `progress` 采样出本帧要画的局部覆盖层
//! 和静态层要挖掉的孔洞。
//!
//! 边界（见议题第 6 节）：
//! - 只持有局部片段的几何与源纹理引用，不创建/删除任何 Qt `QSGNode`。
//! - 不重建整屏文字；`old_layout` / `new_layout` 分别给出编辑前后的局部
//!   cluster 几何（文档坐标）与源 UV，未变化部分不产生任何命令。
//! - 只有一个动画进度：`sample()` 里 `p = clamp((now-started)/duration, 0, 1)`
//!   计算一次，吐字/吞字/位移共用。
//! - 新旧字形按 `QTextLayout` 的 cluster 边界（Core 的 UTF-8 byte range）
//!   整体区分，不猜半个字节。

use super::layout_snapshot::{EditorLayoutSnapshot, SourceRect};
use super::qt_text_node::AnimationClipRect;
use super::render_plan::TextAnimationGlyphInfo;
use super::snapshot_id::LineSnapshotId;
use writer_core::editor::{EditorEditResult, EditorRevision};

/// Issue #904：字形片段使用的本地矩形（文档坐标），与静态层同坐标系。
pub(crate) type LocalRect = SourceRect;

/// Issue #904：裁剪推进方向。
///
/// - `LeftToRight`：从左侧边界向右露出（普通输入）。
/// - `RightToLeft`：从右侧边界向左露出（Backspace 从右往左吞）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipDirection {
    LeftToRight,
    RightToLeft,
}

/// Issue #904：一个局部字形片段的源纹理引用（快照 id + 纹理内 UV）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SnapshotRef {
    pub snapshot_id: LineSnapshotId,
    /// 源行纹理内的 UV（像素，已含 DPR）。
    pub source_uv: LocalRect,
}

/// Issue #904：本帧实际参与动画的局部字形片段。
#[derive(Clone, Debug)]
pub(crate) struct VisibleFragment {
    pub snapshot: SnapshotRef,
    /// Core UTF-8 byte range（半开区间）。
    pub byte_range: (usize, usize),
    /// 文档坐标矩形（不含滚动偏移）。
    pub rect: LocalRect,
    /// 当前可见宽度（= `rect.w` 时完全可见）。
    pub mask_width: f64,
    /// 所属段落 id（用于整体段落位移的去重）。
    pub paragraph_id: u64,
    pub source_revision: EditorRevision,
    pub glyph_id: u64,
}

/// Issue #904：连续编辑重定目标时使用的“上一次真正画出的局部图片”。
///
/// 只记录上一次实际绘制出去的局部片段（当前位置、当前遮罩宽度、源 id），
/// 不含历史帧队列，也不参与静态正文布局。
#[derive(Clone, Debug)]
pub(crate) struct LocalTextFrame {
    pub session: u64,
    pub revision: u64,
    pub fragments: Vec<VisibleFragment>,
}

/// Issue #904 复核 2：遮罩相位。
///
/// 片段上一次真正画出去时遮罩可能还没走完（吐到一半又继续输入）。新计划必须
/// 从「当前可见宽度」继续推到终点，而不是因为目标矩形相等就当静态完整字。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MaskPhase {
    /// 计划开始时该片段已可见的宽度。
    pub start: f64,
    /// 计划结束时应达到的宽度（Reveal/Move = 全宽，Conceal = 0）。
    pub end: f64,
}

/// Issue #904：三类局部动画，共用同一个 progress。
#[derive(Clone, Debug)]
pub(crate) enum TextPiece {
    /// 新字逐渐露出（输入/粘贴）。
    Reveal {
        new_source: SnapshotRef,
        at: LocalRect,
        direction: ClipDirection,
        mask: MaskPhase,
        byte_range: (usize, usize),
        source_revision: EditorRevision,
        glyph_id: u64,
    },
    /// 旧字逐渐被覆盖（删除），仍有旧局部纹理可画。
    Conceal {
        old_source: SnapshotRef,
        at: LocalRect,
        direction: ClipDirection,
        mask: MaskPhase,
        byte_range: (usize, usize),
        source_revision: EditorRevision,
        glyph_id: u64,
    },
    /// 同一未删 cluster 从 `from` 移到 `to`（行内位移 / 换行 / 段落回流）。
    Move {
        source: SnapshotRef,
        from: LocalRect,
        to: LocalRect,
        mask: MaskPhase,
        byte_range: (usize, usize),
        source_revision: EditorRevision,
        glyph_id: u64,
    },
}

/// Issue #904 复核 1：受影响段落以下的可见段落只做整体 y 位移。
///
/// 复用已有 `QSGTransformNode` 从旧 y 插值到新 y，不逐字复制纹理；
/// 动画结束（progress=1）时偏移回到 0，直接留在目标矩阵。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ParagraphMove {
    pub paragraph_id: u64,
    pub from_y: f64,
    pub to_y: f64,
}

impl ParagraphMove {
    /// 当前进度下相对目标位置的额外 y 偏移（结束为 0）。
    fn offset_at(&self, progress: f64) -> f64 {
        (self.from_y - self.to_y) * (1.0 - progress)
    }
}

impl TextPiece {
    fn byte_range(&self) -> (usize, usize) {
        match self {
            TextPiece::Reveal { byte_range, .. }
            | TextPiece::Conceal { byte_range, .. }
            | TextPiece::Move { byte_range, .. } => *byte_range,
        }
    }

    fn glyph_id(&self) -> u64 {
        match self {
            TextPiece::Reveal { glyph_id, .. }
            | TextPiece::Conceal { glyph_id, .. }
            | TextPiece::Move { glyph_id, .. } => *glyph_id,
        }
    }

    fn motion_rect(&self) -> LocalRect {
        match self {
            TextPiece::Reveal { at, .. } | TextPiece::Conceal { at, .. } => *at,
            TextPiece::Move { from, to, .. } => union_rect(from, to),
        }
    }

    fn source_revision(&self) -> EditorRevision {
        match self {
            TextPiece::Reveal {
                source_revision, ..
            }
            | TextPiece::Conceal {
                source_revision, ..
            }
            | TextPiece::Move {
                source_revision, ..
            } => *source_revision,
        }
    }

    fn source(&self) -> SnapshotRef {
        match self {
            TextPiece::Reveal { new_source, .. } => *new_source,
            TextPiece::Conceal { old_source, .. } => *old_source,
            TextPiece::Move { source, .. } => *source,
        }
    }

    fn mask(&self) -> MaskPhase {
        match self {
            TextPiece::Reveal { mask, .. }
            | TextPiece::Conceal { mask, .. }
            | TextPiece::Move { mask, .. } => *mask,
        }
    }
}

/// Issue #904：一次编辑产生的唯一动画对象。
#[derive(Clone, Debug)]
pub(crate) struct TextMotion {
    /// 本次编辑涉及的全部局部片段（旧 + 新），用于源纹理保留。
    pub source: LocalTextFrame,
    pub pieces: Vec<TextPiece>,
    pub started_ms: f64,
    pub duration_ms: u32,
    pub session: u64,
    pub target_revision: u64,
    /// 动画最终裁剪到的视口（文档坐标）。
    pub viewport: LocalRect,
    /// 静态层需排除的目标字形矩形（Reveal 的新位置 / Move 的目标位置）。
    ///
    /// 与 progress 无关：从过渡开始到结束都固定排除新 canonical 的完整 target
    /// 字形，即使 progress=0 尚无可见动画像素。Conceal 是旧字，不在此列。
    pub static_exclusions: Vec<AnimationClipRect>,
    /// 本计划接管所需全部动画源纹理 id；缺任一纹理则不接管（静态层画完整
    /// canonical），两层用同一标志做对称决策。
    pub required_textures: Vec<LineSnapshotId>,
    /// 协同模式下光标在同一 progress 上插值：起点/终点取布局真实 caret 几何。
    pub caret_from: Option<(f64, f64)>,
    pub caret_to: Option<(f64, f64)>,
    /// 受影响段落以下、仅整体 y 变化的可见段落：按同一 progress 平移其
    /// 已有 transform，不生成逐字覆盖层。
    pub paragraph_moves: Vec<ParagraphMove>,
}

/// Issue #904：`TextMotion::sample()` 的一帧结果。
#[derive(Clone, Debug, Default)]
pub(crate) struct TextMotionFrame {
    /// 静态层需要排除的局部矩形（固定 target，与 progress 无关）。
    pub static_holes: Vec<AnimationClipRect>,
    /// 本帧要画的局部文字覆盖层（位置/UV/clip 已算好）。
    pub overlays: Vec<TextAnimationGlyphInfo>,
    /// 本帧接管所需的动画源纹理 id。
    pub required_textures: Vec<LineSnapshotId>,
    /// 本帧仍活动的整体段落 y 偏移（progress 未走完时非空）。
    pub paragraph_y_offsets: Vec<(u64, f64)>,
    pub finished: bool,
    pub needs_next_frame: bool,
    /// 协同模式下正文动画推进到的光标采样点（文档坐标）。
    pub caret_sample: Option<(f64, f64)>,
}

impl TextMotion {
    /// Issue #904：构造或重定向一次局部动画。
    ///
    /// 返回 `None` 表示这次编辑没有可动画的局部片段（纯光标移动等），
    /// 调用方应直接走静态绘制。
    ///
    /// `last_drawn` 是上一次真正绘制出去的局部图片；连续快速输入/删除直接
    /// 从它在途位置重定目标，不叠加历史按键队列。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_or_retarget(
        edit: &EditorEditResult,
        old_layout: Option<&EditorLayoutSnapshot>,
        new_layout: &EditorLayoutSnapshot,
        last_drawn: Option<&LocalTextFrame>,
        now_ms: f64,
        duration_ms: u32,
        session: u64,
        viewport: LocalRect,
    ) -> Option<Self> {
        let (old_start, old_end, new_start, new_end) = edit_ranges(edit)?;
        if old_start == old_end && new_start == new_end {
            return None;
        }

        let new_revision = edit.new_revision;
        let old_revision = edit.base_revision;

        let direction = conceal_direction(edit);
        let delta = (new_end as i64 - new_start as i64) - (old_end as i64 - old_start as i64);

        let drawn_frags: &[VisibleFragment] = last_drawn
            .filter(|frame| frame.session == session)
            .map(|frame| frame.fragments.as_slice())
            .unwrap_or(&[]);

        // 新侧：编辑后目标布局受影响段落的**完整**局部片段（含同段后续折行与
        // 字节范围重新对齐前的后续段落）。这里既不做视口预过滤，也不只取到
        // 改动行末尾——否则换行/回流后本应 Move 的字会被误判成 Reveal/Conceal。
        let new_frags = layout_fragments(new_layout, (new_start, new_end), new_revision);
        let new_span_end = new_frags
            .iter()
            .map(|f| f.byte_range.1)
            .max()
            .unwrap_or(new_end);

        // 旧侧：用对齐到同一字节范围（旧坐标）抓取完整几何，使“只是移动”的字
        // 能按字节 offset 配成 Move。同时把上一次真正画出的遮罩宽度带上，
        // 使吐到一半的字从当前可见宽度继续。
        let old_span_end = ((new_span_end as i64) - delta).max(old_end as i64) as usize;
        let mut old_frags: Vec<VisibleFragment> = match old_layout {
            Some(layout) => layout_fragments(layout, (old_start, old_span_end), old_revision),
            None => Vec::new(),
        };
        for frag in &mut old_frags {
            if let Some(mask) = drawn_mask_for(drawn_frags, frag) {
                frag.mask_width = mask;
            }
        }
        // 上一次还在吞、旧布局里已不存在的旧字：保留旧画面继续吞。
        for frag in drawn_frags {
            if !overlaps(frag.byte_range, (old_start, old_end))
                || old_frags.iter().any(|f| f.byte_range == frag.byte_range)
            {
                continue;
            }
            let mut frag = frag.clone();
            frag.source_revision = old_revision;
            old_frags.push(frag);
        }

        // 受影响段落以下、仅整体 y 变化的段落交给 transform 平移，其文字不再
        // 逐字生成 Move 覆盖层（避免与 transform 叠加两次位移）。
        let paragraph_moves = collect_paragraph_moves(old_layout, new_layout, &viewport);
        let moved_paragraphs: Vec<u64> = paragraph_moves.iter().map(|m| m.paragraph_id).collect();

        let mut pieces: Vec<TextPiece> = Vec::new();
        let mut consumed_old: Vec<bool> = vec![false; old_frags.len()];

        for new_frag in &new_frags {
            // 交给整体段落位移的段落，不生成逐字覆盖层。
            if moved_paragraphs.contains(&new_frag.paragraph_id) {
                continue;
            }
            // 把新坐标映回旧坐标以匹配未删 cluster。
            let mapped = map_new_to_old(new_frag.byte_range, new_start, new_end, delta);
            let matched = old_frags
                .iter()
                .enumerate()
                .find(|(i, f)| {
                    !consumed_old[*i] && f.byte_range == mapped && f.source_revision == old_revision
                })
                .map(|(i, _)| i);

            // 复核 2：同 byte range 上一次真正画出的可见宽度；没有则视为全宽。
            let start_visible = drawn_mask_for(drawn_frags, new_frag).unwrap_or(new_frag.rect.w);

            match matched {
                Some(idx) => {
                    consumed_old[idx] = true;
                    let old_frag = &old_frags[idx];
                    if !rect_approx_eq(&old_frag.rect, &new_frag.rect) {
                        pieces.push(TextPiece::Move {
                            source: new_frag.snapshot,
                            from: old_frag.rect,
                            to: new_frag.rect,
                            mask: MaskPhase {
                                start: start_visible,
                                end: new_frag.rect.w,
                            },
                            byte_range: new_frag.byte_range,
                            source_revision: old_revision,
                            glyph_id: new_frag.glyph_id,
                        });
                    } else if start_visible + MASK_EPS < new_frag.rect.w {
                        // 位置没变但上一次还没吐完：继续从当前宽度露出，
                        // 不再因为 rect 相等就当作静态完整字（避免闪变）。
                        pieces.push(TextPiece::Reveal {
                            new_source: new_frag.snapshot,
                            at: new_frag.rect,
                            direction: ClipDirection::LeftToRight,
                            mask: MaskPhase {
                                start: start_visible,
                                end: new_frag.rect.w,
                            },
                            byte_range: new_frag.byte_range,
                            source_revision: new_revision,
                            glyph_id: new_frag.glyph_id,
                        });
                    }
                }
                None => {
                    let start = drawn_mask_for(drawn_frags, new_frag).unwrap_or(0.0);
                    pieces.push(TextPiece::Reveal {
                        new_source: new_frag.snapshot,
                        at: new_frag.rect,
                        direction: ClipDirection::LeftToRight,
                        mask: MaskPhase {
                            start,
                            end: new_frag.rect.w,
                        },
                        byte_range: new_frag.byte_range,
                        source_revision: new_revision,
                        glyph_id: new_frag.glyph_id,
                    });
                }
            }
        }

        for (idx, old_frag) in old_frags.iter().enumerate() {
            // 只为本次编辑**真正删掉/替换**的旧字产生 Conceal；为配 Move 而多抓
            // 的邻接段落片段不得凭空被吞。
            if consumed_old[idx]
                || moved_paragraphs.contains(&old_frag.paragraph_id)
                || !in_any_old_patch(edit, old_frag.byte_range)
            {
                continue;
            }
            let start_visible = drawn_mask_for(drawn_frags, old_frag).unwrap_or(old_frag.rect.w);
            pieces.push(TextPiece::Conceal {
                old_source: old_frag.snapshot,
                at: old_frag.rect,
                direction,
                mask: MaskPhase {
                    start: start_visible,
                    end: 0.0,
                },
                byte_range: old_frag.byte_range,
                source_revision: old_revision,
                glyph_id: old_frag.glyph_id,
            });
        }

        // Move 用“来源与目标矩形的并集”判断是否与视口相交，屏幕外进入/离开的字
        // 只要路径经过视口就保留；Reveal/Conceal 用自身矩形。
        pieces.retain(|piece| rects_intersect(&piece.motion_rect(), &viewport));
        if pieces.is_empty() && paragraph_moves.is_empty() {
            return None;
        }

        // 静态层排除矩形与 progress 无关：Reveal 固定排除新 canonical 的完整
        // target 字形（即使 progress=0 尚无可见像素），Move 固定排除目标位置。
        // Conceal 是旧字，不按其旧位置挖掉新 canonical。
        let static_exclusions: Vec<AnimationClipRect> = pieces
            .iter()
            .filter_map(|piece| match piece {
                TextPiece::Reveal { new_source, at, .. } => Some(AnimationClipRect {
                    x: at.x,
                    y: at.y,
                    w: at.w,
                    h: at.h,
                    snapshot_id: new_source.snapshot_id,
                }),
                TextPiece::Move { source, to, .. } => Some(AnimationClipRect {
                    x: to.x,
                    y: to.y,
                    w: to.w,
                    h: to.h,
                    snapshot_id: source.snapshot_id,
                }),
                TextPiece::Conceal { .. } => None,
            })
            .collect();

        // 源纹理：本次编辑涉及的全部旧/新片段都要能进 TextureCache，
        // 缺一张就不接管（静态层完整显示 canonical）。
        let mut source_frags = new_frags;
        for frag in &old_frags {
            if !source_frags.iter().any(|f| {
                f.snapshot.snapshot_id == frag.snapshot.snapshot_id
                    && f.byte_range == frag.byte_range
            }) {
                source_frags.push(frag.clone());
            }
        }
        if let Some(drawn) = last_drawn.filter(|frame| frame.session == session) {
            for frag in &drawn.fragments {
                if !source_frags.iter().any(|f| {
                    f.snapshot.snapshot_id == frag.snapshot.snapshot_id
                        && f.byte_range == frag.byte_range
                }) {
                    source_frags.push(frag.clone());
                }
            }
        }

        let mut required_textures: Vec<LineSnapshotId> = Vec::new();
        for piece in &pieces {
            let id = piece.source().snapshot_id;
            if !required_textures.contains(&id) {
                required_textures.push(id);
            }
        }

        // 协同模式下光标共用同一 progress：起点/终点取布局真实 caret 几何，
        // 不拿 overlay 并集的右端冒充 caret 目标。
        let caret_from = old_layout
            .and_then(|layout| layout.caret_rect_doc.as_ref())
            .map(|caret| (caret.x, caret.y));
        let caret_to = new_layout
            .caret_rect_doc
            .as_ref()
            .map(|caret| (caret.x, caret.y));

        Some(TextMotion {
            source: LocalTextFrame {
                session,
                revision: new_revision.value(),
                fragments: source_frags,
            },
            pieces,
            started_ms: now_ms,
            duration_ms,
            session,
            target_revision: new_revision.value(),
            viewport,
            static_exclusions,
            required_textures,
            caret_from,
            caret_to,
            paragraph_moves,
        })
    }

    /// Issue #904：按同一个 `progress` 采样本帧的局部覆盖层与静态孔洞。
    pub(crate) fn sample(&self, now_ms: f64) -> TextMotionFrame {
        let raw = if self.duration_ms == 0 {
            1.0
        } else {
            ((now_ms - self.started_ms) / f64::from(self.duration_ms)).clamp(0.0, 1.0)
        };
        let progress = ease_out_cubic(raw);
        let finished = raw >= 1.0;

        // 过渡结束：不再挖孔、不再画覆盖层、段落位移偏移回到 0（留在目标矩阵）；
        // 同一帧静态层恢复完整 canonical。
        if finished {
            return TextMotionFrame {
                static_holes: Vec::new(),
                overlays: Vec::new(),
                required_textures: self.required_textures.clone(),
                paragraph_y_offsets: Vec::new(),
                finished: true,
                needs_next_frame: false,
                caret_sample: self.caret_to,
            };
        }

        let mut overlays = Vec::with_capacity(self.pieces.len());
        for piece in &self.pieces {
            let mask = piece.mask();
            let visible = lerp(mask.start, mask.end, progress);
            let (target_rect, source_uv, clip_rect) = match piece {
                TextPiece::Reveal {
                    new_source,
                    at,
                    direction,
                    ..
                } => (
                    *at,
                    new_source.source_uv,
                    mask_rect(at, *direction, visible),
                ),
                TextPiece::Conceal {
                    old_source,
                    at,
                    direction,
                    ..
                } => (
                    *at,
                    old_source.source_uv,
                    mask_rect(at, *direction, visible),
                ),
                TextPiece::Move {
                    source, from, to, ..
                } => {
                    let rect = lerp_rect(from, to, progress);
                    (
                        rect,
                        source.source_uv,
                        mask_rect(&rect, ClipDirection::LeftToRight, visible),
                    )
                }
            };

            // 与视口求交；完全离开视口的片段本帧不画（已是 0 面积）。
            let clip = intersect_rect(&clip_rect, &self.viewport);
            overlays.push(glyph_info(piece, target_rect, source_uv, clip));
        }

        TextMotionFrame {
            // 静态孔洞固定取自计划的 target 排除集合，不随 progress 移动。
            static_holes: self.static_exclusions.clone(),
            overlays,
            required_textures: self.required_textures.clone(),
            paragraph_y_offsets: self
                .paragraph_moves
                .iter()
                .map(|m| (m.paragraph_id, m.offset_at(progress)))
                .collect(),
            finished: false,
            needs_next_frame: true,
            caret_sample: self.caret_sample_at(progress),
        }
    }

    /// Issue #904 评论 3: 由本帧**实际画出去**的覆盖层，快照出「上次真正画出的
    /// 局部图片」——当前矩形、当前遮罩宽度、源 id。
    ///
    /// 快速连续输入/删除时下一次直接从这里重定向，而不是回到静态起点；
    /// 不含历史帧队列，也不是第二套引擎。
    pub(crate) fn drawn_local_frame(&self, frame: &TextMotionFrame) -> LocalTextFrame {
        let fragments = frame
            .overlays
            .iter()
            .map(|glyph| VisibleFragment {
                snapshot: SnapshotRef {
                    snapshot_id: glyph.snapshot_id,
                    source_uv: glyph.source_rect,
                },
                byte_range: glyph.logical_range,
                rect: LocalRect {
                    x: glyph.x,
                    y: glyph.y,
                    w: glyph.w,
                    h: glyph.h,
                },
                mask_width: glyph.clip_rect.w,
                paragraph_id: 0,
                source_revision: glyph.source_revision,
                glyph_id: glyph.glyph_id,
            })
            .collect();
        LocalTextFrame {
            session: self.session,
            revision: self.target_revision,
            fragments,
        }
    }

    /// 协同光标：同一 `progress` 上的 caret 位置（真实布局 caret 之间插值）。
    fn caret_sample_at(&self, progress: f64) -> Option<(f64, f64)> {
        match (self.caret_from, self.caret_to) {
            (Some(from), Some(to)) => Some((
                from.0 + (to.0 - from.0) * progress,
                from.1 + (to.1 - from.1) * progress,
            )),
            (None, Some(to)) => Some(to),
            (Some(from), None) => Some(from),
            (None, None) => None,
        }
    }

    /// Issue #904：只更新下一帧采样使用的时长基准，不新建第二条时间线。
    pub(crate) fn set_duration_ms(&mut self, duration_ms: u32) {
        self.duration_ms = duration_ms;
    }

    /// 本动画引用的全部源纹理 id。
    pub(crate) fn source_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = self
            .pieces
            .iter()
            .map(|piece| piece.source().snapshot_id)
            .collect();
        ids.extend(self.source.fragments.iter().map(|f| f.snapshot.snapshot_id));
        ids.sort_by_key(|id| id.to_cache_key());
        ids.dedup();
        ids
    }
}

// ── 辅助函数 ──────────────────────────────────────────────────────────

/// 遮罩是否已走完的容差（像素）。
const MASK_EPS: f64 = 0.05;

/// Issue #904：从 `EditorEditResult.display_patches` 分别取旧/新坐标范围，
/// 不把两侧并到同一个坐标系里。
fn edit_ranges(edit: &EditorEditResult) -> Option<(usize, usize, usize, usize)> {
    if edit.display_patches.is_empty() {
        return None;
    }
    let mut old_start = usize::MAX;
    let mut old_end = 0usize;
    let mut new_start = usize::MAX;
    let mut new_end = 0usize;
    for patch in &edit.display_patches {
        old_start = old_start.min(patch.replace_byte_range.start().value());
        old_end = old_end.max(patch.replace_byte_range.end().value());
        new_start = new_start.min(patch.inserted_byte_range.start().value());
        new_end = new_end.max(patch.inserted_byte_range.end().value());
    }
    if old_start == usize::MAX || new_start == usize::MAX {
        return None;
    }
    Some((old_start, old_end, new_start, new_end))
}

/// 判断删除方向：由本次操作的旧/新光标关系区分 Backspace / Forward Delete。
///
/// - Backspace：光标左移（`new_head < old_head`）→ 遮罩从右向左吞。
/// - Forward Delete / 其它：光标未左移 → 从左向右吞。
///
/// 不再拿“旧删除范围终点 vs 新光标”判断（正常 Backspace 后光标落在删除范围
/// 左侧，旧写法会把方向判反）。
fn conceal_direction(edit: &EditorEditResult) -> ClipDirection {
    let old_head = edit.old_selection.head.index.value();
    let new_head = edit.new_selection.head.index.value();
    if new_head < old_head {
        ClipDirection::RightToLeft
    } else {
        ClipDirection::LeftToRight
    }
}

/// 把新坐标范围映射回旧坐标范围（用于匹配未删 cluster）。
fn map_new_to_old(
    range: (usize, usize),
    new_start: usize,
    new_end: usize,
    delta: i64,
) -> (usize, usize) {
    let map = |offset: usize| -> usize {
        if offset <= new_start {
            offset
        } else if offset >= new_end {
            (offset as i64 - delta).max(0) as usize
        } else {
            // 落在编辑区间内部的偏移没有旧身份。
            usize::MAX
        }
    };
    (map(range.0), map(range.1))
}

/// 从布局快照抓取受影响范围内的局部 cluster 片段。
///
/// Issue #904 复核 1：范围不只扩到改动行末尾，而是扩到**受影响段落的末尾**
/// （即改动点所在段落的全部后续折行）。插字挤到下一行、删除换行合并段落、
/// 插入换行使后续段落下移时，同段后续折行必须拿到完整 old/new 几何，才能在
/// matching 后按字节 offset 配成 Move，而不是被拆成 Reveal/Conceal。
///
/// Issue #904：这里**不按视口过滤**——屏幕外进入/离开的字必须保留完整几何，
/// 才能在 matching 之后按 `union(from, to)` 与视口相交，判成位置动画。
fn layout_fragments(
    layout: &EditorLayoutSnapshot,
    range: (usize, usize),
    revision: EditorRevision,
) -> Vec<VisibleFragment> {
    let mut affected: Vec<u64> = Vec::new();
    for line in layout.line_snapshots.iter() {
        if line.byte_start <= range.1 && line.byte_end >= range.0 {
            if !affected.contains(&line.id.paragraph_id) {
                affected.push(line.id.paragraph_id);
            }
        }
    }

    let mut span_end = range.1;
    for line in layout.line_snapshots.iter() {
        if affected.contains(&line.id.paragraph_id) {
            span_end = span_end.max(line.byte_end);
        }
    }
    let span = (range.0, span_end.max(range.1));

    let mut frags = Vec::new();
    for line in layout.line_snapshots.iter() {
        if line.byte_end < span.0 || line.byte_start > span.1 {
            continue;
        }
        for cluster in &line.clusters {
            if cluster.byte_end < span.0 || cluster.byte_start > span.1 {
                continue;
            }
            let rect = line.source_rect_to_document_rect(&cluster.source_rect);
            let byte_range = (cluster.byte_start, cluster.byte_end);
            frags.push(VisibleFragment {
                snapshot: SnapshotRef {
                    snapshot_id: line.id,
                    source_uv: cluster.source_rect,
                },
                byte_range,
                rect,
                mask_width: rect.w,
                paragraph_id: line.id.paragraph_id,
                source_revision: revision,
                glyph_id: derive_fragment_id(&line.id, byte_range),
            });
        }
    }
    frags
}

/// 复核 2：同 byte range 上一次真正画出的可见宽度。
///
/// 先找矩形一致的片段（同一字仍在原处），否则退回任意同 byte range 的片段。
fn drawn_mask_for(drawn: &[VisibleFragment], frag: &VisibleFragment) -> Option<f64> {
    drawn
        .iter()
        .find(|f| f.byte_range == frag.byte_range && rect_approx_eq(&f.rect, &frag.rect))
        .or_else(|| drawn.iter().find(|f| f.byte_range == frag.byte_range))
        .map(|f| f.mask_width)
}

/// 该字节范围是否落在本次编辑真正替换/删除的旧范围内。
fn in_any_old_patch(edit: &EditorEditResult, byte_range: (usize, usize)) -> bool {
    edit.display_patches.iter().any(|patch| {
        overlaps(
            byte_range,
            (
                patch.replace_byte_range.start().value(),
                patch.replace_byte_range.end().value(),
            ),
        )
    })
}

/// 复核 1：受影响段落以下、只发生整体 y 变化且在视口气带内的段落。
///
/// 只按段落 id 配对（换行插入/删除时下游段落 id 不变）；新出现的段落没有
/// 旧身份，其文字通过逐字 Move 处理。
fn collect_paragraph_moves(
    old_layout: Option<&EditorLayoutSnapshot>,
    new_layout: &EditorLayoutSnapshot,
    viewport: &LocalRect,
) -> Vec<ParagraphMove> {
    let Some(old_layout) = old_layout else {
        return Vec::new();
    };
    let old_tops = paragraph_tops(old_layout);
    let mut moves = Vec::new();
    for (paragraph_id, to_y) in paragraph_tops(new_layout) {
        let Some(from_y) = old_tops
            .iter()
            .find(|(id, _)| *id == paragraph_id)
            .map(|(_, y)| *y)
        else {
            continue;
        };
        if (from_y - to_y).abs() < 0.5 {
            continue;
        }
        // 屏幕外进/出沿用同一 viewport 裁剪：段落整体路径与视口相交才动画。
        let band = LocalRect {
            x: viewport.x,
            y: from_y.min(to_y),
            w: viewport.w,
            h: (to_y - from_y).abs().max(1.0),
        };
        if !rects_intersect(&band, viewport) {
            continue;
        }
        moves.push(ParagraphMove {
            paragraph_id,
            from_y,
            to_y,
        });
    }
    moves
}

/// 段落 id → 段落顶 y（文档坐标）。
fn paragraph_tops(layout: &EditorLayoutSnapshot) -> Vec<(u64, f64)> {
    let mut tops: Vec<(u64, f64)> = Vec::new();
    for line in &layout.line_snapshots {
        match tops.iter_mut().find(|(id, _)| *id == line.id.paragraph_id) {
            Some(entry) => entry.1 = entry.1.min(line.document_origin_y),
            None => tops.push((line.id.paragraph_id, line.document_origin_y)),
        }
    }
    tops
}

fn glyph_info(
    piece: &TextPiece,
    target_rect: LocalRect,
    source_uv: LocalRect,
    clip_rect: LocalRect,
) -> TextAnimationGlyphInfo {
    TextAnimationGlyphInfo {
        x: target_rect.x,
        y: target_rect.y,
        w: target_rect.w,
        h: target_rect.h,
        opacity: 1.0,
        snapshot_id: piece.source().snapshot_id,
        source_rect: source_uv,
        clip_rect,
        logical_range: piece.byte_range(),
        canonical_range: match piece {
            TextPiece::Reveal { .. } | TextPiece::Move { .. } => Some(piece.byte_range()),
            TextPiece::Conceal { .. } => None,
        },
        delete_edge: None,
        shaping_identity: super::layout_snapshot::ShapingIdentity::default(),
        source_revision: piece.source_revision(),
        glyph_id: piece.glyph_id(),
        fragment_index: 0,
        original_glyph_rect: target_rect,
    }
}

/// Issue #904：字形逻辑身份。取代已删除的
/// 与 #901 的 `derive_glyph_id` 算法保持一致（同 id 同确定性）。
pub(crate) fn derive_fragment_id(snapshot_id: &LineSnapshotId, byte_range: (usize, usize)) -> u64 {
    let mut h = snapshot_id.to_cache_key();
    h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = h.wrapping_add(byte_range.0 as u64);
    h = h.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = h.wrapping_add(byte_range.1 as u64);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    h
}

fn mask_rect(at: &LocalRect, direction: ClipDirection, visible: f64) -> LocalRect {
    let visible = visible.clamp(0.0, at.w);
    match direction {
        ClipDirection::LeftToRight => LocalRect {
            x: at.x,
            y: at.y,
            w: visible,
            h: at.h,
        },
        ClipDirection::RightToLeft => LocalRect {
            x: at.x + (at.w - visible),
            y: at.y,
            w: visible,
            h: at.h,
        },
    }
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn lerp_rect(from: &LocalRect, to: &LocalRect, t: f64) -> LocalRect {
    LocalRect {
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
        w: from.w + (to.w - from.w) * t,
        h: from.h + (to.h - from.h) * t,
    }
}

fn ease_out_cubic(t: f64) -> f64 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 <= b.1 && b.0 <= a.1
}

fn rects_intersect(a: &LocalRect, b: &LocalRect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn intersect_rect(a: &LocalRect, b: &LocalRect) -> LocalRect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    LocalRect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0.0),
        h: (y1 - y0).max(0.0),
    }
}

fn union_rect(a: &LocalRect, b: &LocalRect) -> LocalRect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    LocalRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

fn rect_approx_eq(a: &LocalRect, b: &LocalRect) -> bool {
    const EPS: f64 = 0.05;
    (a.x - b.x).abs() < EPS
        && (a.y - b.y).abs() < EPS
        && (a.w - b.w).abs() < EPS
        && (a.h - b.h).abs() < EPS
}
