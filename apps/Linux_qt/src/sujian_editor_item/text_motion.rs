//! Issue #919：正文吞吐动画的唯一本地计划。
//!
//! 本模块取代 #901 的整章 `visual_canvas` 视觉状态机。它只回答一个问题：
//! **这一次编辑，哪些局部字形片段需要在若干毫秒内从中途显示出来**
//! （吐字 / 吞字 / 位移），并按同一个 `progress` 采样出本帧要画的局部覆盖层
//! 和静态层要挖掉的孔洞。
//!
//! Issue #919 重做：移除逐字纹理身份匹配（原 `VisibleFragment` 的
//! `byte_range + source_revision + glyph_id + shaping`），改用
//! `VisualSlice` / `SliceMotion` 以 `display_patches` 的删除/插入/未改区间分类。
//! 遮罩与位移直接使用旧/新排版画面，不再按字节位置把不同字形认成同一字。
//!
//! 边界：
//! - 只持有局部片段的几何与源纹理引用，不创建/删除任何 Qt `QSGNode`。
//! - 不重建整屏文字；`old_layout` / `new_layout` 分别给出编辑前后的局部
//!   cluster 几何（文档坐标）与源 UV，未变化部分不产生任何命令。
//! - 只有一个动画进度：`sample()` 里 `p = clamp((now-started)/duration, 0, 1)`
//!   计算一次，吐字/吞字/位移共用。
//! - 新旧字形按 `QTextLayout` 的 cluster 边界（Core 的 UTF-8 byte range）
//!   整体区分，不猜半个字节。

use super::layout_snapshot::{EditorLayoutSnapshot, ShapingIdentity, SourceRect};
use super::qt_text_node::AnimationClipRect;
use super::render_plan::TextAnimationGlyphInfo;
use super::snapshot_id::LineSnapshotId;
use writer_core::editor::EditorEditResult;

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

/// Issue #919 复核：运动类别，用于 `DrawnSlice` 记录本帧实际显示的运动类型，
/// 供连续编辑重定向时判断如何接续。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum MotionKind {
    #[default]
    Reveal,
    Conceal,
    Shift,
}

/// Issue #904：一个局部字形片段的源纹理引用（快照 id + 纹理内 UV）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SnapshotRef {
    pub snapshot_id: LineSnapshotId,
    /// 源行纹理内的 UV（像素，已含 DPR）。
    pub source_uv: LocalRect,
}

/// Issue #919：一个局部视觉片段：源纹理引用 + 文档坐标矩形。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VisualSlice {
    pub source: SnapshotRef,
    pub rect: LocalRect,
}

/// Issue #919：三类局部动画，共用同一个 progress。
///
/// 不再携带 `byte_range` / `glyph_id` / `shaping` 等逐字身份字段。
/// `byte_range` 仅用于连续编辑重定向时轻量片段关联，不是逐字身份。
#[derive(Clone, Debug)]
pub(crate) enum SliceMotion {
    /// 新字逐渐露出（输入/粘贴）。
    ///
    /// `start_clip` 是动画起点的原始 mask clip（viewport 裁剪前）：progress=0 时
    /// 可见宽度 = `start_clip.w`，progress=1 时扩到 `new.rect` 满宽。连续输入重定向
    /// 时 `start_clip` 取自上一帧 `DrawnSlice.mask_clip`，位置/宽度都继承，不从
    /// `new.rect` 边界重新造遮罩。
    Reveal {
        new: VisualSlice,
        direction: ClipDirection,
        start_clip: LocalRect,
        byte_range: (usize, usize),
        /// 本片段的 shaping 指纹（new 版），用于连续编辑接续时确认同一份字形。
        shaping_identity: ShapingIdentity,
    },
    /// 旧字逐渐被覆盖（删除）。
    ///
    /// `start_clip` 是动画起点的原始 mask clip（viewport 裁剪前）：progress=0 时
    /// 可见宽度 = `start_clip.w`，progress=1 时缩到 0。连续删除重定向时 `start_clip`
    /// 取自上一帧 `DrawnSlice.mask_clip`，裁剪起止位置继承，不从 `old.rect` 边界
    /// 重新造遮罩——已有 clip 被视口/上一动画局部裁过时，下笔不会把可见部分移到
    /// 旧矩形另一边。
    Conceal {
        old: VisualSlice,
        direction: ClipDirection,
        start_clip: LocalRect,
        byte_range: (usize, usize),
        /// 本片段的 shaping 指纹（old 版），用于连续编辑接续时确认同一份字形。
        shaping_identity: ShapingIdentity,
    },
    /// 未删 cluster 从 `from` 积到 `to`（行内位移/换行/段落回流）。
    /// 坐标 = `(lerp(from.x,to.x,p), lerp(from.y,to.y,p))`，宽高固定为 new 版尺寸，
    /// 采样 new 版字形纹理。只插值位置不缩放字形。
    Shift {
        new: VisualSlice,
        from: LocalRect,
        to: LocalRect,
        byte_range: (usize, usize),
        /// 本片段的 shaping 指纹（new 版），用于连续编辑接续时确认同一份字形。
        shaping_identity: ShapingIdentity,
    },
}

impl SliceMotion {
    /// 该片段的运动类别。
    fn motion_kind(&self) -> MotionKind {
        match self {
            SliceMotion::Reveal { .. } => MotionKind::Reveal,
            SliceMotion::Conceal { .. } => MotionKind::Conceal,
            SliceMotion::Shift { .. } => MotionKind::Shift,
        }
    }

    /// 该片段的所属字节范围（用于连续编辑重定向时轻量片段关联）。
    fn byte_range(&self) -> (usize, usize) {
        match self {
            SliceMotion::Reveal { byte_range, .. } => *byte_range,
            SliceMotion::Conceal { byte_range, .. } => *byte_range,
            SliceMotion::Shift { byte_range, .. } => *byte_range,
        }
    }

    /// 该片段的 shaping 指纹（用于连续编辑接续时确认同一份字形）。
    fn shaping_identity(&self) -> &ShapingIdentity {
        match self {
            SliceMotion::Reveal { shaping_identity, .. } => shaping_identity,
            SliceMotion::Conceal { shaping_identity, .. } => shaping_identity,
            SliceMotion::Shift { shaping_identity, .. } => shaping_identity,
        }
    }

    /// 该片段的裁剪方向（Shift 无裁剪方向，返回 None）。
    fn direction(&self) -> Option<ClipDirection> {
        match self {
            SliceMotion::Reveal { direction, .. } => Some(*direction),
            SliceMotion::Conceal { direction, .. } => Some(*direction),
            SliceMotion::Shift { .. } => None,
        }
    }

    /// 该片段的运动包络矩形（与视口求交用）。
    fn motion_rect(&self) -> LocalRect {
        match self {
            SliceMotion::Reveal { new, .. } => new.rect,
            SliceMotion::Conceal { old, .. } => old.rect,
            SliceMotion::Shift { from, to, .. } => union_rect(from, to),
        }
    }

    /// 该片段引用的源纹理 id。
    fn source_snapshot_id(&self) -> LineSnapshotId {
        match self {
            SliceMotion::Reveal { new, .. } => new.source.snapshot_id,
            SliceMotion::Conceal { old, .. } => old.source.snapshot_id,
            SliceMotion::Shift { new, .. } => new.source.snapshot_id,
        }
    }

    /// 该片段引用的完整源纹理引用（snapshot_id + source_uv），用于精确区分
    /// 不同旧视觉源（同 byte_range 但不同 snapshot/UV 的旧图不能互相继承）。
    fn source_ref(&self) -> &SnapshotRef {
        match self {
            SliceMotion::Reveal { new, .. } => &new.source,
            SliceMotion::Conceal { old, .. } => &old.source,
            SliceMotion::Shift { new, .. } => &new.source,
        }
    }

    /// 静态层排除矩形（Reveal 排除新位置，Shift 排除目标位置，Conceal 不排除）。
    fn static_exclusion(&self) -> Option<AnimationClipRect> {
        match self {
            SliceMotion::Reveal { new, .. } => Some(AnimationClipRect {
                x: new.rect.x,
                y: new.rect.y,
                w: new.rect.w,
                h: new.rect.h,
                snapshot_id: new.source.snapshot_id,
            }),
            SliceMotion::Shift { new, to, .. } => Some(AnimationClipRect {
                x: to.x,
                y: to.y,
                w: to.w,
                h: to.h,
                snapshot_id: new.source.snapshot_id,
            }),
            SliceMotion::Conceal { .. } => None,
        }
    }
}

/// Issue #919：上一次真正画出的局部视觉片段（用于连续编辑重定向）。
///
/// 记录本帧实际显示的 `VisualSlice`、文档坐标 rect、有效 clip 矩形、运动类别
/// 和所属字节范围，供下一笔编辑从当前可见画面继续。
#[derive(Clone, Debug)]
pub(crate) struct DrawnSlice {
    pub slice: VisualSlice,
    /// 当前可见宽度（视口裁剪后，用于判断旧图是否仍在途）。
    pub visible_width: f64,
    /// 本帧有效裁剪矩形（视口裁剪后，文档坐标 x/y/w/h）。
    pub clip_rect: LocalRect,
    /// Issue #919 复核 5：`sample()` 产生的原始 mask clip（视口裁剪前）。
    ///
    /// 下一笔接续时用它的左右边界和宽度作为动画起点，而不是仅用 `visible_width`
    /// 从源矩形边界重新造遮罩。视口/上一动画局部裁过时，`clip_rect` 的位置与
    /// `mask_clip` 不同；只用 `visible_width` 会让下笔把可见部分移到旧矩形另一边。
    pub mask_clip: LocalRect,
    /// 运动类别（用于连续编辑重定向时判断如何接续）。
    pub motion_kind: MotionKind,
    /// 所属字节范围（用于连续编辑时轻量片段关联）。
    pub byte_range: (usize, usize),
    /// 本片段的 shaping 指纹：接续时确认与本次新片段是同一份内容/字体 shaping，
    /// 同 byte_range 但字形变了必须保留旧图退场、新图从零露出。
    pub shaping_identity: ShapingIdentity,
    /// 本片段的裁剪方向：旧 Conceal 继续吞时沿用原方向，不用本次编辑方向。
    pub direction: ClipDirection,
}

