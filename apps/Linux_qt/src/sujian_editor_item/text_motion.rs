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
            let old_byte_range = (
                new_to_old_byte(edit, new_span.byte_range.0),
                new_to_old_byte(edit, new_span.byte_range.1),
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
            let last_shift = last_vec
                .and_then(|v| v.iter().find(|ds| ds.motion_kind == MotionKind::Shift).copied());
            if let Some(last) = last_shift {
                // 上一笔在位移，本笔从当前已插值显示的 rect 接新目标。
                if let Some((idx, _)) = old_spans
                    .iter()
                    .enumerate()
                    .find(|(i, s)| !consumed_old[*i] && s.byte_range == old_byte_range)
                {
                    consumed_old[idx] = true;
                    let old_span = &old_spans[idx];
                    // Issue #919 复核 3：shaping 不同则旧退出+新进入。
                    if old_span
                        .shaping_identity
                        .is_same_shaping(&new_span.shaping_identity)
                    {
                        let from = last.clip_rect;
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
                        // Issue #919 复核 7：Shift 中途被替换（shaping 变化）时，
                        // 退场位置取上一帧真正显示的 VisualSlice（last.slice，含
                        // 当时实际位置和源纹理），start_clip = last.mask_clip（上一
                        // 帧实际遮罩），而不是 old_span.slice.rect（上一笔编辑已
                        // 排版完成的 canonical 目标位置）。正在 Shift 的字当前画在
                        // last.slice.rect / last.mask_clip 的中途位置，从该位置以
                        // 本次编辑遮罩方向生成旧图 Conceal，避免从 canonical 目标
                        // 位置突然开始退场而向目标方向闪一下。旧图 SnapshotRef 已
                        // 存在于 last，登记到 claimed_conceal_sources 保证第二遍
                        // 不重复退场；新字仍从 new_span.slice 做 Reveal。
                        claimed_conceal_sources.push(last.slice.source);
                        slices.push(SliceMotion::Conceal {
                            old: last.slice,
                            direction,
                            start_clip: last.mask_clip,
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
    use crate::editor::layout::CaretAffinity;
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
        EditorLayoutSnapshot {
            revision: LayoutRevision::next(),
            line_snapshots: lines,
            paragraph_height_index: None,
            paragraph_slot_map: None,
            paragraph_padding: 0.0,
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: CaretAffinity::Upstream,
        }
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
    fn same_byte_range_different_shaping_retires_old_and_reveals_new() {
        let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
        let line_id = new_layout.line_snapshots[0].id;
        // 上一帧露的是别的字形（glyph hash 不同），即使 byte_range 相同也不能当同一个字。
        let old_shaping = ShapingIdentity {
            glyph_indexes_hash: 42,
            ..ShapingIdentity::default()
        };
        let last = drawn_reveal(line_id, rect(0.0, 0.0, 10.0, 10.0), old_shaping);
        let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
        let motion = retarget(&edit, None, &new_layout, Some(&last), 7);

        assert_eq!(motion.slices.len(), 2);
        match &motion.slices[0] {
            SliceMotion::Conceal {
                start_clip,
                direction,
                ..
            } => {
                // 旧图从当前可见遮罩继续退场，沿用原方向。
                assert_eq!(*start_clip, rect(0.0, 0.0, 10.0, 10.0));
                assert_eq!(*direction, ClipDirection::LeftToRight);
            }
            other => panic!("expected old Conceal first, got {other:?}"),
        }
        match &motion.slices[1] {
            SliceMotion::Reveal { start_clip, .. } => {
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
}
