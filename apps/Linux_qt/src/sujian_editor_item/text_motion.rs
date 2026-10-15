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

/// Issue #904：三类局部动画，共用同一个 progress。
#[derive(Clone, Debug)]
pub(crate) enum TextPiece {
    /// 新字逐渐露出（输入/粘贴）。
    Reveal {
        new_source: SnapshotRef,
        at: LocalRect,
        direction: ClipDirection,
        byte_range: (usize, usize),
        source_revision: EditorRevision,
        glyph_id: u64,
    },
    /// 旧字逐渐被覆盖（删除），仍有旧局部纹理可画。
    Conceal {
        old_source: SnapshotRef,
        at: LocalRect,
        direction: ClipDirection,
        byte_range: (usize, usize),
        source_revision: EditorRevision,
        glyph_id: u64,
    },
    /// 同一未删 cluster 从 `from` 移到 `to`（行内位移 / 换行 / 段落回流）。
    Move {
        source: SnapshotRef,
        from: LocalRect,
        to: LocalRect,
        byte_range: (usize, usize),
        source_revision: EditorRevision,
        glyph_id: u64,
    },
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

        // 旧侧：优先从“上一次真正画出的局部图片”续接（同一文档会话），
        // 使快速连输/连删直接在当前显示位置上重定向，而不是回到静态起点。
        let mut old_frags: Vec<VisibleFragment> = last_drawn
            .filter(|frame| frame.session == session)
            .map(|frame| {
                frame
                    .fragments
                    .iter()
                    .filter(|f| overlaps(f.byte_range, (old_start, old_end)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        // 再用编辑前布局补上不在已绘制画面里的邻近片段（例如改动点同行后续字）。
        if let Some(layout) = old_layout {
            for frag in layout_fragments(layout, (old_start, old_end), old_revision) {
                if !old_frags
                    .iter()
                    .any(|existing| existing.byte_range == frag.byte_range)
                {
                    old_frags.push(frag);
                }
            }
        }

        // 新侧：编辑后目标布局的局部片段。这里**不做**视口预过滤——屏幕外进入/
        // 离开的字只有拿到完整 old/new 几何后才能判成 Move，否则会被误判成
        // Reveal/Conceal，丢掉了位置动画。
        let new_frags = layout_fragments(new_layout, (new_start, new_end), new_revision);

        let direction = conceal_direction(edit);
        let delta = (new_end as i64 - new_start as i64) - (old_end as i64 - old_start as i64);

        let mut pieces: Vec<TextPiece> = Vec::new();
        let mut consumed_old: Vec<bool> = vec![false; old_frags.len()];

        for new_frag in &new_frags {
            // 把新坐标映回旧坐标以匹配未删 cluster。
            let mapped = map_new_to_old(new_frag.byte_range, new_start, new_end, delta);
            let matched = old_frags
                .iter()
                .enumerate()
                .find(|(i, f)| {
                    !consumed_old[*i] && f.byte_range == mapped && f.source_revision == old_revision
                })
                .map(|(i, _)| i);

            match matched {
                Some(idx) => {
                    consumed_old[idx] = true;
                    let old_frag = &old_frags[idx];
                    if !rect_approx_eq(&old_frag.rect, &new_frag.rect) {
                        pieces.push(TextPiece::Move {
                            source: new_frag.snapshot,
                            from: old_frag.rect,
                            to: new_frag.rect,
                            byte_range: new_frag.byte_range,
                            source_revision: old_revision,
                            glyph_id: new_frag.glyph_id,
                        });
                    }
                }
                None => {
                    pieces.push(TextPiece::Reveal {
                        new_source: new_frag.snapshot,
                        at: new_frag.rect,
                        direction: ClipDirection::LeftToRight,
                        byte_range: new_frag.byte_range,
                        source_revision: new_revision,
                        glyph_id: new_frag.glyph_id,
                    });
                }
            }
        }

        for (idx, old_frag) in old_frags.iter().enumerate() {
            if consumed_old[idx] {
                continue;
            }
            pieces.push(TextPiece::Conceal {
                old_source: old_frag.snapshot,
                at: old_frag.rect,
                direction,
                byte_range: old_frag.byte_range,
                source_revision: old_revision,
                glyph_id: old_frag.glyph_id,
            });
        }

        // Move 用“来源与目标矩形的并集”判断是否与视口相交，屏幕外进入/离开的字
        // 只要路径经过视口就保留；Reveal/Conceal 用自身矩形。
        pieces.retain(|piece| rects_intersect(&piece.motion_rect(), &viewport));
        if pieces.is_empty() {
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

        // 过渡结束：不再挖孔、不再画覆盖层；同一帧静态层恢复完整 canonical。
        if finished {
            return TextMotionFrame {
                static_holes: Vec::new(),
                overlays: Vec::new(),
                required_textures: self.required_textures.clone(),
                finished: true,
                needs_next_frame: false,
                caret_sample: self.caret_to,
            };
        }

        let mut overlays = Vec::with_capacity(self.pieces.len());
        for piece in &self.pieces {
            let (target_rect, source_uv, clip_rect) = match piece {
                TextPiece::Reveal {
                    new_source,
                    at,
                    direction,
                    ..
                } => {
                    let visible = at.w * progress;
                    (
                        *at,
                        new_source.source_uv,
                        mask_rect(at, *direction, visible),
                    )
                }
                TextPiece::Conceal {
                    old_source,
                    at,
                    direction,
                    ..
                } => {
                    let visible = at.w * (1.0 - progress);
                    (
                        *at,
                        old_source.source_uv,
                        mask_rect(at, *direction, visible),
                    )
                }
                TextPiece::Move {
                    source, from, to, ..
                } => {
                    let rect = lerp_rect(from, to, progress);
                    (rect, source.source_uv, rect)
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

/// 从布局快照抓取与 `[start, end]` 相交的局部 cluster 片段。
///
/// 为了覆盖"位移"，范围会扩到受影响行的末尾（这些行的后续字会左右滑动）。
///
/// Issue #904：这里**不按视口过滤**——屏幕外进入/离开的字必须保留完整几何，
/// 才能在 matching 之后按 `union(from, to)` 与视口相交，判成位置动画。
fn layout_fragments(
    layout: &EditorLayoutSnapshot,
    range: (usize, usize),
    revision: EditorRevision,
) -> Vec<VisibleFragment> {
    let mut lines_end = range.1;
    for line in layout.line_snapshots.iter() {
        if line.byte_end >= range.0 && line.byte_start <= range.1 {
            lines_end = lines_end.max(line.byte_end);
        }
    }
    let span = (range.0, lines_end.max(range.1));

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
                source_revision: revision,
                glyph_id: derive_fragment_id(&line.id, byte_range),
            });
        }
    }
    frags
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