/// Issue #919：上一次真正提交显示的局部画面。
#[derive(Clone, Debug)]
pub(crate) struct LocalDrawnFrame {
    pub session: u64,
    pub revision: u64,
    pub slices: Vec<DrawnSlice>,
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

/// Issue #919：一次编辑产生的唯一动画对象。
#[derive(Clone, Debug)]
pub(crate) struct TextMotion {
    pub slices: Vec<SliceMotion>,
    pub started_ms: f64,
    pub duration_ms: u32,
    pub session: u64,
    pub target_revision: u64,
    /// 动画最终裁剪到的视口（文档坐标）。
    pub viewport: LocalRect,
    /// 静态层需排除的目标字形矩形（Reveal 的新位置 / Shift 的目标位置）。
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
    /// Issue #919：构造或重定向一次局部动画。
    ///
    /// 返回 `None` 表示这次编辑没有可动画的局部片段（纯光标移动等），
    /// 调用方应直接走静态绘制。
    ///
    /// 以 `edit.display_patches` 分类：
    /// - `replace_byte_range`（旧坐标）交旧布局 → `Conceal`（只用旧版画面）；
    /// - `inserted_byte_range`（新坐标）交新布局 → `Reveal`（只用新版画面）；
    /// - patch 外未改区间 → 用偏移映射在 old/new 布局中定位，几何不同则 `Shift`
    ///   （`from`=旧 rect, `to`=新 rect, 采样 new 版字形纹理）。
    ///
    /// `last_drawn` 用于连续快速输入：由于移除了逐字身份匹配，重定向简化为
    /// 从当前可见画面继续，不再按 byte_range 匹配旧遮罩。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_or_retarget(
        edit: &EditorEditResult,
        old_layout: Option<&EditorLayoutSnapshot>,
        new_layout: &EditorLayoutSnapshot,
        last_drawn: Option<&LocalDrawnFrame>,
        now_ms: f64,
        duration_ms: u32,
        session: u64,
        viewport: LocalRect,
    ) -> Option<Self> {
        let (old_start, old_end, new_start, new_end) = edit_ranges(edit)?;
        if old_start == old_end && new_start == new_end {
            return None;
        }

        let direction = conceal_direction(edit);

        // 新侧布局片段（含 byte_range / VisualSlice / paragraph_id）。
        let new_spans = layout_spans(new_layout, (new_start, new_end));
        // 旧侧布局片段。
        let old_spans: Vec<LayoutSpan> = match old_layout {
            Some(layout) => layout_spans(layout, (old_start, old_end)),
            None => Vec::new(),
        };

        // Issue #919 复核 1：连续快速输入/删除重定向。
        // 把 last_drawn 的 slices 作为旧当前视觉源：还在吐的沿已露出宽度继续，
        // 正在吞的沿当前有效 clip 继续，Shift 从已经插值显示的 rect 接新目标。
        // 保留上一帧仍在屏幕上的旧版本纹理至本次动画不再引用为止。
        // Issue #919 复核 2：同 byte_range 可能同时有 Reveal+Conceal（同长度替换），
        // 用 Vec 允许同位置多条 DrawnSlice 并存，使用时按 motion_kind 筛选。
        let last_drawn_map: std::collections::HashMap<(usize, usize), Vec<&DrawnSlice>> =
            last_drawn
                .filter(|frame| frame.session == session)
                .map(|frame| {
                    let mut map: std::collections::HashMap<(usize, usize), Vec<&DrawnSlice>> =
                        std::collections::HashMap::new();
                    for ds in &frame.slices {
                        map.entry(ds.byte_range).or_default().push(ds);
                    }
                    map
                })
                .unwrap_or_default();

        // 受影响段落 id（用于整体段落位移候选）。
        let mut affected_ids: Vec<u64> = Vec::new();
        for span in new_spans.iter().chain(old_spans.iter()) {
            if span.paragraph_id != 0 {
                affected_ids.push(span.paragraph_id);
            }
        }
        // Issue #919 复核 2：从 display_patches 直接确定真正内容变更的稳定 ParagraphId
        // 集合（旧/新版分别找，不把 layout_spans 扩展取到的未改后续行算成内容已改）。
        let content_changed_ids =
            content_changed_paragraph_ids(edit, old_layout, new_layout);
        // collect_paragraph_moves 只输出未变内容、纯整体移动的段落。
        let paragraph_moves = collect_paragraph_moves(old_layout, new_layout, &viewport, &affected_ids)
            .into_iter()
            .filter(|m| !content_changed_ids.contains(&m.paragraph_id))
            .collect::<Vec<_>>();
        // moved_paragraphs 直接取最终的 paragraph_moves，不再过滤一份、渲染另一份。
        // 同一段落只能由一种位移路径接管。
        let moved_paragraphs: Vec<u64> = paragraph_moves
            .iter()
            .map(|m| m.paragraph_id)
            .collect();

        let mut slices: Vec<SliceMotion> = Vec::new();
        let mut consumed_old: Vec<bool> = vec![false; old_spans.len()];
        // Issue #919 复核 5：已被本计划 Conceal 接管的旧视觉源（snapshot_id + source_uv）。
        // 第一遍为替换旧字退场加的 Conceal 和第二遍为本轮被替换旧字加的 Conceal 不能
        // 对同一旧视觉源各加一份——否则"甲"吐到一半被"乙"替换时，半个"甲"退场一次、
        // 完整"甲"又退场一次，旧字瞬间补全、两张旧纹理覆盖，产生闪字/重影。
        let mut claimed_conceal_sources: Vec<SnapshotRef> = Vec::new();

        // 第一遍：对每个新片段分类 Reveal / Shift。
        for new_span in &new_spans {
            // 交给整体段落位移的段落，不生成逐字覆盖层。
            if moved_paragraphs.contains(&new_span.paragraph_id) {
                continue;
            }
            // 落在某个 patch 的 inserted_byte_range 内 → Reveal（新字露出）。
            if inserted_in_patch(edit, new_span.byte_range) {
                // Issue #919 复核 3：新插入的字从 last_drawn 查找同 byte_range 的
                // 未完成 Reveal。必须核对 shaping_identity：相同字形才继承已露宽度
                // （连续输入同一位置继续吐）；不同字形说明是替换/输入法换字，旧图退场
                // （Conceal 沿用原方向从当前 mask clip 继续吞到 0）+ 新图从零露出。
                // byte_range 只做候选定位，不单独作为接续判据。
                // 复核 5：start_clip 取自上一帧 mask_clip（视口裁剪前的原始动画遮罩），
                // 不只用 visible_width 从源矩形边界重新造遮罩。
                let last_reveal = last_drawn_map
                    .get(&new_span.byte_range)
                    .and_then(|v| v.iter().find(|ds| ds.motion_kind == MotionKind::Reveal));
                let start_clip = match last_reveal {
                    Some(ds)
                        if ds.shaping_identity
                            .is_same_shaping(&new_span.shaping_identity) =>
                    {
                        // 同一字形：连续输入同一位置，从上一帧 mask clip 继续吐。
                        ds.mask_clip
                    }
                    Some(ds) if ds.visible_width > 0.01 => {
                        // 不同字形但旧图仍在途：旧图退场 + 新图从零露出。
                        // 登记此旧视觉源已由 Conceal 接管，第二遍不再为同一源加 Conceal。
                        claimed_conceal_sources.push(ds.slice.source);
                        slices.push(SliceMotion::Conceal {
                            old: ds.slice,
                            direction: ds.direction,
                            start_clip: ds.mask_clip,
                            byte_range: ds.byte_range,
                            shaping_identity: ds.shaping_identity.clone(),
                        });
                        mask_rect(&new_span.slice.rect, ClipDirection::LeftToRight, 0.0)
                    }
                    _ => mask_rect(&new_span.slice.rect, ClipDirection::LeftToRight, 0.0),
                };
                slices.push(SliceMotion::Reveal {
                    new: new_span.slice,
                    direction: ClipDirection::LeftToRight,
                    start_clip,
                    byte_range: new_span.byte_range,
                    shaping_identity: new_span.shaping_identity.clone(),
                });
                continue;
            }
            // patch 外未改区间的新侧：用偏移映射映回旧坐标，找旧片段配 Shift。
            // Issue #919：起点用右偏映射、终点用左偏映射，避免删除边界处未改
            // 文本被错映到删除后坐标而误判成 Reveal（见 new_to_old_byte_end）。
            let old_byte_range = (
                new_to_old_byte(edit, new_span.byte_range.0),
                new_to_old_byte_end(edit, new_span.byte_range.1),
            );

            // Issue #919 复核 1：连续编辑重定向——先查 last_drawn 中是否有同
            // 旧 byte_range 的未完成动画，从当前可见画面接续。
            // 复核 2：同 byte_range 可能并存 Reveal+Conceal，先查 Reveal 再查 Shift。
            let last_vec = last_drawn_map.get(&old_byte_range);
            // 先查未完成 Reveal（连续输入同一位置继续吐）
            let last_reveal = last_vec
                .and_then(|v| v.iter().find(|ds| ds.motion_kind == MotionKind::Reveal).copied());
            if let Some(last) = last_reveal {
                // Issue #919 复核 3：必须核对 shaping_identity，不同字形不能继承。
                if last.shaping_identity.is_same_shaping(&new_span.shaping_identity)
                    && last.visible_width < new_span.slice.rect.w - 0.01
                {
                    // 上一笔还在吐同一字形，本笔继续吐到满宽。
                    if let Some(idx) =
                        old_spans.iter().position(|s| s.byte_range == old_byte_range)
                    {
                        consumed_old[idx] = true;
                    }
                    slices.push(SliceMotion::Reveal {
                        new: new_span.slice,
                        direction: ClipDirection::LeftToRight,
                        start_clip: last.mask_clip,
                        byte_range: new_span.byte_range,
                        shaping_identity: new_span.shaping_identity.clone(),
                    });
                    continue;
                }
            }
            // 再查 Shift（从当前位移接续）
            // Issue #919 复核 16：合并 Shift 的接续。merge_adjacent_shifts() 把多条
            // Shift 合为一条（如"乙"(9..12)+"丙"(12..15) → (9,15)），sample() 只产出
            // 合并后的单条 DrawnSlice。下一笔编辑逐 cluster 枚举 new_spans，"乙"的
            // old_byte_range=(9,12) 精确查 last_drawn_map 找不到合并键 (9,15)。
            // 先精确匹配，再遍历查找包含 old_byte_range 的合并 Shift，从合并 mask_clip
            // 中按 cluster 在合并 UV 中的相对偏移还原当前 cluster 的当前显示位置。
            // 先找本轮 old_span（patch 外未改区间对应的旧片段），供合并 Shift
            // 核对来源 snapshot_id / source_uv 是否属于已提交的合并源。
            let old_span_match = old_spans
                .iter()
                .enumerate()
                .find(|(i, s)| !consumed_old[*i] && s.byte_range == old_byte_range);
            let last_shift_exact = last_vec
                .and_then(|v| v.iter().find(|ds| ds.motion_kind == MotionKind::Shift).copied());
            let last_merged = if last_shift_exact.is_none() && old_span_match.is_some() {
                let idx = old_span_match.unwrap().0;
                find_merged_shift(&last_drawn_map, old_byte_range, &old_spans[idx].slice.source)
            } else {
                None
            };
            let last_shift = last_shift_exact.or(last_merged);
            if let Some(last) = last_shift {
                // 上一笔在位移，本笔从当前已插值显示的 rect 接新目标。
                if let Some((idx, _)) = old_span_match {
                    consumed_old[idx] = true;
                    let old_span = &old_spans[idx];
                    let from_merged_shift = last_merged.is_some();
                    // Issue #919 复核 15/16：Shift 接续的起点取上一帧未裁剪的
                    // 当前画面位置。精确匹配时 = `last.mask_clip`（`sample()` 对
                    // Shift 产生的完整插值 rect）；合并匹配时由
                    // `restore_cluster_rect` 从合并 `mask_clip` 还原当前 cluster
                    // 的插值位置。还原用 `old_span` 的 UV（与合并片段同源），
                    // 不用 `new_span` 的 UV（新布局中 cluster 可能独占一行，UV 从
                    // 0 开始，无法在合并 UV 中定位）。不用视口裁剪后的 `clip_rect`：
                    // 被视口裁掉一部分的位移字形，若用 `clip_rect` 的可见边缘当起点、
                    // 再按 new 版固定宽度绘制，可见部分会被挪到视口边缘，内容切片错位。
                    let shift_from = if from_merged_shift {
                        restore_cluster_rect(
                            &last.mask_clip,
                            &last.slice.source.source_uv,
                            &old_span.slice.source.source_uv,
                            &new_span.slice.rect,
                        )
                    } else {
                        last.mask_clip
                    };
                    // Issue #919 复核 3：shaping 不同则旧退出+新进入。
                    if old_span
                        .shaping_identity
                        .is_same_shaping(&new_span.shaping_identity)
                    {
                        let from = shift_from;
                        let to = new_span.slice.rect;
                        if !rect_approx_eq(&from, &to) {
                            slices.push(SliceMotion::Shift {
                                new: new_span.slice,
                                from,
                                to,
                                byte_range: new_span.byte_range,
                                shaping_identity: new_span.shaping_identity.clone(),
                            });
                        }
                    } else {
                        // Issue #919 复核 7/16：Shift 中途被替换（shaping 变化）时，
                        // 退场位置取上一帧真正显示的画面，而不是 old_span.slice.rect
                        // （上一笔编辑已排版完成的 canonical 目标位置）。正在 Shift
                        // 的字当前画在中途位置，从该位置以本次编辑遮罩方向生成旧图
                        // Conceal，避免从 canonical 目标位置突然开始退场而向目标方
                        // 向闪一下。新字仍从 new_span.slice 做 Reveal。
                        if from_merged_shift {
                            // 合并匹配：`last.slice` 是合并 VisualSlice（合并 UV/rect），
                            // 不能直接用作单 cluster Conceal。用 `old_span.slice` 的单
                            // cluster 源纹理，但 rect 取上一帧已插值显示的 `shift_from`
                            // （而非 `old_span.slice.rect` canonical 目标位置），
                            // `start_clip` 同样用 `shift_from`，使 Conceal 的 target
                            // 与 clip 对齐、有非零交集。否则跨行位移距离较大时，
                            // `old.rect`（canonical 终点）与 `start_clip`（插值位置）
                            // 不相交，`scene_graph_renderer` 做交集会跳过整个 glyph，
                            // 旧字从未开始吞就直接消失。
                            // 前提：find_merged_shift 已保证 old_span 的 UV 确实属于
                            // 已提交的合并源（snapshot_id + source_uv 包含核对）。
                            let old_at_current = VisualSlice {
                                source: old_span.slice.source,
                                rect: shift_from,
                            };
                            claimed_conceal_sources.push(old_at_current.source);
                            slices.push(SliceMotion::Conceal {
                                old: old_at_current,
                                direction,
                                start_clip: shift_from,
                                byte_range: old_span.byte_range,
                                shaping_identity: old_span.shaping_identity.clone(),
                            });
                        } else {
                            // 精确匹配：沿用上一帧的 slice 和 mask_clip。
                            claimed_conceal_sources.push(last.slice.source);
                            slices.push(SliceMotion::Conceal {
                                old: last.slice,
                                direction,
                                start_clip: last.mask_clip,
                                byte_range: old_span.byte_range,
                                shaping_identity: old_span.shaping_identity.clone(),
                            });
                        }
                        slices.push(SliceMotion::Reveal {
                            new: new_span.slice,
                            direction: ClipDirection::LeftToRight,
                            start_clip: mask_rect(
                                &new_span.slice.rect,
                                ClipDirection::LeftToRight,
                                0.0,
                            ),
                            byte_range: new_span.byte_range,
                            shaping_identity: new_span.shaping_identity.clone(),
                        });
                    }
                    continue;
                }
            }

            let matched = old_spans
                .iter()
                .enumerate()
                .find(|(i, s)| {
                    !consumed_old[*i]
                        && s.byte_range == old_byte_range
                        && !moved_paragraphs.contains(&s.paragraph_id)
                })
                .map(|(i, _)| i);
            match matched {
                Some(idx) => {
                    consumed_old[idx] = true;
                    let old_span = &old_spans[idx];
                    // Issue #919 复核 3：shaping 边界检查。
                    // shaping 不同 → 旧退出+新进入；相同且几何不同 → Shift。
                    if !old_span
                        .shaping_identity
                        .is_same_shaping(&new_span.shaping_identity)
                    {
                        claimed_conceal_sources.push(old_span.slice.source);
                        slices.push(SliceMotion::Conceal {
                            old: old_span.slice,
                            direction,
                            start_clip: old_span.slice.rect,
                            byte_range: old_span.byte_range,
                            shaping_identity: old_span.shaping_identity.clone(),
                        });
                        slices.push(SliceMotion::Reveal {
                            new: new_span.slice,
                            direction: ClipDirection::LeftToRight,
                            start_clip: mask_rect(
                                &new_span.slice.rect,
                                ClipDirection::LeftToRight,
                                0.0,
                            ),
                            byte_range: new_span.byte_range,
                            shaping_identity: new_span.shaping_identity.clone(),
                        });
                    } else if !rect_approx_eq(&old_span.slice.rect, &new_span.slice.rect) {
                        // shaping 相同、几何不同 → Shift（采样 new 版字形纹理）。
                        slices.push(SliceMotion::Shift {
                            new: new_span.slice,
                            from: old_span.slice.rect,
                            to: new_span.slice.rect,
                            byte_range: new_span.byte_range,
                            shaping_identity: new_span.shaping_identity.clone(),
                        });
                    }
                    // shaping 相同、几何相同：静态，不产生动画。
                }
                None => {
                    // 没有旧对应（新行/新段落/旧布局缺失）：归为 Reveal。
                    slices.push(SliceMotion::Reveal {
                        new: new_span.slice,
                        direction: ClipDirection::LeftToRight,
                        start_clip: mask_rect(
                            &new_span.slice.rect,
                            ClipDirection::LeftToRight,
                            0.0,
                        ),
                        byte_range: new_span.byte_range,
                        shaping_identity: new_span.shaping_identity.clone(),
                    });
                }
            }
        }

        // 第二遍：未被消费且被真正替换/删除的旧片段 → Conceal。
        // Issue #919 复核 5：已被第一遍 Conceal 接管的旧视觉源（claimed_conceal_sources）
        // 不再重复加 Conceal——同一旧图只退场一次，避免"甲"吐到一半被"乙"替换时
        // 上一帧半个"甲"退场一次、当前旧布局完整"甲"又退场一次的双 Conceal。
        for (idx, old_span) in old_spans.iter().enumerate() {
            if consumed_old[idx]
                || moved_paragraphs.contains(&old_span.paragraph_id)
                || !replaced_in_patch(edit, old_span.byte_range)
                || claimed_conceal_sources.contains(&old_span.slice.source)
            {
                continue;
            }
            // Issue #919 复核 3：连续删除重定向——只继承相同真实旧视觉源
            // （snapshot_id + source_uv 相同）的未完成 Conceal 的裁剪起点和方向，
            // 否则从自身完整旧图开始吞。不用 byte_range 单独作为接续判据：
            // 同位置但不同旧图（如上一笔删的字和本轮删的字碰巧同 byte_range）
            // 不能互相继承遮罩宽度。
            // 复核 5：start_clip 取自上一帧 mask_clip（视口裁剪前的原始动画遮罩），
            // 继承裁剪起止位置，不只继承 visible_width。
            // Issue #919 复核 6：以 old_span.slice.source 为键从上一帧找对应的
            // 当前真实显示图片，允许上一状态是 Reveal 或 Conceal。若命中且当前 clip
            // 有可见区域，则 start_clip=ds.mask_clip——半字正在露出时立即 Backspace，
            // 从已露遮罩继续吞到 0，不先把半字补成整字再吞（实际打字最常见的闪字）。
            // 此前是 Conceal 时沿用 ds.direction；此前是 Reveal 而现在发生删除时使用
            // 本次删除方向。已经从第一遍声明接管的同一源不再重复添加，沿用现有
            // claimed_conceal_sources（第二遍开头已含 source 跳过检查）。
            let inherited = last_drawn
                .filter(|f| f.session == session)
                .and_then(|frame| {
                    frame.slices.iter().find(|ds| {
                        ds.slice.source == old_span.slice.source && ds.visible_width > 0.01
                    })
                });
            let (start_clip, conceal_direction) = match inherited {
                Some(ds) if ds.motion_kind == MotionKind::Conceal => (ds.mask_clip, ds.direction),
                Some(ds) => (ds.mask_clip, direction),
                None => (old_span.slice.rect, direction),
            };
            slices.push(SliceMotion::Conceal {
                old: old_span.slice,
                direction: conceal_direction,
                start_clip,
                byte_range: old_span.byte_range,
                shaping_identity: old_span.shaping_identity.clone(),
            });
        }

        // Issue #919 复核 2：上一笔删除但仍在途的旧 Conceal——被删字已不在当前 Core
        // 正文，不会进入 old_spans；从 last_drawn 直接沿用其画面继续吞到 0。
        // 复核 3：每条旧 Conceal 的持有者由实际 source.snapshot_id + source_uv 区分；
        // 被上一笔删除的旧图是独立在途退出像素，不能因本轮 old_spans 的字节范围
        // 碰巧相同就跳过。继续使用它原本的 source + clip_rect + direction 从当前
        // 裁剪形状收至 0，不用本次编辑方向。
        if let Some(frame) = last_drawn.filter(|f| f.session == session) {
            for ds in &frame.slices {
                if ds.motion_kind != MotionKind::Conceal || ds.visible_width <= 0.01 {
                    continue;
                }
                // 已被本轮相同真实旧视觉源接管的跳过（第二遍用 source identity 继承）。
                if old_spans
                    .iter()
                    .any(|s| s.slice.source == ds.slice.source)
                {
                    continue;
                }
                // 同 source 已被本计划 Conceal 接管的跳过。
                if slices.iter().any(|s| {
                    s.motion_kind() == MotionKind::Conceal && s.source_ref() == &ds.slice.source
                }) {
                    continue;
                }
                slices.push(SliceMotion::Conceal {
                    old: ds.slice,
                    direction: ds.direction,
                    start_clip: ds.mask_clip,
                    byte_range: ds.byte_range,
                    shaping_identity: ds.shaping_identity.clone(),
                });
            }
        }

        // Issue #919 复核 16：patch 外未改的连续文本按旧/新视觉行的连续片段
        // 分组——把同一旧/新视觉行、字节区间连续、源 UV 连续、位移向量相同的
        // 相邻 Shift 合并成一条连续 VisualSlice，而不是每个 cluster 各一条。
        slices = merge_adjacent_shifts(slices);

        // 视口过滤：Shift 用 from/to 并集，Reveal/Conceal 用自身矩形。
        slices.retain(|s| rects_intersect(&s.motion_rect(), &viewport));
        if slices.is_empty() && paragraph_moves.is_empty() {
            return None;
        }

        // 静态层排除矩形（与 progress 无关）。
        let static_exclusions: Vec<AnimationClipRect> =
            slices.iter().filter_map(|s| s.static_exclusion()).collect();

        // 所需源纹理：本次编辑涉及的全部片段。
        let mut required_textures: Vec<LineSnapshotId> = Vec::new();
        for s in &slices {
            let id = s.source_snapshot_id();
            if !required_textures.contains(&id) {
                required_textures.push(id);
            }
        }

        // 协同模式下光标共用同一 progress：起点/终点取布局真实 caret 几何。
        let caret_from = old_layout
            .and_then(|layout| layout.caret_rect_doc.as_ref())
            .map(|caret| (caret.x, caret.y));
        let caret_to = new_layout
            .caret_rect_doc
            .as_ref()
            .map(|caret| (caret.x, caret.y));

        Some(TextMotion {
            slices,
            started_ms: now_ms,
            duration_ms,
            session,
            target_revision: edit.new_revision.value(),
            viewport,
            static_exclusions,
            required_textures,
            caret_from,
            caret_to,
            paragraph_moves,
        })
    }

    /// Issue #919：按同一个 `progress` 采样本帧的局部覆盖层与静态孔洞。
    ///
    /// `p = ease_out_cubic(clamp((now-started)/duration, 0, 1))` 只算一次，
    /// Reveal/Conceal/Shift 共用：
    /// - Reveal 可见宽度 = `rect.w * p`；
    /// - Conceal 可见宽度 = `rect.w * (1-p)`；
    /// - Shift 坐标 = `from + (to-from)*p`，不裁剪宽度，只位移。
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

        let mut overlays = Vec::with_capacity(self.slices.len());
        for motion in &self.slices {
            // Issue #919 复核 5：mask_clip 是 sample() 产生的原始动画遮罩（视口裁剪前），
            // 传给 TextAnimationGlyphInfo → DrawnSlice，供下一笔接续时继承裁剪起止位置，
            // 不只用视口裁剪后的 visible_width 从源矩形边界重新造遮罩。
            let (target_rect, source_uv, clip_rect, mask_clip) = match motion {
                SliceMotion::Reveal {
                    new,
                    direction,
                    start_clip,
                    ..
                } => {
                    // Issue #919 复核 6：位置从 start_clip（上一帧已提交位置）插值到
                    // new.rect，遮罩左边界从 start_clip.x 推进到 new.rect.x，宽度由
                    // 已露出推进到目标全宽。上一帧正在露出的字若因下一笔在它前面插入
                    // 文字/换行而发生位置变化，从当前显示画面流畅移动到新目标，不瞬移。
                    // 普通新输入（start_clip=new.rect 位置、w=0）退化为从 new.rect
                    // 开始吐。源纹理始终用 new.source_uv，宽高固定 new 版，不缩放字形。
                    let target_rect = LocalRect {
                        x: lerp(start_clip.x, new.rect.x, progress),
                        y: lerp(start_clip.y, new.rect.y, progress),
                        w: new.rect.w,
                        h: new.rect.h,
                    };
                    let visible = start_clip.w + (new.rect.w - start_clip.w) * progress;
                    let clip = mask_rect(&target_rect, *direction, visible);
                    (target_rect, new.source.source_uv, clip, clip)
                }
                SliceMotion::Conceal {
                    old,
                    direction,
                    start_clip,
                    ..
                } => {
                    // Issue #919 复核 5：从 start_clip 继续吞到 0。
                    // Conceal 的位置由 start_clip 决定，不从 old.rect 边界重新造遮罩——
                    // 已有 clip 被视口/上一动画局部裁过时，下笔不会把可见部分移到
                    // 旧矩形另一边（右向左删除、换行边界不再跳变）。
                    let visible = start_clip.w * (1.0 - progress);
                    let clip = conceal_clip(start_clip, *direction, visible);
                    (old.rect, old.source.source_uv, clip, clip)
                }
                SliceMotion::Shift { new, from, to, .. } => {
                    // Issue #919 复核 3：只插值位置，宽高使用 new 版固定目标尺寸，
                    // 不缩放字形。在 QSG 侧优先用 QSGTransformNode 位移。
                    let rect = LocalRect {
                        x: lerp(from.x, to.x, progress),
                        y: lerp(from.y, to.y, progress),
                        w: new.rect.w,
                        h: new.rect.h,
                    };
                    // Shift 不裁剪宽度，只位移：clip = 完整当前 rect。
                    (rect, new.source.source_uv, rect, rect)
                }
            };

            // 与视口求交；完全离开视口的片段本帧不画（已是 0 面积）。
            let clip = intersect_rect(&clip_rect, &self.viewport);
            overlays.push(glyph_info(motion, target_rect, source_uv, clip, mask_clip));
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

    /// Issue #919：本动画引用的全部源纹理 id（去重排序）。
    pub(crate) fn source_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> =
            self.slices.iter().map(|s| s.source_snapshot_id()).collect();
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
/// `conceal_clip` 的语义：`LeftToRight` 保留左边 `[x, x+visible]`（左边缘固定，
/// 减少 visible 时从右向左吞）；`RightToLeft` 保留右边 `[x+w-visible, x+w]`
///（右边缘固定，减少 visible 时从左向右吞）。
///
/// - Backspace：光标左移（`new_head < old_head`），光标从右向左吃掉文字 →
///   左边缘固定、逐步擦去右侧像素 → `LeftToRight`。
/// - Forward Delete / 其它：光标未左移，从左向右吃掉文字 →
///   右边缘固定、逐步擦去左侧像素 → `RightToLeft`。
fn conceal_direction(edit: &EditorEditResult) -> ClipDirection {
    let old_head = edit.old_selection.head.index.value();
    let new_head = edit.new_selection.head.index.value();
    if new_head < old_head {
        ClipDirection::LeftToRight
    } else {
        ClipDirection::RightToLeft
    }
}

/// Issue #919：该新字节范围是否落在某个 patch 的插入区间内（半开相交）。
fn inserted_in_patch(edit: &EditorEditResult, byte_range: (usize, usize)) -> bool {
    edit.display_patches.iter().any(|patch| {
        let ins_start = patch.inserted_byte_range.start().value();
        let ins_end = patch.inserted_byte_range.end().value();
        ins_start < ins_end && ins_start < byte_range.1 && byte_range.0 < ins_end
    })
}

/// Issue #919：该旧字节范围是否被某个 patch 真正替换/删除（半开相交）。
///
/// 零长度的插入 patch 不算"替换"：插入只是把后面的字挤开，后续字仍应配成 Shift。
fn replaced_in_patch(edit: &EditorEditResult, byte_range: (usize, usize)) -> bool {
    edit.display_patches.iter().any(|patch| {
        let rep_start = patch.replace_byte_range.start().value();
        let rep_end = patch.replace_byte_range.end().value();
        rep_start < rep_end && rep_start < byte_range.1 && byte_range.0 < rep_end
    })
}

/// Issue #919：把新坐标偏移映回旧坐标（基于 `display_patches` 的分段线性映射）。
///
/// 这是坐标映射，不是逐字身份匹配：对每个 patch，新坐标中 `inserted_byte_range`
/// 之前不变，之后减去 `(inserted_len - replace_len)`。落在 inserted 内部的偏移
/// 无旧身份，但未改区间不会落到那里（已由 `inserted_in_patch` 过滤）。
fn new_to_old_byte(edit: &EditorEditResult, new_offset: usize) -> usize {
    let mut offset = new_offset as i64;
    for patch in &edit.display_patches {
        let ins_start = patch.inserted_byte_range.start().value();
        let ins_end = patch.inserted_byte_range.end().value();
        let rep_start = patch.replace_byte_range.start().value();
        let rep_end = patch.replace_byte_range.end().value();
        let ins_len = ins_end as i64 - ins_start as i64;
        let rep_len = rep_end as i64 - rep_start as i64;
        let delta = ins_len - rep_len;
        if new_offset >= ins_end && delta != 0 {
            offset -= delta;
        }
    }
    offset.max(0) as usize
}

/// Issue #919：把新坐标偏移映回旧坐标，专用于**区间终点**。
///
/// 删除时 `inserted_byte_range` 为空（`ins_start == ins_end`），同一个新偏移在
/// 编辑边界处既可能是删除点左侧未改文本的末尾，也可能是右侧文本的开头。用
/// `new_to_old_byte`（`>= ins_end` 取右侧/删除后坐标）映区间起点，
/// 本函数（`> ins_start` 取左侧/删除前坐标）映区间终点，消除这个歧义：
/// 否则 Backspace 删除"乙"时，未改的"甲"末端会被错映到删除后的旧坐标，
/// 匹配不到旧 cluster 而落到 Reveal 兜底——未改的字被重新露出一次。
fn new_to_old_byte_end(edit: &EditorEditResult, new_offset: usize) -> usize {
    let mut offset = new_offset as i64;
    for patch in &edit.display_patches {
        let ins_start = patch.inserted_byte_range.start().value();
        let ins_end = patch.inserted_byte_range.end().value();
        let rep_start = patch.replace_byte_range.start().value();
        let rep_end = patch.replace_byte_range.end().value();
        let ins_len = ins_end as i64 - ins_start as i64;
        let rep_len = rep_end as i64 - rep_start as i64;
        let delta = ins_len - rep_len;
        if new_offset > ins_start && delta != 0 {
            offset -= delta;
        }
    }
    offset.max(0) as usize
}

/// Issue #919：布局片段的内部辅助结构，用于 `begin_or_retarget` 分类。
/// 不导出。
struct LayoutSpan {
    byte_range: (usize, usize),
    slice: VisualSlice,
    paragraph_id: u64,
    /// Qt cluster 的 shaping 指纹：仅用于判断编辑边界是否需要重画该视觉簇，
    /// 不作为逐字历史身份。
    shaping_identity: super::layout_snapshot::ShapingIdentity,
}

/// Issue #919：从布局快照抓取受影响范围内的局部 cluster 片段。
///
/// 范围扩到受影响段落的末尾（含同段后续折行），使插字挤到下一行、删除换行
/// 合并段落时，同段后续折行能拿到完整 old/new 几何配成 Shift。
/// 不按视口过滤——屏幕外进入/离开的字必须保留完整几何，才能在配对之后按
/// `union(from, to)` 与视口相交，判成位置动画。
fn layout_spans(layout: &EditorLayoutSnapshot, range: (usize, usize)) -> Vec<LayoutSpan> {
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

    let mut spans = Vec::new();
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
            spans.push(LayoutSpan {
                byte_range,
                slice: VisualSlice {
                    source: SnapshotRef {
                        snapshot_id: line.id,
                        source_uv: cluster.source_rect,
                    },
                    rect,
                },
                // 整体段落位移的身份用稳定 ParagraphId（与静态 QSG 同源）。
                paragraph_id: line.stable_paragraph_id,
                shaping_identity: cluster.shaping_identity.clone(),
            });
        }
    }
    spans
}

/// Issue #919 复核 2：从 display_patches 直接确定真正内容变更的稳定 ParagraphId 集合。
///
/// 不依赖 layout_spans 的扩展范围（会把同段后续折行也算进来），
/// 只看 patch 的 replace/inserted byte range 在 old/new layout 中实际覆盖的段落。
/// 零长度 replace 或 inserted 不算内容变更。
fn content_changed_paragraph_ids(
    edit: &EditorEditResult,
    old_layout: Option<&EditorLayoutSnapshot>,
    new_layout: &EditorLayoutSnapshot,
) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    for patch in &edit.display_patches {
        let rep_start = patch.replace_byte_range.start().value();
        let rep_end = patch.replace_byte_range.end().value();
        let ins_start = patch.inserted_byte_range.start().value();
        let ins_end = patch.inserted_byte_range.end().value();
        // 旧侧：replace_byte_range 非空时，在 old_layout 找所属段落。
        if rep_start < rep_end {
            if let Some(layout) = old_layout {
                for line in &layout.line_snapshots {
                    if line.byte_start <= rep_end
                        && line.byte_end >= rep_start
                        && line.stable_paragraph_id != 0
                        && !ids.contains(&line.stable_paragraph_id)
                    {
                        ids.push(line.stable_paragraph_id);
                    }
                }
            }
        }
        // 新侧：inserted_byte_range 非空时，在 new_layout 找所属段落。
        if ins_start < ins_end {
            for line in &new_layout.line_snapshots {
                if line.byte_start <= ins_end
                    && line.byte_end >= ins_start
                    && line.stable_paragraph_id != 0
                    && !ids.contains(&line.stable_paragraph_id)
                {
                    ids.push(line.stable_paragraph_id);
                }
            }
        }
    }
    ids
}

/// 复核 1：受影响段落以外、只发生整体 y 变化且在视口气带内的段落。
///
/// 只按段落 id 配对（换行插入/删除时下游段落 id 不变）；新出现的段落没有
/// 旧身份，其文字通过逐字 Shift 处理。
fn collect_paragraph_moves(
    old_layout: Option<&EditorLayoutSnapshot>,
    new_layout: &EditorLayoutSnapshot,
    viewport: &LocalRect,
    affected_ids: &[u64],
) -> Vec<ParagraphMove> {
    let Some(old_layout) = old_layout else {
        return Vec::new();
    };

    let margin = viewport.h.max(100.0);
    let y_start = viewport.y - margin;
    let y_end = viewport.y + viewport.h + margin;
    let mut candidate_ids: Vec<u64> = Vec::new();
    for layout in [old_layout, new_layout] {
        let (first, last_excl) = layout.paragraph_slots_in_y_range(y_start, y_end);
        for slot in first..last_excl {
            if let Some(paragraph_id) = layout.paragraph_id_at_slot(slot) {
                candidate_ids.push(paragraph_id);
            }
        }
    }
    candidate_ids.extend_from_slice(affected_ids);
    candidate_ids.sort_unstable();
    candidate_ids.dedup();

    let mut moves = Vec::new();
    for paragraph_id in candidate_ids {
        if paragraph_id == 0 {
            continue;
        }
        let Some((from_y, from_h)) = old_layout.paragraph_anchor(paragraph_id) else {
            continue;
        };
        let Some((to_y, to_h)) = new_layout.paragraph_anchor(paragraph_id) else {
            continue;
        };
        if (from_y - to_y).abs() < 0.5 {
            continue;
        }
        // 屏幕外进/出沿用同一 viewport 裁剪：段落整体路径与视口相交才动画。
        // 用旧/新真实段落矩形（段首 + 段落高度）的移动包络与视口求交。
        let top = from_y.min(to_y);
        let bottom = (from_y + from_h).max(to_y + to_h);
        let band = LocalRect {
            x: viewport.x,
            y: top,
            w: viewport.w,
            h: (bottom - top).max(1.0),
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

/// Issue #919：生成一个局部文字覆盖层（简化版，不含逐字身份字段）。
fn glyph_info(
    motion: &SliceMotion,
    target_rect: LocalRect,
    source_uv: LocalRect,
    clip_rect: LocalRect,
    mask_clip: LocalRect,
) -> TextAnimationGlyphInfo {
    TextAnimationGlyphInfo {
        x: target_rect.x,
        y: target_rect.y,
        w: target_rect.w,
        h: target_rect.h,
        opacity: 1.0,
        snapshot_id: motion.source_snapshot_id(),
        source_rect: source_uv,
        clip_rect,
        mask_clip,
        motion_kind: motion.motion_kind(),
        byte_range: motion.byte_range(),
        shaping_identity: motion.shaping_identity().clone(),
        direction: motion.direction(),
    }
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

/// Issue #919 复核 5：Conceal 的裁剪矩形从 `start_clip`（上一帧原始 mask clip）
/// 缩到 0 宽度，位置由 `start_clip` 决定，不从源矩形边界重新造遮罩。
///
/// - `LeftToRight`：左边固定在 `start_clip.x`，右边向左缩到 0。
/// - `RightToLeft`：右边固定在 `start_clip.x + start_clip.w`，左边向右缩到 0。
fn conceal_clip(start_clip: &LocalRect, direction: ClipDirection, visible: f64) -> LocalRect {
    let visible = visible.clamp(0.0, start_clip.w);
    match direction {
        ClipDirection::LeftToRight => LocalRect {
            x: start_clip.x,
            y: start_clip.y,
            w: visible,
            h: start_clip.h,
        },
        ClipDirection::RightToLeft => LocalRect {
            x: start_clip.x + (start_clip.w - visible),
            y: start_clip.y,
            w: visible,
            h: start_clip.h,
        },
    }
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

#[allow(dead_code)]
fn lerp_rect(from: &LocalRect, to: &LocalRect, t: f64) -> LocalRect {
    LocalRect {
        x: lerp(from.x, to.x, t),
        y: lerp(from.y, to.y, t),
        w: lerp(from.w, to.w, t),
        h: lerp(from.h, to.h, t),
    }
}

fn ease_out_cubic(t: f64) -> f64 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

/// 两个 byte 区间是否相交（闭区间语义，保留供区间判断）。
#[allow(dead_code)]
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

/// Issue #919 复核 16：在 `last_drawn_map` 中查找包含 `old_byte_range` 的合并 Shift。
///
/// `merge_adjacent_shifts()` 把多条 Shift 合为一条（如"乙"(9..12)+"丙"(12..15)
/// → byte_range=(9,15)），`sample()` 只产出合并后的单条 `DrawnSlice`。下一笔编辑
/// 逐 cluster 枚举 `new_spans`，"乙"的 `old_byte_range=(9,12)` 精确查
/// `last_drawn_map` 找不到合并键 `(9,15)`。此函数遍历查找**包含** `old_byte_range`
/// 的合并 Shift `DrawnSlice`，供 `begin_or_retarget` 还原当前 cluster 的接续位置。
///
/// Issue #919 复核 17：不能只凭 byte-range 包含就认成同一合并组。同一 session 内
/// 连续快打时，已提交视觉帧的 snapshot_id 可能与当前 canonical revision 不同，碰巧
/// 相同的 byte range 会匹配错误来源，`restore_cluster_rect` 会把另一张图的 UV 当作
/// 本片的 UV。因此额外核对 `snapshot_id` 一致，且 `old_source.source_uv` 落在合并
/// `source_uv` 内（UV 包含），才允许接续。
fn find_merged_shift<'a>(
    last_drawn_map: &std::collections::HashMap<(usize, usize), Vec<&'a DrawnSlice>>,
    old_byte_range: (usize, usize),
    old_source: &SnapshotRef,
) -> Option<&'a DrawnSlice> {
    last_drawn_map
        .values()
        .flatten()
        .find(|ds| {
            ds.motion_kind == MotionKind::Shift
                && ds.byte_range.0 <= old_byte_range.0
                && old_byte_range.1 <= ds.byte_range.1
                && ds.byte_range != old_byte_range
                && ds.slice.source.snapshot_id == old_source.snapshot_id
                && uv_contains(&ds.slice.source.source_uv, &old_source.source_uv)
        })
        .copied()
}

/// Issue #919 复核 17：判断 `inner` 的 UV 矩形是否被 `outer` 包含（含边界）。
///
/// 用于 `find_merged_shift` 确认当前 cluster 的 `source_uv` 确实落在合并 Shift 的
/// 合并 `source_uv` 内，避免跨 revision / 跨行图碰巧同 byte range 错误匹配。
fn uv_contains(outer: &LocalRect, inner: &LocalRect) -> bool {
    const EPS: f64 = 0.01;
    inner.x >= outer.x - EPS
        && inner.y >= outer.y - EPS
        && inner.x + inner.w <= outer.x + outer.w + EPS
        && inner.y + inner.h <= outer.y + outer.h + EPS
}

/// Issue #919 复核 16：从合并 Shift 的 `mask_clip` 中还原当前 cluster 的当前显示位置。
///
/// 合并 Shift 的 `mask_clip` 是合并后的插值 rect（包含"乙丙"），`merged_source_uv`
/// 是合并后的源纹理 UV。当前 cluster 在合并片段中的相对位置由 `cluster_source_uv`
/// 相对于 `merged_source_uv` 确定。同一行内 UV→文档坐标变换是线性的
/// （`doc = uv / dpr + offset`），所以 UV 中的相对比例 = 文档坐标中的相对比例。
/// 还原后的位置 = 合并插值位置按 UV 比例切出当前 cluster 的部分，宽高用当前
/// cluster 的固定尺寸（不缩放字形）。
fn restore_cluster_rect(
    merged_mask_clip: &LocalRect,
    merged_source_uv: &LocalRect,
    cluster_source_uv: &LocalRect,
    cluster_new_rect: &LocalRect,
) -> LocalRect {
    let rel_x = if merged_source_uv.w > 0.01 {
        (cluster_source_uv.x - merged_source_uv.x) / merged_source_uv.w
    } else {
        0.0
    };
    let rel_y = if merged_source_uv.h > 0.01 {
        (cluster_source_uv.y - merged_source_uv.y) / merged_source_uv.h
    } else {
        0.0
    };
    LocalRect {
        x: merged_mask_clip.x + rel_x * merged_mask_clip.w,
        y: merged_mask_clip.y + rel_y * merged_mask_clip.h,
        w: cluster_new_rect.w,
        h: cluster_new_rect.h,
    }
}

/// Issue #919 复核 16：把相邻可合并的 Shift 合并成连续视觉片段。
///
/// 合并条件：同一新行纹理、旧/新字节区间连续、同一旧/新视觉行、位移向量相同、
/// 新侧源 UV 在行内连续。遇到换行、源纹理 ID 变化、位移不一致或 UV 不连续就
/// 结束当前组。合并时 from/to/new.rect/new.source_uv 取整段并集，byte_range
/// 合并为连续区间；shaping_identity 沿用组内首字——合并段在下一笔编辑接续时
/// 由 `find_merged_shift` + `restore_cluster_rect` 还原各 cluster 的当前可见
/// rect，原始 cluster 的 shaping 判断仍读取旧/新布局各自的 `LayoutSpan.shaping_identity`，
/// 不复用合并组首字指纹作为所有成员的身份。
fn merge_adjacent_shifts(slices: Vec<SliceMotion>) -> Vec<SliceMotion> {
    let mut result: Vec<SliceMotion> = Vec::with_capacity(slices.len());
    for s in slices {
        let prev_is_shift = matches!(result.last(), Some(SliceMotion::Shift { .. }));
        let cur_is_shift = matches!(s, SliceMotion::Shift { .. });
        if prev_is_shift && cur_is_shift {
            let prev = result.pop().unwrap();
            if let (
                SliceMotion::Shift {
                    new: a_new,
                    from: a_from,
                    to: a_to,
                    byte_range: a_br,
                    shaping_identity: a_shaping,
                },
                SliceMotion::Shift {
                    new: b_new,
                    from: b_from,
                    to: b_to,
                    byte_range: b_br,
                    ..
                },
            ) = (&prev, &s)
            {
                const EPS: f64 = 0.05;
                let same_snapshot = a_new.source.snapshot_id == b_new.source.snapshot_id;
                let byte_continuous = a_br.1 == b_br.0;
                let same_old_line = (a_from.y - b_from.y).abs() < EPS;
                let same_new_line = (a_to.y - b_to.y).abs() < EPS;
                let same_disp_x = ((b_to.x - b_from.x) - (a_to.x - a_from.x)).abs() < EPS;
                let same_disp_y = ((b_to.y - b_from.y) - (a_to.y - a_from.y)).abs() < EPS;
                let uv_continuous = ((a_new.source.source_uv.x + a_new.source.source_uv.w)
                    - b_new.source.source_uv.x)
                    .abs()
                    < EPS
                    && (a_new.source.source_uv.y - b_new.source.source_uv.y).abs() < EPS;
                if same_snapshot
                    && byte_continuous
                    && same_old_line
                    && same_new_line
                    && same_disp_x
                    && same_disp_y
                    && uv_continuous
                {
                    result.push(SliceMotion::Shift {
                        new: VisualSlice {
                            source: SnapshotRef {
                                snapshot_id: a_new.source.snapshot_id,
                                source_uv: union_rect(
                                    &a_new.source.source_uv,
                                    &b_new.source.source_uv,
                                ),
                            },
                            rect: union_rect(&a_new.rect, &b_new.rect),
                        },
                        from: union_rect(a_from, b_from),
                        to: union_rect(a_to, b_to),
                        byte_range: (a_br.0, b_br.1),
                        shaping_identity: a_shaping.clone(),
                    });
                    continue;
                }
            }
            result.push(prev);
            result.push(s);
        } else {
            result.push(s);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    //! Issue #919：把局部文字动画的核心不变量固化成回归测试。
    //!
    //! 这些测试只验证 `mask_rect` / `conceal_clip` / `SliceMotion::sample` 的
    //! 几何语义，不构造 Qt 运行时。它们对应历次复核修正的实际显示错误：
    //! 连续编辑从当前可见画面接续、遮罩按原始 clip 边界收拢、Shift 只位移不缩放。
    //!
    //! 复核 10 追加：直接驱动 `TextMotion::begin_or_retarget`，用构造出来的
    //! `EditorEditResult` + 旧/新 `EditorLayoutSnapshot` 验证「按 patch 分类
    //! Reveal / Conceal / Shift」「连续编辑从 last_drawn 接续」「同 byte_range
    //! 不同 shaping 必须旧退场 + 新露出」，不构造 Qt 运行时。

    use super::super::layout_snapshot::{
        LayoutRevision, LineClusterSnapshot, PreparedLineSnapshot,
    };
    use crate::editor::layout::chunked_vec::ChunkedSlotMap;
    use crate::editor::layout::{CaretAffinity, ParagraphHeightIndex, ParagraphId};
    use writer_core::editor::{
        DisplayPatch, EditorContentDelta, EditorEditResult, EditorOperationKind, EditorRevision,
        EditorSelection, EditorTransactionCause, Utf8ByteRange,
    };

    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> LocalRect {
        LocalRect { x, y, w, h }
    }

    fn snap_ref(paragraph_id: u64, uv: LocalRect) -> SnapshotRef {
        SnapshotRef {
            snapshot_id: LineSnapshotId::new(1, paragraph_id, 0),
            source_uv: uv,
        }
    }

    fn identity() -> ShapingIdentity {
        ShapingIdentity::default()
    }

    fn make_motion(slices: Vec<SliceMotion>) -> TextMotion {
        TextMotion {
            slices,
            started_ms: 0.0,
            duration_ms: 100,
            session: 1,
            target_revision: 2,
            viewport: rect(0.0, 0.0, 1000.0, 1000.0),
            static_exclusions: Vec::new(),
            required_textures: Vec::new(),
            caret_from: None,
            caret_to: None,
            paragraph_moves: Vec::new(),
        }
    }

    fn visual(paragraph_id: u64, uv: LocalRect, rect: LocalRect) -> VisualSlice {
        VisualSlice {
            source: snap_ref(paragraph_id, uv),
            rect,
        }
    }

    #[test]
    fn mask_rect_uses_direction_edge() {
        let at = rect(0.0, 5.0, 20.0, 10.0);
        assert_eq!(
            mask_rect(&at, ClipDirection::LeftToRight, 8.0),
            rect(0.0, 5.0, 8.0, 10.0)
        );
        assert_eq!(
            mask_rect(&at, ClipDirection::RightToLeft, 8.0),
            rect(12.0, 5.0, 8.0, 10.0)
        );
        assert_eq!(
            mask_rect(&at, ClipDirection::LeftToRight, 999.0),
            rect(0.0, 5.0, 20.0, 10.0)
        );
        assert_eq!(
            mask_rect(&at, ClipDirection::LeftToRight, -3.0),
            rect(0.0, 5.0, 0.0, 10.0)
        );
    }

    #[test]
    fn conceal_clip_converges_from_start_clip_boundary() {
        let start = rect(10.0, 2.0, 8.0, 10.0);
        assert_eq!(conceal_clip(&start, ClipDirection::LeftToRight, 8.0), start);
        assert_eq!(
            conceal_clip(&start, ClipDirection::LeftToRight, 3.0),
            rect(10.0, 2.0, 3.0, 10.0)
        );
        assert_eq!(
            conceal_clip(&start, ClipDirection::LeftToRight, 0.0),
            rect(10.0, 2.0, 0.0, 10.0)
        );
        // RightToLeft 固定右边界 (x+w=18)，向左收拢。
        assert_eq!(
            conceal_clip(&start, ClipDirection::RightToLeft, 3.0),
            rect(15.0, 2.0, 3.0, 10.0)
        );
    }

    /// 旧字 target 与动画遮罩必须有实际非零交集，否则 `scene_graph_renderer`
    /// 在 `iw <= 0` 时 `continue`，屏幕上没有任何退场像素，测试会把空画面
    /// 误当正确动画。这里构造互相有覆盖、但遮罩左边缘不同于完整旧字左边缘的
    /// 几何：旧图 target=40..60，上一帧原始 start_clip=50..58，交集宽 8。
    fn conceal_with_start_clip(direction: ClipDirection) -> TextMotion {
        let old = visual(3, rect(0.0, 0.0, 20.0, 10.0), rect(40.0, 0.0, 20.0, 10.0));
        let start_clip = rect(50.0, 0.0, 8.0, 10.0);
        make_motion(vec![SliceMotion::Conceal {
            old,
            direction,
            start_clip,
            byte_range: (0, 3),
            shaping_identity: identity(),
        }])
    }

    /// target 与 mask 的水平交集宽度（>0 表示屏幕上确有退场像素）。
    fn intersect_width(target_x: f64, target_w: f64, clip: &LocalRect) -> f64 {
        let x0 = clip.x.max(target_x);
        let x1 = (clip.x + clip.w).min(target_x + target_w);
        (x1 - x0).max(0.0)
    }

    #[test]
    fn conceal_sample_continues_from_start_clip_not_old_rect() {
        // Backspace → LeftToRight：左边缘固定，从右向左吞。
        let motion = conceal_with_start_clip(ClipDirection::LeftToRight);

        // progress=0：遮罩完整继承 start_clip，与 target 有非零交集。
        let first = motion.sample(0.0);
        let overlay = first.overlays.first().expect("conceal overlay");
        assert_eq!((overlay.x, overlay.w), (40.0, 20.0)); // target=40..60
        assert_eq!((overlay.clip_rect.x, overlay.clip_rect.w), (50.0, 8.0)); // mask=50..58
        assert_eq!(intersect_width(overlay.x, overlay.w, &overlay.clip_rect), 8.0);

        // progress=0.5：左边缘固定 50，宽度缩小，仍与 target 有实际非零交集。
        let mid = motion.sample(50.0);
        let overlay = mid.overlays.first().expect("conceal overlay");
        assert_eq!(overlay.clip_rect.x, 50.0);
        assert!(overlay.clip_rect.w > 0.0 && overlay.clip_rect.w < 8.0);
        assert!(intersect_width(overlay.x, overlay.w, &overlay.clip_rect) > 0.0);
    }

    #[test]
    fn conceal_sample_right_to_left_continues_from_start_clip() {
        // Forward Delete → RightToLeft：右边缘固定，从左向右吞。
        let motion = conceal_with_start_clip(ClipDirection::RightToLeft);

        // progress=0：遮罩完整继承 start_clip，与 target 有非零交集。
        let first = motion.sample(0.0);
        let overlay = first.overlays.first().expect("conceal overlay");
        assert_eq!((overlay.x, overlay.w), (40.0, 20.0)); // target=40..60
        assert_eq!((overlay.clip_rect.x, overlay.clip_rect.w), (50.0, 8.0)); // mask=50..58
        assert_eq!(intersect_width(overlay.x, overlay.w, &overlay.clip_rect), 8.0);

        // progress=0.5：右边缘固定 58，宽度缩小，仍与 target 有实际非零交集。
        let mid = motion.sample(50.0);
        let overlay = mid.overlays.first().expect("conceal overlay");
        assert_eq!(overlay.clip_rect.x + overlay.clip_rect.w, 58.0);
        assert!(overlay.clip_rect.w > 0.0 && overlay.clip_rect.w < 8.0);
        assert!(intersect_width(overlay.x, overlay.w, &overlay.clip_rect) > 0.0);
    }

    #[test]
    fn conceal_sample_narrows_strictly_within_start_clip() {
        // Issue #919：Conceal 必须从上一帧可见 clip 持续收缩，且永不越过已显示边界。
        // 两种方向都保留各自固定的一侧边缘，避免下笔把可见部分瞬移到旧矩形另一边。
        let start = rect(50.0, 0.0, 8.0, 10.0);
        for direction in [ClipDirection::LeftToRight, ClipDirection::RightToLeft] {
            let motion = conceal_with_start_clip(direction);
            let mut prev_width = start.w;
            for step in 1..8 {
                let frame = motion.sample(step as f64 * 10.0);
                let overlay = frame.overlays.first().expect("conceal overlay");
                assert!(
                    overlay.clip_rect.w < prev_width,
                    "遮罩必须严格收缩: {} < {}",
                    overlay.clip_rect.w,
                    prev_width
                );
                assert!(overlay.clip_rect.w >= 0.0);
                prev_width = overlay.clip_rect.w;
                match direction {
                    ClipDirection::LeftToRight => {
                        assert_eq!(overlay.clip_rect.x, start.x);
                        assert!(overlay.clip_rect.x + overlay.clip_rect.w <= start.x + start.w);
                    }
                    ClipDirection::RightToLeft => {
                        assert_eq!(overlay.clip_rect.x + overlay.clip_rect.w, start.x + start.w);
                        assert!(overlay.clip_rect.x >= start.x);
                    }
                }
            }
        }
    }

    #[test]
    fn reveal_sample_moves_from_start_clip_and_fixes_size() {
        let new = visual(9, rect(0.0, 0.0, 20.0, 10.0), rect(40.0, 0.0, 20.0, 10.0));
        let start_clip = rect(0.0, 0.0, 5.0, 10.0);
        let motion = make_motion(vec![SliceMotion::Reveal {
            new,
            direction: ClipDirection::LeftToRight,
            start_clip,
            byte_range: (0, 3),
            shaping_identity: identity(),
        }]);

        let first = motion.sample(0.0);
        let overlay = first.overlays.first().expect("reveal overlay");
        // 从上一帧已提交位置起步，不从 new.rect 直接开始。
        assert_eq!(overlay.x, 0.0);
        assert_eq!((overlay.w, overlay.h), (20.0, 10.0));
        assert_eq!(overlay.clip_rect.w, 5.0);

        let mid = motion.sample(50.0);
        let overlay = mid.overlays.first().expect("reveal overlay");
        assert!(overlay.x > 0.0 && overlay.x < 40.0);
        assert_eq!((overlay.w, overlay.h), (20.0, 10.0));
        assert!(overlay.clip_rect.w > 5.0 && overlay.clip_rect.w <= 20.0);
    }

    #[test]
    fn shift_sample_keeps_new_size_and_only_moves() {
        let from = rect(100.0, 0.0, 5.0, 12.0);
        let to = rect(140.0, 3.0, 30.0, 20.0);
        let new = visual(7, rect(0.0, 0.0, 30.0, 20.0), to);
        let motion = make_motion(vec![SliceMotion::Shift {
            new,
            from,
            to,
            byte_range: (0, 3),
            shaping_identity: identity(),
        }]);

        let first = motion.sample(0.0);
        let overlay = first.overlays.first().expect("shift overlay");
        assert_eq!((overlay.x, overlay.y), (from.x, from.y));
        // 宽高固定为 new 版目标尺寸，绝不用 from/to 插值缩放字形。
        assert_eq!((overlay.w, overlay.h), (to.w, to.h));

        let mid = motion.sample(50.0);
        let overlay = mid.overlays.first().expect("shift overlay");
        assert_eq!((overlay.w, overlay.h), (to.w, to.h));
        assert!(overlay.x > from.x && overlay.x < to.x);
        assert!(overlay.y > from.y && overlay.y < to.y);
    }

    #[test]
    fn paragraph_move_offset_is_zero_at_end() {
        let pm = ParagraphMove {
            paragraph_id: 3,
            from_y: 20.0,
            to_y: 0.0,
        };
        assert!((pm.offset_at(0.0) - 20.0).abs() < 1e-9);
        assert!(pm.offset_at(1.0).abs() < 1e-9);
    }

    #[test]
    fn source_snapshot_ids_are_deduplicated() {
        let shared = LineSnapshotId::new(1, 1, 0);
        let slice = |uv: LocalRect| {
            VisualSlice {
                source: SnapshotRef {
                    snapshot_id: shared,
                    source_uv: uv,
                },
                rect: rect(0.0, 0.0, 10.0, 10.0),
            }
        };
        let motion = make_motion(vec![
            SliceMotion::Reveal {
                new: slice(rect(0.0, 0.0, 10.0, 10.0)),
                direction: ClipDirection::LeftToRight,
                start_clip: rect(0.0, 0.0, 0.0, 10.0),
                byte_range: (0, 1),
                shaping_identity: identity(),
            },
            SliceMotion::Conceal {
                old: slice(rect(10.0, 0.0, 10.0, 10.0)),
                direction: ClipDirection::LeftToRight,
                start_clip: rect(0.0, 0.0, 10.0, 10.0),
                byte_range: (1, 2),
                shaping_identity: identity(),
            },
        ]);
        assert_eq!(motion.source_snapshot_ids(), vec![shared]);
    }

    #[test]
    fn motion_keeps_old_and_new_sources_distinct() {
        // Issue #919：同一 byte_range 的旧 Conceal 与新 Reveal 必须各自持有旧/新快照，
        // 不能被字节位置合并成一个源；这也是移除逐字身份匹配后仍不允许重影的前提。
        let old = visual(1, rect(0.0, 0.0, 10.0, 10.0), rect(40.0, 0.0, 10.0, 10.0));
        let new = visual(2, rect(0.0, 0.0, 10.0, 10.0), rect(40.0, 0.0, 10.0, 10.0));
        let motion = make_motion(vec![
            SliceMotion::Conceal {
                old,
                direction: ClipDirection::LeftToRight,
                start_clip: rect(40.0, 0.0, 10.0, 10.0),
                byte_range: (0, 1),
                shaping_identity: identity(),
            },
            SliceMotion::Reveal {
                new,
                direction: ClipDirection::LeftToRight,
                start_clip: rect(40.0, 0.0, 0.0, 10.0),
                byte_range: (0, 1),
                shaping_identity: identity(),
            },
        ]);
        let ids = motion.source_snapshot_ids();
        assert_eq!(ids.len(), 2, "旧/新画面必须各自持有快照");
        assert_ne!(ids[0], ids[1]);
    }

    // ── 复核 10：begin_or_retarget 端到端分类回归 ────────────────────────

    fn byte_range(start: usize, end: usize) -> Utf8ByteRange {
        match Utf8ByteRange::from_values(start, end) {
            Some(range) => range,
            None => panic!("invalid byte range {start}..{end}"),
        }
    }

    fn cluster(byte_start: usize, byte_end: usize, x: f64, w: f64) -> LineClusterSnapshot {
        LineClusterSnapshot {
            byte_start,
            byte_end,
            // 行局部物理像素；dpr=1、visual_x=0 时文档 x 就等于此 x。
            source_rect: rect(x, 0.0, w, 10.0),
            shaping_identity: ShapingIdentity::default(),
        }
    }

    /// 造一条只含指定 cluster 的测试视觉行：段首文档 y = `top`，`visual_x = 0`。
    fn test_line(
        ordinal: u32,
        byte_start: usize,
        clusters: Vec<LineClusterSnapshot>,
        top: f64,
    ) -> PreparedLineSnapshot {
        let mut line =
            PreparedLineSnapshot::stub_for_tests(ordinal as usize, top, byte_start, clusters);
        line.visual_x = 0.0;
        line
    }

    fn snapshot(lines: Vec<PreparedLineSnapshot>) -> EditorLayoutSnapshot {
        // Issue #919 复核 11：把 revision 写回每条 line.id，使 LineSnapshotId
        // 携带真实布局版本而非 stub_for_tests 的固定 (0,0,ordinal)。
        // ordinal=0 时旧值恰好等于 LineSnapshotId::none()（无纹理哨兵），
        // old/new 两版会被误认成同一 SnapshotRef；此处用 revision.0 作为
        // layout_revision、stable_paragraph_id 作为 paragraph_id、原 ordinal
        // 作为 visual_line_ordinal，保证 id.is_none()==false 且 old/new 不同。
        let revision = LayoutRevision::next();
        let line_snapshots = lines
            .into_iter()
            .map(|mut line| {
                line.id = LineSnapshotId::new(
                    revision.0,
                    line.stable_paragraph_id,
                    line.id.visual_line_ordinal,
                );
                line
            })
            .collect();
        EditorLayoutSnapshot {
            revision,
            line_snapshots,
            paragraph_height_index: None,
            paragraph_slot_map: None,
            paragraph_padding: 0.0,
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: CaretAffinity::Upstream,
        }
    }

    /// 造一个带真实段落几何索引的测试快照。
    ///
    /// `snapshot()` 只填 `line_snapshots`，不含 `paragraph_height_index` /
    /// `paragraph_slot_map`；整体段落位移 `collect_paragraph_moves` 依赖这两项
    /// 按稳定 ParagraphId 查 `(段首 y, 段高)`。`heights[i]`/`ids[i]` 对应 slot i，
    /// 段首 y = 前 i 段高度之和（`paragraph_padding = 0`）。
    fn snapshot_with_paragraph_index(
        lines: Vec<PreparedLineSnapshot>,
        heights: Vec<f64>,
        ids: Vec<u64>,
    ) -> EditorLayoutSnapshot {
        let mut snapshot = snapshot(lines);
        snapshot.paragraph_height_index = Some(std::sync::Arc::new(
            ParagraphHeightIndex::from_heights(heights),
        ));
        snapshot.paragraph_slot_map = Some(std::sync::Arc::new(ChunkedSlotMap::from_ids(
            ids.into_iter().map(ParagraphId::from_u64).collect(),
        )));
        snapshot
    }

    #[allow(clippy::too_many_arguments)]
    fn make_edit(
        old_text: &str,
        new_text: &str,
        old_head: usize,
        new_head: usize,
        replace: (usize, usize),
        inserted: (usize, usize),
        inserted_text: &str,
    ) -> EditorEditResult {
        let base = EditorRevision::initial();
        let new_rev = base.next();
        EditorEditResult {
            transaction_id: 1,
            base_revision: base,
            new_revision: new_rev,
            display_patches: vec![DisplayPatch {
                base_revision: base,
                new_revision: new_rev,
                replace_byte_range: byte_range(replace.0, replace.1),
                inserted_byte_range: byte_range(inserted.0, inserted.1),
                inserted_text: inserted_text.to_string(),
                resulting_selection_byte_range: byte_range(inserted.1, inserted.1),
            }],
            old_selection: EditorSelection::collapsed(old_text, old_head),
            new_selection: EditorSelection::collapsed(new_text, new_head),
            cause: EditorTransactionCause::Typing,
            operation_kind: EditorOperationKind::Insert,
            offset_map: None,
            content_delta: EditorContentDelta::default(),
        }
    }

    /// 造一个含多笔 `display_patches` 的事务。
    ///
    /// 每笔为 `((旧起, 旧止), (新起, 新止), 新文本)`；`replace` 是旧坐标、
    /// `inserted` 是新（最终）坐标，与 Core `DisplayPatch` 语义一致。
    fn make_edit_from_patches(
        old_text: &str,
        new_text: &str,
        old_head: usize,
        new_head: usize,
        patches: Vec<((usize, usize), (usize, usize), &str)>,
    ) -> EditorEditResult {
        let base = EditorRevision::initial();
        let new_rev = base.next();
        let display_patches = patches
            .into_iter()
            .map(
                |((rep_start, rep_end), (ins_start, ins_end), text)| DisplayPatch {
                    base_revision: base,
                    new_revision: new_rev,
                    replace_byte_range: byte_range(rep_start, rep_end),
                    inserted_byte_range: byte_range(ins_start, ins_end),
                    inserted_text: text.to_string(),
                    resulting_selection_byte_range: byte_range(ins_end, ins_end),
                },
            )
            .collect();
        EditorEditResult {
            transaction_id: 1,
            base_revision: base,
            new_revision: new_rev,
            display_patches,
            old_selection: EditorSelection::collapsed(old_text, old_head),
            new_selection: EditorSelection::collapsed(new_text, new_head),
            cause: EditorTransactionCause::Typing,
            operation_kind: EditorOperationKind::Insert,
            offset_map: None,
            content_delta: EditorContentDelta::default(),
        }
    }

    fn viewport() -> LocalRect {
        rect(0.0, 0.0, 1000.0, 1000.0)
    }

    fn retarget(
        edit: &EditorEditResult,
        old_layout: Option<&EditorLayoutSnapshot>,
        new_layout: &EditorLayoutSnapshot,
        last_drawn: Option<&LocalDrawnFrame>,
        session: u64,
    ) -> TextMotion {
        match TextMotion::begin_or_retarget(
            edit,
            old_layout,
            new_layout,
            last_drawn,
            0.0,
            100,
            session,
            viewport(),
        ) {
            Some(motion) => motion,
            None => panic!("expected TextMotion, got None"),
        }
    }

    #[test]
    fn cursor_only_edit_without_content_returns_none() {
        // 空 patch（无插入/替换）：只有光标移动，不做局部动画。
        let new_layout = snapshot(vec![]);
        let edit = make_edit("", "", 0, 0, (0, 0), (0, 0), "");
        assert!(TextMotion::begin_or_retarget(
            &edit,
            None,
            &new_layout,
            None,
            0.0,
            100,
            1,
            viewport(),
        )
        .is_none());
    }

    #[test]
    fn inserted_patch_classifies_as_reveal_from_zero_width() {
        let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
        let motion = retarget(&edit, None, &new_layout, None, 7);

        assert_eq!(motion.slices.len(), 1);
        match &motion.slices[0] {
            SliceMotion::Reveal {
                new,
                direction,
                start_clip,
                ..
            } => {
                assert_eq!(new.rect, rect(0.0, 0.0, 20.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
                // 无上一帧接续：全新字从零宽露出，位置就是自身 target。
                assert_eq!(*start_clip, rect(0.0, 0.0, 0.0, 10.0));
            }
            other => panic!("expected Reveal, got {other:?}"),
        }
        assert_eq!(
            motion.source_snapshot_ids(),
            vec![new_layout.line_snapshots[0].id]
        );
    }

    #[test]
    fn backspace_conceals_with_left_edge_fixed() {
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![]);
        // Backspace：光标从 3 左移到 0 → 左边缘固定、从右往左吞。
        let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        assert_eq!(motion.slices.len(), 1);
        match &motion.slices[0] {
            SliceMotion::Conceal {
                old,
                direction,
                start_clip,
                ..
            } => {
                assert_eq!(old.rect, rect(0.0, 0.0, 20.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
                // 无在途旧图：从旧字完整画面开始吞。
                assert_eq!(*start_clip, rect(0.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Conceal, got {other:?}"),
        }
    }

    #[test]
    fn forward_delete_conceals_with_right_edge_fixed() {
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![]);
        // Forward Delete：光标停在 0（未左移）→ 右边缘固定、从左往右吞。
        let edit = make_edit("甲", "", 0, 0, (0, 3), (0, 0), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        match &motion.slices[0] {
            SliceMotion::Conceal { direction, .. } => {
                assert_eq!(*direction, ClipDirection::RightToLeft);
            }
            other => panic!("expected Conceal, got {other:?}"),
        }
    }

    #[test]
    fn insert_before_existing_text_shifts_unchanged_cluster() {
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        )]);
        // 在段首插入"乙"：新字 Reveal，"甲"未改但被挤到 x=20 → Shift。
        let edit = make_edit("甲", "乙甲", 0, 3, (0, 0), (0, 3), "乙");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        assert_eq!(motion.slices.len(), 2);
        assert!(matches!(motion.slices[0], SliceMotion::Reveal { .. }));
        match &motion.slices[1] {
            SliceMotion::Shift { from, to, .. } => {
                assert_eq!(*from, rect(0.0, 0.0, 20.0, 10.0));
                assert_eq!(*to, rect(20.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Shift for unchanged 甲, got {other:?}"),
        }
    }

    fn drawn_reveal(
        line_id: LineSnapshotId,
        mask_clip: LocalRect,
        shaping_identity: ShapingIdentity,
    ) -> LocalDrawnFrame {
        LocalDrawnFrame {
            session: 7,
            revision: 1,
            slices: vec![DrawnSlice {
                slice: VisualSlice {
                    source: SnapshotRef {
                        snapshot_id: line_id,
                        source_uv: rect(0.0, 0.0, 20.0, 10.0),
                    },
                    rect: rect(0.0, 0.0, 20.0, 10.0),
                },
                visible_width: mask_clip.w,
                clip_rect: mask_clip,
                mask_clip,
                motion_kind: MotionKind::Reveal,
                byte_range: (0, 3),
                shaping_identity,
                direction: ClipDirection::LeftToRight,
            }],
        }
    }

    #[test]
    fn continuous_reveal_inherits_previous_mask_clip() {
        let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let line_id = new_layout.line_snapshots[0].id;
        // 上一帧同一字形吐到一半（遮罩 0..10）。
        let last = drawn_reveal(
            line_id,
            rect(0.0, 0.0, 10.0, 10.0),
            ShapingIdentity::default(),
        );
        let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
        let motion = retarget(&edit, None, &new_layout, Some(&last), 7);

        assert_eq!(motion.slices.len(), 1);
        match &motion.slices[0] {
            SliceMotion::Reveal { start_clip, .. } => {
                // 从上一帧原始 mask clip 继续吐，不退回零宽。
                assert_eq!(*start_clip, rect(0.0, 0.0, 10.0, 10.0));
            }
            other => panic!("expected Reveal, got {other:?}"),
        }
    }

    #[test]
    fn consecutive_typing_continues_unchanged_cluster_reveal() {
        // Issue #919：连续输入——上一帧“甲”正吐到一半（遮罩 0..10），本帧在其后输入
        // “乙”。“甲”在本次编辑里是 patch 外未改区间，必须从上一帧已显示的
        // `mask_clip` 继续吐到满宽，而不是因为不在本次插入区间就退回零宽/重新露出。
        // 这覆盖 `begin_or_retarget` 中「未改区间接续上一帧 Reveal」的分支：用偏移
        // 映射定位旧 byte range，再从 last_drawn 的同一 shaping 画面接续。
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        )]);
        let old_id = old_layout.line_snapshots[0].id;
        let new_id = new_layout.line_snapshots[0].id;
        // 上一帧“甲”Reveal 只露出左半 (0..10)，源是上一版排版行图。
        let last = drawn_reveal(
            old_id,
            rect(0.0, 0.0, 10.0, 10.0),
            ShapingIdentity::default(),
        );

        // 在“甲”之后输入“乙”：插入 patch 为 3..6，“甲”0..3 属未改区间。
        let edit = make_edit("甲", "甲乙", 3, 6, (3, 3), (3, 6), "乙");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

        // “甲”未改但继续 Reveal（从半宽接续），“乙”新插入 Reveal（从零宽）。
        let reveal_jia = motion
            .slices
            .iter()
            .find(|s| s.byte_range() == (0, 3) && s.motion_kind() == MotionKind::Reveal)
            .expect("“甲”应继续 Reveal");
        match reveal_jia {
            SliceMotion::Reveal {
                new, start_clip, ..
            } => {
                assert_eq!(new.source.snapshot_id, new_id);
                assert_eq!(
                    *start_clip,
                    rect(0.0, 0.0, 10.0, 10.0),
                    "必须从上一帧半宽继续吐，不退回零宽"
                );
            }
            other => panic!("expected Reveal for 甲, got {other:?}"),
        }
        let reveal_yi = motion
            .slices
            .iter()
            .find(|s| s.byte_range() == (3, 6) && s.motion_kind() == MotionKind::Reveal)
            .expect("“乙”应 Reveal");
        match reveal_yi {
            SliceMotion::Reveal {
                new, start_clip, ..
            } => {
                assert_eq!(new.source.snapshot_id, new_id);
                assert_eq!(*start_clip, rect(20.0, 0.0, 0.0, 10.0));
            }
            other => panic!("expected Reveal for 乙, got {other:?}"),
        }
        assert_eq!(motion.slices.len(), 2);
    }

    #[test]
    fn same_byte_range_different_shaping_retires_old_and_reveals_new() {
        // Issue #919 复核 11：独立构造 old/new 排版画面，两份不同的行纹理 ID。
        // 上一帧旧字纹理来源（old_id）与本轮新字（new_id）各自独立，模拟真实
        // 旧/新可渲染视觉资源，而非共用同一份新行纹理。
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let old_id = old_layout.line_snapshots[0].id;
        let new_id = new_layout.line_snapshots[0].id;
        assert_ne!(old_id, new_id);
        // 上一帧露的是别的字形（glyph hash 不同），用旧排版画面的纹理 ID。
        let old_shaping = ShapingIdentity {
            glyph_indexes_hash: 42,
            ..ShapingIdentity::default()
        };
        let last = drawn_reveal(old_id, rect(0.0, 0.0, 10.0, 10.0), old_shaping);
        let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
        let motion = retarget(&edit, None, &new_layout, Some(&last), 7);

        assert_eq!(motion.slices.len(), 2);
        let source_ids = motion.source_snapshot_ids();
        assert!(source_ids.contains(&old_id));
        assert!(source_ids.contains(&new_id));
        match &motion.slices[0] {
            SliceMotion::Conceal {
                old,
                start_clip,
                direction,
                ..
            } => {
                assert_eq!(old.source.snapshot_id, old_id);
                // 旧图从当前可见遮罩继续退场，沿用原方向。
                assert_eq!(*start_clip, rect(0.0, 0.0, 10.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
            }
            other => panic!("expected old Conceal first, got {other:?}"),
        }
        match &motion.slices[1] {
            SliceMotion::Reveal { new, start_clip, .. } => {
                assert_eq!(new.source.snapshot_id, new_id);
                // 新字形从零宽重新露出，不继承旧字宽度。
                assert_eq!(*start_clip, rect(0.0, 0.0, 0.0, 10.0));
            }
            other => panic!("expected new Reveal second, got {other:?}"),
        }
    }

    #[test]
    fn replacement_patch_is_insert_and_replace_but_pure_insert_is_not_replace() {
        let insert = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
        assert!(inserted_in_patch(&insert, (0, 3)));
        assert!(!replaced_in_patch(&insert, (0, 3)));

        let replace = make_edit("甲", "乙", 3, 3, (0, 3), (0, 3), "乙");
        assert!(inserted_in_patch(&replace, (0, 3)));
        assert!(replaced_in_patch(&replace, (0, 3)));
    }

    #[test]
    fn new_to_old_byte_removes_insertion_delta_after_patch() {
        // "甲乙" → 段首插入"丙"：新坐标 3..6 映回旧坐标 0..3。
        let edit = make_edit("甲乙", "丙甲乙", 0, 3, (0, 0), (0, 3), "丙");
        assert_eq!(new_to_old_byte(&edit, 3), 0);
        assert_eq!(new_to_old_byte(&edit, 6), 3);
    }

    // ── 复核 12：旧图接续起点取上一帧“实际显示画面”的端到端回归 ──────────

    /// 带非默认 shaping 指纹的 cluster：模拟同一 byte range 但字形/shaping 已变。
    fn cluster_shaped(
        byte_start: usize,
        byte_end: usize,
        x: f64,
        w: f64,
        glyph_hash: u64,
    ) -> LineClusterSnapshot {
        LineClusterSnapshot {
            byte_start,
            byte_end,
            source_rect: rect(x, 0.0, w, 10.0),
            shaping_identity: ShapingIdentity {
                glyph_indexes_hash: glyph_hash,
                ..ShapingIdentity::default()
            },
        }
    }

    /// 上一帧真正提交显示的 `Shift` 片段：`slice.rect` 是中途插值位置，
    /// `mask_clip` 是该位置的实际动画遮罩（Shift 不裁宽度，等于完整 rect）。
    fn drawn_shift(line_id: LineSnapshotId, displayed: LocalRect) -> LocalDrawnFrame {
        LocalDrawnFrame {
            session: 7,
            revision: 1,
            slices: vec![DrawnSlice {
                slice: VisualSlice {
                    source: SnapshotRef {
                        snapshot_id: line_id,
                        source_uv: rect(0.0, 0.0, 20.0, 10.0),
                    },
                    rect: displayed,
                },
                visible_width: displayed.w,
                clip_rect: displayed,
                mask_clip: displayed,
                motion_kind: MotionKind::Shift,
                byte_range: (0, 3),
                shaping_identity: identity(),
                direction: ClipDirection::LeftToRight,
            }],
        }
    }

    #[test]
    fn shift_interrupted_by_shaping_change_conceals_from_last_shown_position() {
        // Issue #919 复核 7：上一帧“甲”正在 Shift（旧 0→新 20），本笔替换使
        // shaping 变化。退场必须从上一帧真正显示的中途位置 (x=10) 原地吞，
        // 而不是从旧 canonical (x=0) 或新目标 (x=20) 突然开始。
        let old_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster_shaped(0, 3, 0.0, 20.0, 0)],
            0.0,
        )]);
        // 新布局含两块 cluster：
        // - “甲” byte 0..3：x=20、shaping 已变（glyph_hash=7）。
        // - “乙” byte 3..6：x=40、新插入字。
        // 注意：“甲”从旧 x=0 变成新 x=20 是人为构造的目标几何，用于验证中途
        // 位置接续；普通 LTR 文本在末尾追加“乙”不会把“甲”平移到 x=20。
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![
                cluster_shaped(0, 3, 20.0, 20.0, 7),
                cluster(3, 6, 40.0, 20.0),
            ],
            0.0,
        )]);
        let old_id = old_layout.line_snapshots[0].id;
        let new_id = new_layout.line_snapshots[0].id;
        assert_ne!(old_id, new_id);

        // 上一帧“甲”已经离开 x=0，插值显示在 x=10，源仍是旧排版行图。
        let last = drawn_shift(old_id, rect(10.0, 0.0, 20.0, 10.0));

        // 在“甲”之后插入“乙”，新排版把“甲”放到 x=20 且 shaping 已变。
        let edit = make_edit("甲", "甲乙", 3, 6, (3, 3), (3, 6), "乙");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

        // 三条切片：旧“甲” Conceal + 新 shaping“甲” Reveal + 新插入“乙” Reveal。
        assert_eq!(motion.slices.len(), 3);
        let ids = motion.source_snapshot_ids();
        assert!(ids.contains(&old_id));
        assert!(ids.contains(&new_id));

        // 按 byte_range + motion_kind 定位，不依赖切片顺序。
        let conceal_jia = motion
            .slices
            .iter()
            .find(|s| s.byte_range() == (0, 3) && s.motion_kind() == MotionKind::Conceal)
            .expect("应有旧“甲” Conceal");
        let reveal_jia = motion
            .slices
            .iter()
            .find(|s| s.byte_range() == (0, 3) && s.motion_kind() == MotionKind::Reveal)
            .expect("应有新 shaping“甲” Reveal");
        let reveal_yi = motion
            .slices
            .iter()
            .find(|s| s.byte_range() == (3, 6) && s.motion_kind() == MotionKind::Reveal)
            .expect("应有新插入“乙” Reveal");

        match conceal_jia {
            SliceMotion::Conceal { old, start_clip, .. } => {
                assert_eq!(old.source.snapshot_id, old_id);
                // 起点是上一帧实际显示位置，不是 old_span.slice.rect (x=0)。
                assert_eq!(old.rect, rect(10.0, 0.0, 20.0, 10.0));
                assert_eq!(*start_clip, rect(10.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Conceal, got {other:?}"),
        }
        match reveal_jia {
            SliceMotion::Reveal { new, start_clip, .. } => {
                assert_eq!(new.source.snapshot_id, new_id);
                assert_eq!(new.rect, rect(20.0, 0.0, 20.0, 10.0));
                // 新字形从零宽重新露出，不继承上一帧旧图位置。
                assert_eq!(*start_clip, rect(20.0, 0.0, 0.0, 10.0));
            }
            other => panic!("expected Reveal, got {other:?}"),
        }
        match reveal_yi {
            SliceMotion::Reveal { new, start_clip, .. } => {
                assert_eq!(new.source.snapshot_id, new_id);
                // 新插入的“乙”位于 x=40，从零宽开始露出。
                assert_eq!(new.rect, rect(40.0, 0.0, 20.0, 10.0));
                assert_eq!(*start_clip, rect(40.0, 0.0, 0.0, 10.0));
            }
            other => panic!("expected Reveal, got {other:?}"),
        }
    }

    #[test]
    fn shift_continuation_uses_unclipped_current_rect() {
        // Issue #919 复核 15：上一帧 Shift 片段被视口裁过一部分时，`DrawnSlice`
        // 的 `clip_rect`（视口裁剪后）与 `mask_clip`（`sample()` 产生的完整插值
        // rect）不同。下一笔继续位移必须从 `mask_clip`（真实当前画面位置）接新
        // 目标，而不是从 `clip_rect` 的视口边缘——否则被裁掉的位移字形会把可见
        // 部分挪到视口边界，内容切片错位、在视口边缘跳变。
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        )]);
        let old_id = old_layout.line_snapshots[0].id;

        // 上一帧“甲”已从 x=0 插值到 x=10（完整 rect 10..30），但被视口左边缘
        // 裁到 12..28：clip_rect != mask_clip。
        let last = frame_of(vec![DrawnSlice {
            slice: VisualSlice {
                source: SnapshotRef {
                    snapshot_id: old_id,
                    source_uv: rect(0.0, 0.0, 20.0, 10.0),
                },
                rect: rect(10.0, 0.0, 20.0, 10.0),
            },
            visible_width: 16.0,
            clip_rect: rect(12.0, 0.0, 16.0, 10.0),
            mask_clip: rect(10.0, 0.0, 20.0, 10.0),
            motion_kind: MotionKind::Shift,
            byte_range: (0, 3),
            shaping_identity: identity(),
            direction: ClipDirection::LeftToRight,
        }]);

        // 段首插入“乙”，未改的“甲”继续位移到 x=20。
        let edit = make_edit("甲", "乙甲", 0, 3, (0, 0), (0, 3), "乙");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

        let shift = motion
            .slices
            .iter()
            .find(|s| s.motion_kind() == MotionKind::Shift)
            .expect("应有“甲” Shift 接续");
        match shift {
            SliceMotion::Shift { from, to, .. } => {
                assert_eq!(
                    *from,
                    rect(10.0, 0.0, 20.0, 10.0),
                    "必须从上一帧完整插值 rect 接续，而不是视口裁剪后的 clip_rect"
                );
                assert_eq!(*to, rect(20.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Shift, got {other:?}"),
        }
    }

    #[test]
    fn deleting_half_revealed_glyph_continues_from_visible_clip() {
        // Issue #919 复核 6：刚吐到一半的字立即 Backspace，退场起点取上一帧
        // 已显示的 mask_clip（半宽），不先把半字补成整字再吞。
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![]);
        let old_id = old_layout.line_snapshots[0].id;

        // 上一帧“甲”是 Reveal，只露出左半 (0..10)。
        let last = drawn_reveal(
            old_id,
            rect(0.0, 0.0, 10.0, 10.0),
            ShapingIdentity::default(),
        );

        // Backspace：光标从 3 左移到 0。
        let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

        assert_eq!(motion.slices.len(), 1);
        match &motion.slices[0] {
            SliceMotion::Conceal {
                old,
                start_clip,
                direction,
                ..
            } => {
                assert_eq!(old.source.snapshot_id, old_id);
                // 从已显示的半宽遮罩继续吞，而不是从整字 (w=20) 重新开始。
                assert_eq!(*start_clip, rect(0.0, 0.0, 10.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
            }
            other => panic!("expected Conceal, got {other:?}"),
        }
    }

    // ── 复核 13：删除边界处未改文字不得被当成新字 Reveal ───────────────────

    #[test]
    fn backspace_does_not_reveal_unchanged_preceding_cluster() {
        // 正文“甲乙丙”，Backspace 删除“乙”（旧 3..6）。未改的“甲”仍占旧/新
        // 0..3，必须保持静态，不能被映成删除后坐标而重新露出。
        let old_layout = snapshot(vec![test_line(
            0,
            0,
            vec![
                cluster(0, 3, 0.0, 20.0),
                cluster(3, 6, 20.0, 20.0),
                cluster(6, 9, 40.0, 20.0),
            ],
            0.0,
        )]);
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        )]);

        // Backspace：旧光标 6 → 新光标 3，删除“乙”3..6。
        let edit = make_edit("甲乙丙", "甲丙", 6, 3, (3, 6), (3, 3), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        assert!(
            motion
                .slices
                .iter()
                .all(|s| s.motion_kind() != MotionKind::Reveal),
            "未改的“甲”不应被 Reveal: {:?}",
            motion.slices
        );
        // 被删的“乙”退场；“丙”从左邻删除后左移到 x=20。
        let conceal = motion
            .slices
            .iter()
            .find(|s| s.motion_kind() == MotionKind::Conceal)
            .expect("应有“乙” Conceal");
        assert_eq!(conceal.byte_range(), (3, 6));
        let shift = motion
            .slices
            .iter()
            .find(|s| s.motion_kind() == MotionKind::Shift)
            .expect("应有“丙” Shift");
        match shift {
            SliceMotion::Shift { from, to, .. } => {
                assert_eq!(*from, rect(40.0, 0.0, 20.0, 10.0));
                assert_eq!(*to, rect(20.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Shift, got {other:?}"),
        }
    }

    #[test]
    fn forward_delete_does_not_reveal_unchanged_following_cluster() {
        // 正文“甲乙丙”，Forward Delete 删除“甲”（旧 0..3）。未改的“乙”“丙”
        // 左移，不得产生 Reveal。
        let old_layout = snapshot(vec![test_line(
            0,
            0,
            vec![
                cluster(0, 3, 0.0, 20.0),
                cluster(3, 6, 20.0, 20.0),
                cluster(6, 9, 40.0, 20.0),
            ],
            0.0,
        )]);
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        )]);

        // Forward Delete：光标停在 0（未左移），删除“甲”0..3。
        let edit = make_edit("甲乙丙", "乙丙", 0, 0, (0, 3), (0, 0), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        assert!(
            motion
                .slices
                .iter()
                .all(|s| s.motion_kind() != MotionKind::Reveal),
            "删除后未改文字不应 Reveal: {:?}",
            motion.slices
        );
        let conceal = motion
            .slices
            .iter()
            .find(|s| s.motion_kind() == MotionKind::Conceal)
            .expect("应有“甲” Conceal");
        assert_eq!(conceal.byte_range(), (0, 3));
        // 「乙」「丙」未改、同视觉行、位移相同（均左移 20）、字节连续、源 UV 连续，
        // 按 #919 连续片段分组合并成一条 Shift（byte_range 0..6），不得产生 Reveal。
        let shifts: Vec<_> = motion
            .slices
            .iter()
            .filter(|s| s.motion_kind() == MotionKind::Shift)
            .collect();
        assert_eq!(shifts.len(), 1, "乙丙应合并为一条连续 Shift: {:?}", motion.slices);
        match shifts[0] {
            SliceMotion::Shift { new, from, to, byte_range, .. } => {
                assert_eq!(*byte_range, (0, 6));
                assert_eq!((from.x, from.y, from.w, from.h), (20.0, 0.0, 40.0, 10.0));
                assert_eq!((to.x, to.y, to.w, to.h), (0.0, 0.0, 40.0, 10.0));
                assert_eq!(new.source.snapshot_id, new_layout.line_snapshots[0].id);
            }
            other => panic!("expected Shift, got {other:?}"),
        }
    }

    // ── 复核 14：在途旧图接续与旧/新画面归属的补充回归 ─────────────────────

    fn frame_of(slices: Vec<DrawnSlice>) -> LocalDrawnFrame {
        LocalDrawnFrame {
            session: 7,
            revision: 1,
            slices,
        }
    }

    fn drawn_slice(
        source: SnapshotRef,
        displayed: LocalRect,
        mask_clip: LocalRect,
        motion_kind: MotionKind,
        direction: ClipDirection,
        byte_range: (usize, usize),
    ) -> DrawnSlice {
        DrawnSlice {
            slice: VisualSlice {
                source,
                rect: displayed,
            },
            visible_width: mask_clip.w,
            clip_rect: mask_clip,
            mask_clip,
            motion_kind,
            byte_range,
            shaping_identity: identity(),
            direction,
        }
    }

    #[test]
    fn previous_deletion_conceal_continues_into_next_edit() {
        // Issue #919 复核 3：上一笔删除但仍在途的旧 Conceal 已不在当前 Core 正文，
        // 不会进入本轮 old_spans；必须从 last_drawn 直接沿用其画面继续吞到 0，
        // 否则连续按 Backspace 时，上一笔还可见的半个旧字会直接从新计划消失。
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![]);
        let jia_id = old_layout.line_snapshots[0].id;

        // 上一帧：更早一笔删除的“乙”（独立旧排版行图，已不在本轮正文）正吞到一半。
        let yi_source = SnapshotRef {
            snapshot_id: LineSnapshotId::new(999, 1, 0),
            source_uv: rect(20.0, 0.0, 20.0, 10.0),
        };
        let last = frame_of(vec![drawn_slice(
            yi_source,
            rect(20.0, 0.0, 20.0, 10.0),
            rect(20.0, 0.0, 10.0, 10.0),
            MotionKind::Conceal,
            ClipDirection::LeftToRight,
            (3, 6),
        )]);

        // 本笔 Backspace 删除“甲”。
        let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

        let conceals: Vec<_> = motion
            .slices
            .iter()
            .filter(|s| s.motion_kind() == MotionKind::Conceal)
            .collect();
        assert_eq!(
            conceals.len(),
            2,
            "应有本轮“甲”和上一笔在途“乙”两条 Conceal: {:?}",
            motion.slices
        );

        // 本轮删除的“甲”：无同源在途旧图，从自身完整旧图开始吞。
        let jia = conceals
            .iter()
            .find(|s| s.byte_range() == (0, 3))
            .expect("应有“甲” Conceal");
        match jia {
            SliceMotion::Conceal {
                old,
                start_clip,
                direction,
                ..
            } => {
                assert_eq!(old.source.snapshot_id, jia_id);
                assert_eq!(*start_clip, rect(0.0, 0.0, 20.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
            }
            other => panic!("expected Conceal, got {other:?}"),
        }

        // 上一笔在途“乙”：沿用原 source / 原遮罩 / 原方向继续吞，不与本轮合并。
        let yi = conceals
            .iter()
            .find(|s| s.byte_range() == (3, 6))
            .expect("应有在途“乙” Conceal");
        match yi {
            SliceMotion::Conceal {
                old,
                start_clip,
                direction,
                ..
            } => {
                assert_eq!(old.source, yi_source);
                assert_eq!(*start_clip, rect(20.0, 0.0, 10.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
            }
            other => panic!("expected Conceal, got {other:?}"),
        }

        let ids = motion.source_snapshot_ids();
        assert!(ids.contains(&jia_id));
        assert!(ids.contains(&yi_source.snapshot_id));
    }

    #[test]
    fn in_flight_deleted_glyph_not_swallowed_by_same_byte_range() {
        // Issue #919 复核 4：正文“甲乙丙”先删“乙”（旧 3..6）→“甲丙”，新“丙”恰好
        // 占据 3..6。在“乙”只吞一半时再 Forward Delete“甲”，不能因本轮 old_spans 的
        // 3..6 有“丙”就跳过在途“乙”的残留，也不能把“乙”的遮罩套到“丙”上。
        // 每张旧图按真实 source（snapshot_id + source_uv）区分，而不是字节位置。
        let old_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        )]);
        let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let bing_id = old_layout.line_snapshots[0].id;
        let new_id = new_layout.line_snapshots[0].id;

        // 上一笔删除的“乙”来自更早的“甲乙丙”排版（不同 revision 的行图），正在吞。
        let yi_source = SnapshotRef {
            snapshot_id: LineSnapshotId::new(999, 1, 0),
            source_uv: rect(20.0, 0.0, 20.0, 10.0),
        };
        let last = frame_of(vec![drawn_slice(
            yi_source,
            rect(20.0, 0.0, 20.0, 10.0),
            rect(20.0, 0.0, 10.0, 10.0),
            MotionKind::Conceal,
            ClipDirection::LeftToRight,
            (3, 6),
        )]);

        // Forward Delete“甲”（旧 0..3）：“丙”应 Shift，在途“乙”继续退场。
        let edit = make_edit("甲丙", "丙", 0, 0, (0, 3), (0, 0), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

        // “丙” 3..6 → 0..3 是 Shift（不因字节位置与在途“乙”相同而被判成退场/重影）。
        let shift = motion
            .slices
            .iter()
            .find(|s| s.motion_kind() == MotionKind::Shift)
            .expect("应有“丙” Shift");
        match shift {
            SliceMotion::Shift { new, from, to, .. } => {
                assert_eq!(new.source.snapshot_id, new_id);
                assert_eq!(*from, rect(20.0, 0.0, 20.0, 10.0));
                assert_eq!(*to, rect(0.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Shift, got {other:?}"),
        }

        // 本轮删除的“甲”退场。
        let jia = motion
            .slices
            .iter()
            .find(|s| {
                s.motion_kind() == MotionKind::Conceal && s.source_ref().snapshot_id == bing_id
            })
            .expect("应有“甲” Conceal");
        assert_eq!(jia.byte_range(), (0, 3));

        // 在途“乙”独立退场：保留原 source / 遮罩 / 方向，不与“丙”合并。
        let yi = motion
            .slices
            .iter()
            .find(|s| s.source_ref() == &yi_source)
            .expect("应有在途“乙” Conceal");
        match yi {
            SliceMotion::Conceal {
                start_clip,
                direction,
                ..
            } => {
                assert_eq!(*start_clip, rect(20.0, 0.0, 10.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
            }
            other => panic!("expected Conceal, got {other:?}"),
        }

        // “丙”只位移，不应被当成 Conceal（否则 3..6 处会出现两张退场图）。
        assert!(
            !motion.slices.iter().any(|s| {
                s.motion_kind() == MotionKind::Conceal
                    && s.source_ref().snapshot_id == bing_id
                    && s.source_ref().source_uv == rect(20.0, 0.0, 20.0, 10.0)
            }),
            "未改的“丙”不应退场: {:?}",
            motion.slices
        );

        let ids = motion.source_snapshot_ids();
        assert!(ids.contains(&yi_source.snapshot_id));
        assert!(ids.contains(&bing_id));
    }

    #[test]
    fn new_to_old_byte_end_maps_deletion_boundary_to_left_side() {
        // Issue #919 复核 13：删除“乙”（旧 3..6 → 新 3..3）时，同一个新偏移 3 既是
        // “甲”末尾（删除前坐标 3）也是“丙”开头（删除后坐标 3）。区间起点用
        // new_to_old_byte 取右侧（删除后）坐标，区间终点用 new_to_old_byte_end 取
        // 左侧（删除前）坐标，消除边界歧义。
        let edit = make_edit("甲乙丙", "甲丙", 6, 3, (3, 6), (3, 3), "");
        assert_eq!(new_to_old_byte(&edit, 3), 6);
        assert_eq!(new_to_old_byte_end(&edit, 3), 3);
    }

    #[test]
    fn reveal_and_shift_targets_become_static_holes() {
        // Issue #919：静态层只挖 Reveal 的新位置 / Shift 的目标位置，Conceal 是旧字
        // 不挖孔；孔洞固定取自计划 target 排除集合，与 progress 无关。
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        )]);
        // 段首插入“乙”：新字 Reveal(x=0)，“甲”未改但被挤到 x=20 → Shift(to x=20)。
        let edit = make_edit("甲", "乙甲", 0, 3, (0, 0), (0, 3), "乙");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);
        let new_id = new_layout.line_snapshots[0].id;

        let holes = motion.sample(0.0).static_holes;
        assert_eq!(holes.len(), 2, "Reveal + Shift 各挖一个孔: {holes:?}");
        assert!(holes.iter().any(|h| h.x == 0.0 && h.snapshot_id == new_id));
        assert!(holes.iter().any(|h| h.x == 20.0 && h.snapshot_id == new_id));

        // 与 progress 无关：中间帧孔洞不变。
        assert_eq!(motion.sample(50.0).static_holes.len(), 2);
        // 过渡结束后静态层恢复完整 canonical，不再挖孔。
        assert!(motion.sample(100.0).static_holes.is_empty());
    }

    #[test]
    fn conceal_plan_does_not_create_static_hole() {
        // Issue #919：纯删除只有旧字 Conceal，静态层不挖孔（旧字形仍由静态/覆盖层
        // 唯一绘制，不能同时把相同像素画两次）。
        let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let new_layout = snapshot(vec![]);
        let old_id = old_layout.line_snapshots[0].id;
        let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        assert!(motion.sample(0.0).static_holes.is_empty());
        assert_eq!(motion.required_textures, vec![old_id]);
    }

    #[test]
    fn paragraph_with_content_change_does_not_also_get_paragraph_move() {
        // Issue #919 复核 2：同一段落只能由一种位移路径接管。段落既有真实内容变更
        // （display_patches 命中）又因上方插入新段而整体下移时，不能再叠加整体
        // ParagraphMove——否则该段同时采用“整段静态节点 y 位移”和“局部字形旧→新
        // 位移/遮罩”，两套画面坐标不一致，会跳行/重影。
        //
        // 构造：一次事务两笔 patch。旧正文“甲\n乙”两段（稳定 ID 6、7）；
        // 新正文“甲\n\n丙”三段——段 6“甲”不变、新增空段 8、段 7“乙”→“丙”。
        // 段 7 段首从 y=20 下移到 y=40（因中间插入空段 8），且段 7 内容已变，
        // 必须只走局部退场/露出。
        let mut old_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
        old_p6.stable_paragraph_id = 6;
        let mut old_p7 = test_line(1, 4, vec![cluster(4, 7, 0.0, 20.0)], 20.0);
        old_p7.stable_paragraph_id = 7;
        let old_layout =
            snapshot_with_paragraph_index(vec![old_p6, old_p7], vec![20.0, 20.0], vec![6, 7]);

        let mut new_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
        new_p6.stable_paragraph_id = 6;
        // 新增空段 8：位于两个换行之间（byte_start=4），无 cluster。
        let mut new_p8 = test_line(1, 4, vec![], 20.0);
        new_p8.stable_paragraph_id = 8;
        let mut new_p7 = test_line(2, 5, vec![cluster(5, 8, 0.0, 20.0)], 40.0);
        new_p7.stable_paragraph_id = 7;
        let new_layout = snapshot_with_paragraph_index(
            vec![new_p6, new_p8, new_p7],
            vec![20.0, 20.0, 20.0],
            vec![6, 8, 7],
        );

        // 直接验证段落锚点：旧段 7 在 y=20；新段 8 在 y=20、段 7 下移到 y=40。
        assert_eq!(
            old_layout.paragraph_anchor(7),
            Some((20.0, 20.0)),
            "旧段 7 段首 y=20"
        );
        assert_eq!(
            new_layout.paragraph_anchor(7),
            Some((40.0, 20.0)),
            "新段 7 段首 y=40"
        );
        assert_eq!(
            new_layout.paragraph_anchor(8),
            Some((20.0, 20.0)),
            "新段 8 段首 y=20"
        );

        // 旧正文“甲\n乙”（段 6 = 0..3，段 7 = 4..7）；新正文“甲\n\n丙”
        // （段 6 = 0..3，空段 8 = 4..4，段 7 = 5..8）。
        let edit = make_edit_from_patches(
            "甲\n乙",
            "甲\n\n丙",
            7,
            8,
            vec![((3, 3), (3, 4), "\n"), ((4, 7), (5, 8), "丙")],
        );
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        // 段 7 的段首 y 确实变了，但内容也变了：只走局部路径，无整体位移。
        assert!(
            motion.paragraph_moves.is_empty(),
            "内容变更段落不应再有 ParagraphMove: {:?}",
            motion.paragraph_moves
        );
        // 本用例没有需要逐字位移的未改 cluster。
        assert!(
            !motion
                .slices
                .iter()
                .any(|s| s.motion_kind() == MotionKind::Shift),
            "本用例不应产生 Shift: {:?}",
            motion.slices
        );
        // 段 7 的旧“乙”退场、新“丙”露出仍保留。
        assert!(
            motion.slices.iter().any(|s| matches!(
                s,
                SliceMotion::Conceal { .. } if s.byte_range() == (4, 7)
            )),
            "旧“乙”应退场: {:?}",
            motion.slices
        );
        assert!(
            motion.slices.iter().any(|s| matches!(
                s,
                SliceMotion::Reveal { byte_range, .. } if *byte_range == (5, 8)
            )),
            "新“丙”应露出: {:?}",
            motion.slices
        );
    }

    #[test]
    fn offscreen_edit_without_visible_slices_returns_none() {
        // Issue #919：完全落在视口外的编辑不产生局部动画——所有片段经
        // `slices.retain(|s| rects_intersect(s.motion_rect(), viewport))` 被滤掉后，
        // 若也没有整体段落位移，`begin_or_retarget` 应返回 None，让静态层直接画
        // canonical。屏幕外的字在滚动进入视口时由静态绘制呈现，不需要在空中预跑
        // 一段看不见的吞吐动画。
        let new_layout = snapshot(vec![test_line(
            0,
            0,
            vec![cluster(0, 3, 2000.0, 20.0)],
            0.0,
        )]);
        let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
        assert!(
            TextMotion::begin_or_retarget(
                &edit,
                None,
                &new_layout,
                None,
                0.0,
                100,
                1,
                viewport(),
            )
            .is_none(),
            "视口外编辑不应创建局部动画"
        );
    }

    #[test]
    fn newline_insert_moves_downstream_unchanged_paragraph_as_a_whole() {
        // Issue #919 复核 2：与「内容变更段落不再叠加整体位移」互为对照——内容未改的
        // 下游段落因上方插入换行而整体下移时，必须由唯一一条 `ParagraphMove` 接管，
        // 不再为段内每个字生成逐字切片，避免同段文字被静态位移和局部覆盖层画两次。
        //
        // 构造：旧正文“甲\n乙”两段（稳定 ID 6、7）；在“甲”末尾回车换行得到
        // “甲\n\n乙”——段 6“甲”不变、新增空段 8、段 7“乙”内容未改但整体下移。
        let mut old_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
        old_p6.stable_paragraph_id = 6;
        let mut old_p7 = test_line(1, 4, vec![cluster(4, 7, 0.0, 20.0)], 20.0);
        old_p7.stable_paragraph_id = 7;
        let old_layout =
            snapshot_with_paragraph_index(vec![old_p6, old_p7], vec![20.0, 20.0], vec![6, 7]);

        let mut new_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
        new_p6.stable_paragraph_id = 6;
        // 新增空段 8：位于两个换行之间（byte_start=4），无 cluster。
        let mut new_p8 = test_line(1, 4, vec![], 20.0);
        new_p8.stable_paragraph_id = 8;
        let mut new_p7 = test_line(2, 5, vec![cluster(5, 8, 0.0, 20.0)], 40.0);
        new_p7.stable_paragraph_id = 7;
        let new_layout = snapshot_with_paragraph_index(
            vec![new_p6, new_p8, new_p7],
            vec![20.0, 20.0, 20.0],
            vec![6, 8, 7],
        );

        // 段 7 内容未改，但段首因中间插入空段 8 从 y=20 下移到 y=40。
        assert_eq!(old_layout.paragraph_anchor(7), Some((20.0, 20.0)));
        assert_eq!(new_layout.paragraph_anchor(7), Some((40.0, 20.0)));

        // 在“甲”末尾（旧 3..3）插入换行，新插入区间 3..4。
        let edit = make_edit("甲\n乙", "甲\n\n乙", 3, 4, (3, 3), (3, 4), "\n");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        // 未改内容的段 7 只由唯一一条整体位移接管。
        assert_eq!(
            motion.paragraph_moves,
            vec![ParagraphMove {
                paragraph_id: 7,
                from_y: 20.0,
                to_y: 40.0,
            }],
            "段 7 应只走一条整体 ParagraphMove: {:?}",
            motion.paragraph_moves
        );
        // 段 7 的文字不产生逐字覆盖层（同段文字不能既静态位移又局部重画）。
        assert!(
            !motion
                .slices
                .iter()
                .any(|s| s.byte_range() == (4, 7) || s.byte_range() == (5, 8)),
            "整体位移的段 7 不应再产生逐字切片: {:?}",
            motion.slices
        );

        // 同一 progress 采样：起点为把目标位置补回旧 y 的完整偏移，结束回到 0。
        assert_eq!(motion.sample(0.0).paragraph_y_offsets, vec![(7, -20.0)]);
        assert!(motion.sample(100.0).paragraph_y_offsets.is_empty());
    }

    #[test]
    fn newline_delete_moves_downstream_unchanged_paragraph_up_as_a_whole() {
        // Issue #919 复核 2：与「插入换行让下游段整体下移」对称——删除一个换行使
        // 上方的空段消失、下游内容未改的段整体上移时，同样只能由唯一一条
        // `ParagraphMove` 接管（`from_y` 大于 `to_y`，offset 起点为正），不再为
        // 段内每个字生成逐字切片，避免同段文字被静态位移和局部覆盖层画两次。
        //
        // 构造：旧正文“甲\n\n乙”三段（稳定 ID 6、8、7）——段 6“甲”、空段 8、
        // 段 7“乙”；删除段 6 末尾换行（旧 3..4）得到新正文“甲\n乙”，空段 8 消失、
        // 段 7“乙”内容未改但整体从 y=40 上移到 y=20。
        let mut old_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
        old_p6.stable_paragraph_id = 6;
        // 空段 8：位于两个换行之间（byte_start=4），无 cluster。
        let mut old_p8 = test_line(1, 4, vec![], 20.0);
        old_p8.stable_paragraph_id = 8;
        let mut old_p7 = test_line(2, 5, vec![cluster(5, 8, 0.0, 20.0)], 40.0);
        old_p7.stable_paragraph_id = 7;
        let old_layout = snapshot_with_paragraph_index(
            vec![old_p6, old_p8, old_p7],
            vec![20.0, 20.0, 20.0],
            vec![6, 8, 7],
        );

        let mut new_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
        new_p6.stable_paragraph_id = 6;
        let mut new_p7 = test_line(1, 4, vec![cluster(4, 7, 0.0, 20.0)], 20.0);
        new_p7.stable_paragraph_id = 7;
        let new_layout =
            snapshot_with_paragraph_index(vec![new_p6, new_p7], vec![20.0, 20.0], vec![6, 7]);

        // 段 7 内容未改，但段首因删除上方换行/空段从 y=40 上移到 y=20。
        assert_eq!(old_layout.paragraph_anchor(7), Some((40.0, 20.0)));
        assert_eq!(new_layout.paragraph_anchor(7), Some((20.0, 20.0)));
        // 空段 8 只存在于旧布局，新布局没有它的锚点。
        assert!(new_layout.paragraph_anchor(8).is_none());

        // 删除“甲”后的换行：旧 3..4 → 新插入区间 3..3（空）。
        let edit = make_edit_from_patches("甲\n\n乙", "甲\n乙", 3, 3, vec![((3, 4), (3, 3), "")]);
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

        // 未改内容的段 7 只由唯一一条整体位移接管（起点为正偏移，即当前画在旧 y）。
        assert_eq!(
            motion.paragraph_moves,
            vec![ParagraphMove {
                paragraph_id: 7,
                from_y: 40.0,
                to_y: 20.0,
            }],
            "段 7 应只走一条整体 ParagraphMove: {:?}",
            motion.paragraph_moves
        );
        // 段 7 的文字不产生逐字覆盖层（同段文字不能既静态位移又局部重画）。
        assert!(
            !motion
                .slices
                .iter()
                .any(|s| s.byte_range() == (5, 8) || s.byte_range() == (4, 7)),
            "整体位移的段 7 不应再产生逐字切片: {:?}",
            motion.slices
        );

        // 同一 progress 采样：起点为把目标位置补回旧 y 的完整正偏移，结束回到 0。
        assert_eq!(motion.sample(0.0).paragraph_y_offsets, vec![(7, 20.0)]);
        assert!(motion.sample(100.0).paragraph_y_offsets.is_empty());
    }

    #[test]
    fn sample_clip_rect_is_viewport_intersected_but_mask_clip_is_original() {
        // Issue #919 复核 5：`sample()` 产生的原始动画遮罩 `mask_clip` 不被视口收缩，
        // 只有最终可见的 `clip_rect` 与视口求交。二者若混用，下一笔接续时会把视口
        // 裁剪边缘当成动画起点；`scene_graph_renderer` 也分别回执这两个矩形。
        let mut motion = conceal_with_start_clip(ClipDirection::LeftToRight);
        // start_clip = 50..58；把视口左边界移到 52，切掉左侧 2 像素。
        motion.viewport = rect(52.0, 0.0, 1000.0, 1000.0);

        let overlay = motion
            .sample(0.0)
            .overlays
            .into_iter()
            .next()
            .expect("conceal overlay");
        // mask_clip 保持整段原始遮罩 50..58。
        assert_eq!(
            (overlay.mask_clip.x, overlay.mask_clip.w),
            (50.0, 8.0),
            "mask_clip 不应被视口收缩"
        );
        // clip_rect 被视口裁到 52..58。
        assert_eq!((overlay.clip_rect.x, overlay.clip_rect.w), (52.0, 6.0));
        // 右边界相同但起点不同，clip_rect 不能代替 mask_clip。
        assert_eq!(
            overlay.clip_rect.x + overlay.clip_rect.w,
            overlay.mask_clip.x + overlay.mask_clip.w
        );
        assert_ne!(overlay.clip_rect, overlay.mask_clip);
    }

    #[test]
    fn caret_sample_uses_same_shared_progress() {
        // Issue #919：唯一光标控制器与文字动画共用同一 progress。这里以纯几何
        // `TextMotion` 验证光标按同一条 ease-out 进度在真实布局 caret 之间插值，
        // 不新建第二条时间线。
        let mut motion = make_motion(Vec::new());
        motion.caret_from = Some((0.0, 0.0));
        motion.caret_to = Some((40.0, 20.0));

        assert_eq!(motion.sample(0.0).caret_sample, Some((0.0, 0.0)));

        // raw=0.5 → progress=ease_out_cubic(0.5)=0.875。
        let mid = motion.sample(50.0).caret_sample.expect("caret sample");
        assert!((mid.0 - 35.0).abs() < 1e-9, "x={}", mid.0);
        assert!((mid.1 - 17.5).abs() < 1e-9, "y={}", mid.1);

        // 结束时停在目标 caret，不残留中途位置。
        assert_eq!(motion.sample(100.0).caret_sample, Some((40.0, 20.0)));
    }

    #[test]
    fn caret_sample_falls_back_to_present_endpoint() {
        // Issue #919：起点或终点 caret 缺失时（如首次布局、光标在视口外），保留
        // 可用的另一侧，不因缺一边而整体失去光标运动。
        let mut motion = make_motion(Vec::new());
        motion.caret_from = None;
        motion.caret_to = Some((10.0, 5.0));
        assert_eq!(motion.sample(0.0).caret_sample, Some((10.0, 5.0)));

        motion.caret_from = Some((3.0, 4.0));
        motion.caret_to = None;
        assert_eq!(motion.sample(0.0).caret_sample, Some((3.0, 4.0)));
    }

    #[test]
    fn wrapped_unchanged_text_shifts_per_visual_line_without_merging_textures() {
        // Issue #919 复核 16：patch 外未改的连续文本按旧/新视觉行的连续片段分组，
        // 而不是每个 cluster 各一条 Shift。本用例构造同段「甲乙丙丁」在旧布局折成
        // 首行「甲乙丙」、次行「丁」（每行容纳 3 个等宽字）；段首插入「戊己」后折行
        // 点改变：新首行「戊己甲」、新次行「乙丙丁」。
        //
        // 位置映射：甲 (0,0)→(40,0)；乙 (20,0)→(0,20)；丙 (40,0)→(20,20)；
        // 丁 (0,20)→(40,20)。乙、丙位移同为 (-20,20) 且同属旧首行→新次行，源 UV
        // 连续，应合并成一条连续 Shift；甲（旧首行→新首行）和丁（旧次行→新次行）
        // 各为其他组合，不合并。最终 3 条 Shift，每条只用单一新行纹理。
        const PARA: u64 = 5;
        let line = |ordinal: u32, byte_start: usize, clusters, top: f64| {
            let mut l = test_line(ordinal, byte_start, clusters, top);
            l.stable_paragraph_id = PARA;
            l
        };

        // 旧：一段「甲乙丙丁」折两行——首行「甲乙丙」、次行「丁」（每行 3 字容量）。
        let old_layout = snapshot(vec![
            line(
                0,
                0,
                vec![
                    cluster(0, 3, 0.0, 20.0),
                    cluster(3, 6, 20.0, 20.0),
                    cluster(6, 9, 40.0, 20.0),
                ],
                0.0,
            ),
            line(1, 9, vec![cluster(9, 12, 0.0, 20.0)], 20.0),
        ]);
        // 新：段首插入「戊己」——首行「戊己甲」、次行「乙丙丁」，折行点改变。
        let new_layout = snapshot(vec![
            line(
                0,
                0,
                vec![
                    cluster(0, 3, 0.0, 20.0),
                    cluster(3, 6, 20.0, 20.0),
                    cluster(6, 9, 40.0, 20.0),
                ],
                0.0,
            ),
            line(
                1,
                9,
                vec![
                    cluster(9, 12, 0.0, 20.0),
                    cluster(12, 15, 20.0, 20.0),
                    cluster(15, 18, 40.0, 20.0),
                ],
                20.0,
            ),
        ]);

        let old_line0_id = old_layout.line_snapshots[0].id;
        let old_line1_id = old_layout.line_snapshots[1].id;
        let new_line0_id = new_layout.line_snapshots[0].id;
        let new_line1_id = new_layout.line_snapshots[1].id;
        assert!(!old_line0_id.is_none() && !old_line1_id.is_none());
        assert!(!new_line0_id.is_none() && !new_line1_id.is_none());
        assert_ne!(old_line0_id, old_line1_id);
        assert_ne!(new_line0_id, new_line1_id);

        // 段首插入「戊己」：旧 0..0 → 新 0..6。
        let edit = make_edit("甲乙丙丁", "戊己甲乙丙丁", 0, 6, (0, 0), (0, 6), "戊己");
        let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 1);

        let shifts: Vec<&SliceMotion> = motion
            .slices
            .iter()
            .filter(|s| s.motion_kind() == MotionKind::Shift)
            .collect();
        // 甲、乙丙（合并）、丁各一条，共 3 条。
        assert_eq!(
            shifts.len(),
            3,
            "甲/乙丙合并/丁应共 3 条 Shift: {:?}",
            motion.slices
        );

        // 每条 Shift 只使用单一新行纹理；两行纹理都被分别引用，不跨行硬拼同一纹理。
        let mut shift_source_ids: Vec<LineSnapshotId> = Vec::new();
        for s in &shifts {
            let id = s.source_snapshot_id();
            if !shift_source_ids.contains(&id) {
                shift_source_ids.push(id);
            }
        }
        assert_eq!(
            shift_source_ids.len(),
            2,
            "移位片段必须分别引用两行的 new 版纹理，实际 {shift_source_ids:?}"
        );
        assert!(shift_source_ids.contains(&new_line0_id));
        assert!(shift_source_ids.contains(&new_line1_id));
        // 旧行纹理绝不能被当作新画面来源（不能拿旧图冒充新字）。
        assert!(shifts
            .iter()
            .all(|s| s.source_snapshot_id() != old_line0_id
                && s.source_snapshot_id() != old_line1_id));

        // 「甲」留在新首行：旧首行 x=0 → 新首行 x=40，纹理取自新首行。
        match shifts
            .iter()
            .find(|s| s.byte_range() == (6, 9))
            .copied()
            .expect("甲 Shift")
        {
            SliceMotion::Shift { new, from, to, .. } => {
                assert_eq!(new.source.snapshot_id, new_line0_id);
                assert_eq!((from.x, from.y), (0.0, 0.0));
                assert_eq!((to.x, to.y), (40.0, 0.0));
            }
            other => panic!("甲应为 Shift: {other:?}"),
        }

        // 「乙丙」合并为一条连续 Shift：旧首行 x=20..60 → 新次行 x=0..40，
        // 纹理取自新次行，byte_range 连续 (9, 15)。
        match shifts
            .iter()
            .find(|s| s.byte_range() == (9, 15))
            .copied()
            .expect("乙丙合并 Shift")
        {
            SliceMotion::Shift { new, from, to, .. } => {
                assert_eq!(
                    new.source.snapshot_id, new_line1_id,
                    "乙丙的新画面必须取自新次行纹理"
                );
                assert_eq!(
                    (from.x, from.y, from.w, from.h),
                    (20.0, 0.0, 40.0, 10.0),
                    "乙丙旧画面并集"
                );
                assert_eq!(
                    (to.x, to.y, to.w, to.h),
                    (0.0, 20.0, 40.0, 10.0),
                    "乙丙新画面并集"
                );
                assert_eq!(
                    (new.source.source_uv.x, new.source.source_uv.w),
                    (0.0, 40.0),
                    "乙丙源 UV 合并为连续区间"
                );
            }
            other => panic!("乙丙应为合并 Shift: {other:?}"),
        }

        // 「丁」从旧次行移到新次行末尾：旧次行 x=0 → 新次行 x=40。
        match shifts
            .iter()
            .find(|s| s.byte_range() == (15, 18))
            .copied()
            .expect("丁 Shift")
        {
            SliceMotion::Shift { new, from, to, .. } => {
                assert_eq!(new.source.snapshot_id, new_line1_id);
                assert_eq!((from.x, from.y), (0.0, 20.0), "丁的旧画面在旧次行");
                assert_eq!((to.x, to.y), (40.0, 20.0), "丁移入新次行末尾");
            }
            other => panic!("丁应为 Shift: {other:?}"),
        }
    }

    /// Issue #919 复核 16：合并 Shift 的连续重定向。
    ///
    /// 第一笔编辑把"乙丙"从 (20,0) Shift 到 (0,10)，`merge_adjacent_shifts` 合为
    /// 一条 `byte_range=(9,15)` 的 Shift。`sample(duration/2)` 后合并片段在插值
    /// 位置 (2.5, 8.75, 40, 10)。第二笔编辑使"乙""丙"再次移动（从 y=10 到 y=20/30）。
    ///
    /// 断言：第二笔的"乙""丙"各自 Shift 的 `from` 是上一帧该 cluster 的真实显示位置
    /// （由 `restore_cluster_rect` 从合并 `mask_clip` 按 UV 比例还原），而非旧布局的
    /// canonical 终点 (0,10)/(20,10)。此测试比只检查一次性 `shifts.len()==3` 更直接
    /// 覆盖合并逻辑的完整生命周期。
    #[test]
    fn merged_shift_retargets_from_interpolated_position() {
        // --- 先构造第二笔 old_layout，取"乙丙"行 id 作为第一笔合并 Shift 的源 ---
        // Issue #919 复核 17：让第二笔 old_layout 与上一笔目标快照具备相同的源
        // revision / 源行映射。find_merged_shift 现在核对 snapshot_id + source_uv，
        // 若第一笔 snap 用与 old_layout 无关的硬编码 id，snapshot_id 核对会失败，
        // 测试无法证明合并 Shift 的接续真正生效。
        let old_layout = snapshot(vec![
            test_line(
                0,
                0,
                vec![
                    cluster(0, 3, 0.0, 20.0),
                    cluster(3, 6, 20.0, 20.0),
                    cluster(6, 9, 40.0, 20.0),
                ],
                0.0,
            ),
            test_line(
                1,
                9,
                vec![cluster(9, 12, 0.0, 20.0), cluster(12, 15, 20.0, 20.0)],
                10.0,
            ),
        ]);
        // "乙丙"行是 old_layout.line_snapshots[1]，其 id 即第一笔合并 Shift 的源
        // snapshot_id。sample() 透传 new.source.snapshot_id 到 DrawnSlice，下一笔
        // retarget 时 old_spans 中"乙""丙"的 slice.source.snapshot_id 也取自此行 id，
        // find_merged_shift 的 snapshot_id + source_uv 核对才能通过。
        let merged_snap_id = old_layout.line_snapshots[1].id;

        // --- 第一笔：直接构造合并后的 TextMotion ---
        let merged_uv = rect(0.0, 0.0, 40.0, 10.0);
        let merged_new_rect = rect(0.0, 10.0, 40.0, 10.0);
        let merged_from = rect(20.0, 0.0, 40.0, 10.0);
        let merged_to = rect(0.0, 10.0, 40.0, 10.0);
        let snap = SnapshotRef {
            snapshot_id: merged_snap_id,
            source_uv: merged_uv,
        };
        let motion = TextMotion {
            slices: vec![SliceMotion::Shift {
                new: VisualSlice {
                    source: snap,
                    rect: merged_new_rect,
                },
                from: merged_from,
                to: merged_to,
                byte_range: (9, 15),
                shaping_identity: identity(),
            }],
            started_ms: 0.0,
            duration_ms: 100,
            session: 7,
            target_revision: 2,
            viewport: viewport(),
            static_exclusions: vec![],
            required_textures: vec![snap.snapshot_id],
            caret_from: None,
            caret_to: None,
            paragraph_moves: vec![],
        };

        // sample(duration/2)：progress = ease_out_cubic(0.5) = 0.875
        let frame = motion.sample(50.0);
        assert!(!frame.finished);
        assert_eq!(frame.overlays.len(), 1, "合并 Shift 应产出 1 条 overlay");
        let ov = &frame.overlays[0];
        assert_eq!(ov.motion_kind, MotionKind::Shift);
        assert_eq!(ov.byte_range, (9, 15));

        let p = 0.875_f64;
        let expected_x = 20.0 + (0.0 - 20.0) * p; // 2.5
        let expected_y = 0.0 + (10.0 - 0.0) * p; // 8.75
        assert!(
            (ov.x - expected_x).abs() < 0.01,
            "合并插值 x={} expected={}",
            ov.x,
            expected_x
        );
        assert!(
            (ov.y - expected_y).abs() < 0.01,
            "合并插值 y={} expected={}",
            ov.y,
            expected_y
        );

        // 从 overlay 构造 LocalDrawnFrame（模拟 scene_graph_renderer 的 DrawnSlice 构造）
        let drawn = DrawnSlice {
            slice: VisualSlice {
                source: SnapshotRef {
                    snapshot_id: ov.snapshot_id,
                    source_uv: ov.source_rect,
                },
                rect: rect(ov.x, ov.y, ov.w, ov.h),
            },
            visible_width: ov.clip_rect.w,
            clip_rect: ov.clip_rect,
            mask_clip: ov.mask_clip,
            motion_kind: ov.motion_kind,
            byte_range: ov.byte_range,
            shaping_identity: ov.shaping_identity.clone(),
            direction: ov.direction.unwrap_or(ClipDirection::LeftToRight),
        };
        let last_drawn = LocalDrawnFrame {
            session: 7,
            revision: 2,
            slices: vec![drawn],
        };

        // --- 第二笔编辑：在开头插入"戊"使"乙""丙"从 y=10 移到 y=20/y=30 ---
        // old_layout（已在上方构造）："甲乙'丙'" 一行(y=0) + "乙丙" 一行(y=10)
        //   甲(0,0,20,10) byte(0,3), 乙'(20,0,20,10) byte(3,6), 丙'(40,0,20,10) byte(6,9)
        //   乙(0,10,20,10) byte(9,12), 丙(20,10,20,10) byte(12,15)
        // new_layout："戊甲乙'丙'" 一行(y=0) + "乙" 一行(y=20) + "丙" 一行(y=30)
        //   戊(0,0,20,10) byte(0,3), 甲(20,0,20,10) byte(3,6),
        //   乙'(40,0,20,10) byte(6,9), 丙'(60,0,20,10) byte(9,12)
        //   乙(0,20,20,10) byte(12,15), 丙(0,30,20,10) byte(15,18)
        //
        // "乙""丙"在不同新行 → merge_adjacent_shifts 不会合并它们，
        // 可以单独断言各自的 from。
        let new_layout = snapshot(vec![
            test_line(
                0,
                0,
                vec![
                    cluster(0, 3, 0.0, 20.0),
                    cluster(3, 6, 20.0, 20.0),
                    cluster(6, 9, 40.0, 20.0),
                    cluster(9, 12, 60.0, 20.0),
                ],
                0.0,
            ),
            test_line(1, 12, vec![cluster(12, 15, 0.0, 20.0)], 20.0),
            test_line(2, 15, vec![cluster(15, 18, 0.0, 20.0)], 30.0),
        ]);

        // 编辑：在 byte 0 插入"戊"（3 字节 UTF-8 汉字）。replace=(0,0), inserted=(0,3)
        // "戊"是 3 字节，new_head=3 是合法 UTF-8 边界（戊 后 甲 前），
        // EditorSelection::collapsed 不会夹偏移。旧测试用 ASCII "X"（1 字节）
        // 当 3 字节，偏移 3 落在首字内部，selection 被夹到非法边界外。
        let edit = make_edit("甲乙丙乙丙", "戊甲乙丙乙丙", 0, 3, (0, 0), (0, 3), "戊");
        let motion2 = retarget(&edit, Some(&old_layout), &new_layout, Some(&last_drawn), 7);

        // "乙"的 new byte_range=(12,15)，old_byte_range=(9,12)
        // "丙"的 new byte_range=(15,18)，old_byte_range=(12,15)
        let yi_shift = motion2
            .slices
            .iter()
            .find(|s| s.motion_kind() == MotionKind::Shift && s.byte_range() == (12, 15))
            .expect("应有乙 Shift (byte 12..15)");
        let bing_shift = motion2
            .slices
            .iter()
            .find(|s| s.motion_kind() == MotionKind::Shift && s.byte_range() == (15, 18))
            .expect("应有丙 Shift (byte 15..18)");

        match yi_shift {
            SliceMotion::Shift { from, to, new, .. } => {
                // from = 上一帧"乙"的插值位置：(2.5, 8.75, 20, 10)
                // 而非旧布局 canonical 终点 (0, 10, 20, 10)
                assert!(
                    (from.x - 2.5).abs() < 0.01,
                    "乙 from.x={} expected=2.5 (上一帧插值位置)",
                    from.x
                );
                assert!(
                    (from.y - 8.75).abs() < 0.01,
                    "乙 from.y={} expected=8.75 (上一帧插值位置)",
                    from.y
                );
                assert_eq!(from.w, 20.0);
                assert_eq!(from.h, 10.0);
                // to = 新布局位置
                assert_eq!(*to, rect(0.0, 20.0, 20.0, 10.0));
                // 源 UV 正确（"乙"的 new 版 UV）
                assert_eq!(new.source.source_uv, rect(0.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Shift for 乙, got {other:?}"),
        }

        match bing_shift {
            SliceMotion::Shift { from, to, new, .. } => {
                // from = 上一帧"丙"的插值位置：(22.5, 8.75, 20, 10)
                // 而非旧布局 canonical 终点 (20, 10, 20, 10)
                assert!(
                    (from.x - 22.5).abs() < 0.01,
                    "丙 from.x={} expected=22.5 (上一帧插值位置)",
                    from.x
                );
                assert!(
                    (from.y - 8.75).abs() < 0.01,
                    "丙 from.y={} expected=8.75 (上一帧插值位置)",
                    from.y
                );
                assert_eq!(from.w, 20.0);
                assert_eq!(from.h, 10.0);
                assert_eq!(*to, rect(0.0, 30.0, 20.0, 10.0));
                assert_eq!(new.source.source_uv, rect(0.0, 0.0, 20.0, 10.0));
            }
            other => panic!("expected Shift for 丙, got {other:?}"),
        }
    }

    /// Issue #919 复核 17：合并 Shift 中途被替换（shaping 变化）且跨行位移距离
    /// 大于字高时，退场 Conceal 的 target 与 clip 必须有非零交集。旧代码用
    /// `old_span.slice.rect`（canonical 目标位置）作 Conceal `old.rect`，与
    /// `start_clip`（插值位置）在 y 方向不相交，`scene_graph_renderer` 做交集
    /// 会跳过整个 glyph，旧字从未开始吞就直接消失。
    #[test]
    fn merged_shift_shaping_change_conceal_has_nonzero_target_clip_overlap() {
        // --- 先构造第二笔 old_layout，取"乙丙"行 id 作为第一笔合并 Shift 的源 ---
        let old_layout = snapshot(vec![
            test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0),
            test_line(
                1,
                3,
                vec![cluster_shaped(3, 6, 0.0, 20.0, 0), cluster(6, 9, 20.0, 20.0)],
                100.0,
            ),
        ]);
        let merged_snap_id = old_layout.line_snapshots[1].id;

        // --- 第一笔：合并 Shift 把"乙丙"从 y=0 移到 y=100（跨行位移）---
        let merged_uv = rect(0.0, 0.0, 40.0, 10.0);
        let snap = SnapshotRef {
            snapshot_id: merged_snap_id,
            source_uv: merged_uv,
        };
        let motion = TextMotion {
            slices: vec![SliceMotion::Shift {
                new: VisualSlice {
                    source: snap,
                    rect: rect(0.0, 100.0, 40.0, 10.0),
                },
                from: rect(20.0, 0.0, 40.0, 10.0),
                to: rect(0.0, 100.0, 40.0, 10.0),
                byte_range: (3, 9),
                shaping_identity: identity(),
            }],
            started_ms: 0.0,
            duration_ms: 100,
            session: 7,
            target_revision: 2,
            viewport: viewport(),
            static_exclusions: vec![],
            required_textures: vec![snap.snapshot_id],
            caret_from: None,
            caret_to: None,
            paragraph_moves: vec![],
        };

        // sample(50ms)：progress = ease_out_cubic(0.5) = 0.875
        // 合并插值 y = 0 + 100*0.875 = 87.5，canonical 目标 y = 100，相距 12.5 > 字高 10。
        let frame = motion.sample(50.0);
        let ov = &frame.overlays[0];
        let drawn = DrawnSlice {
            slice: VisualSlice {
                source: SnapshotRef {
                    snapshot_id: ov.snapshot_id,
                    source_uv: ov.source_rect,
                },
                rect: rect(ov.x, ov.y, ov.w, ov.h),
            },
            visible_width: ov.clip_rect.w,
            clip_rect: ov.clip_rect,
            mask_clip: ov.mask_clip,
            motion_kind: ov.motion_kind,
            byte_range: ov.byte_range,
            shaping_identity: ov.shaping_identity.clone(),
            direction: ov.direction.unwrap_or(ClipDirection::LeftToRight),
        };
        let last_drawn = LocalDrawnFrame {
            session: 7,
            revision: 2,
            slices: vec![drawn],
        };

        // --- 第二笔：在"甲"后插入"戊"，"乙"shaping 变化（glyph_hash 0→999）---
        // old_layout："甲"一行(y=0) + "乙丙"一行(y=100)
        //   甲(0,0,20,10) byte(0,3)
        //   乙(0,100,20,10) byte(3,6) shaping hash=0, 丙(20,100,20,10) byte(6,9)
        // new_layout："甲戊"一行(y=0) + "乙丙"一行(y=110)
        //   甲(0,0,20,10) byte(0,3), 戊(20,0,20,10) byte(3,6)
        //   乙(0,110,20,10) byte(6,9) shaping hash=999, 丙(20,110,20,10) byte(9,12)
        let new_layout = snapshot(vec![
            test_line(
                0,
                0,
                vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
                0.0,
            ),
            test_line(
                1,
                6,
                vec![cluster_shaped(6, 9, 0.0, 20.0, 999), cluster(9, 12, 20.0, 20.0)],
                110.0,
            ),
        ]);

        // 编辑：在 byte 3 插入"戊"（3 字节）。replace=(3,3), inserted=(3,6)
        let edit = make_edit("甲乙丙", "甲戊乙丙", 3, 6, (3, 3), (3, 6), "戊");
        let motion2 = retarget(&edit, Some(&old_layout), &new_layout, Some(&last_drawn), 7);

        // "乙"new byte_range=(6,9)，old_byte_range=(3,6)，在合并 Shift (3,9) 内。
        // shaping 变化 → Conceal + Reveal。Conceal 应用 shift_from（插值位置）。
        let conceal_yi = motion2
            .slices
            .iter()
            .find(|s| s.byte_range() == (3, 6) && s.motion_kind() == MotionKind::Conceal)
            .expect("应有旧\"乙\" Conceal (byte 3..6)");

        match conceal_yi {
            SliceMotion::Conceal { old, start_clip, .. } => {
                // old.rect = shift_from = 上一帧"乙"的插值位置 (2.5, 87.5, 20, 10)
                // 而非 canonical 目标 (0, 100, 20, 10)
                assert!(
                    (old.rect.y - 87.5).abs() < 0.01,
                    "old.rect.y={} expected=87.5 (上一帧插值位置)",
                    old.rect.y
                );
                assert!(
                    (old.rect.x - 2.5).abs() < 0.01,
                    "old.rect.x={} expected=2.5 (上一帧插值位置)",
                    old.rect.x
                );
                assert_eq!(old.rect.w, 20.0);
                assert_eq!(old.rect.h, 10.0);
                // start_clip = shift_from，与 old.rect 对齐
                assert_eq!(*start_clip, old.rect);
                // canonical old_span.slice.rect.y = 100，相距 12.5 > 字高 10
                assert!((100.0 - old.rect.y).abs() > 10.0);
            }
            other => panic!("expected Conceal, got {other:?}"),
        }

        // sample(0)：退场 overlay 的 target 与 clip 应有非零交集。
        // 旧代码 target=canonical(0,100), clip=shift_from(2.5,87.5) → y 不相交 → 空交集。
        let frame2 = motion2.sample(0.0);
        let conceal_overlay = frame2
            .overlays
            .iter()
            .find(|o| o.motion_kind == MotionKind::Conceal && o.byte_range == (3, 6))
            .expect("应有 Conceal overlay");
        let target = rect(conceal_overlay.x, conceal_overlay.y, conceal_overlay.w, conceal_overlay.h);
        let overlap = intersect_rect(&target, &conceal_overlay.clip_rect);
        assert!(
            overlap.w > 0.01 && overlap.h > 0.01,
            "target {:?} 与 clip {:?} 交集为 {:?}，应有非零交集",
            target,
            conceal_overlay.clip_rect,
            overlap
        );

        // sample(50ms)：中间帧仍应有非零交集（遮罩从 shift_from 收拢，target 固定）
        let frame2_mid = motion2.sample(50.0);
        let conceal_overlay_mid = frame2_mid
            .overlays
            .iter()
            .find(|o| o.motion_kind == MotionKind::Conceal && o.byte_range == (3, 6))
            .expect("应有中间帧 Conceal overlay");
        let target_mid = rect(
            conceal_overlay_mid.x,
            conceal_overlay_mid.y,
            conceal_overlay_mid.w,
            conceal_overlay_mid.h,
        );
        let overlap_mid = intersect_rect(&target_mid, &conceal_overlay_mid.clip_rect);
        assert!(
            overlap_mid.w > 0.01 && overlap_mid.h > 0.01,
            "中间帧 target {:?} 与 clip {:?} 交集为 {:?}，应有非零交集",
            target_mid,
            conceal_overlay_mid.clip_rect,
            overlap_mid
        );
    }
}
