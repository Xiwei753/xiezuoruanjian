use super::qt_text_node;
use super::render_plan::{RenderPlan, TextAnimationGlyphInfo};
use super::texture_cache::TextureCache;
use super::visual_canvas::{CanvasDrawPlan, DrawCommandKind};
use crate::editor::layout::{LayoutSnapshot, ParagraphHeightIndex, ParagraphId};
use crate::editor::scene_graph;
use writer_core::editor::EditorRevision;

/// Issue #901 第十轮问题1：本帧真正完成绘制的动画 fragment。
///
/// `render_frame` 在 `render_text_animation_layer` 实际 push 到 glyph_data
/// 后才产生此记录。调用方据此构造 `PresentedScene`，不再无条件把原始
/// `canvas_plan.commands` 中每条 TextFragment 作为"已经显示"记录。
///
/// 资源缺失或 handoff 时渲染器传 `glyphs=&[]`，此列表为空——只有静态 owner
/// 画的片段进入 `drawn_static_fragments`。
#[derive(Clone, Debug)]
pub(crate) struct DrawnAnimationFragment {
    /// 源纹理的完整行快照 id。
    pub snapshot_id: super::layout_snapshot::LineSnapshotId,
    /// 实际绘制的目标矩形（已与 clip_rect 求交，文档坐标）。
    pub target_rect: super::layout_snapshot::SourceRect,
    /// 实际绘制的 source UV（已按裁剪比例调整，纹理局部坐标）。
    pub source_uv: super::layout_snapshot::SourceRect,
    /// 该 fragment 的透明度。
    pub opacity: f64,
    /// 该 fragment 对应的 Core 逻辑 UTF-8 byte range。
    pub byte_range: (usize, usize),
    /// 该 fragment 对应的 Core source revision。
    pub source_revision: EditorRevision,
    /// Issue #901 第十轮问题2：字形逻辑身份。
    pub glyph_id: u64,
    /// Issue #901 第十轮问题2：同一字形内的碎片序号。
    pub fragment_index: u32,
    /// Issue #901 第十一轮问题4：原始完整 glyph rectangle（未被裁剪的完整字形矩形）。
    ///
    /// 从 `TextAnimationGlyphInfo.original_glyph_rect` 传递，最终填入
    /// `PixelContribution.original_glyph_rect`，供下一帧 FLIP 变换使用。
    pub original_glyph_rect: super::layout_snapshot::SourceRect,
}

/// Issue #901 第十轮问题1：静态 owner 本帧实际可见的片段。
///
/// 静态 QSGTextNode 在 `clip_rects` 挖空后实际绘制的 cluster 片段。
/// 调用方据此构造 `RenderPlan.static_text_contributions`，不再用前置的
/// `animation_resources_ready_for_static` 平行推测。
///
/// `animation_resources_ready=false` 时（渲染器画完整 canonical 静态层、
/// 清空动画层），此列表包含所有视口内 cluster 的完整可见片段；
/// `animation_resources_ready=true` 时，每个 cluster 已减去 effective_holes，
/// 可能产生多个碎片（同一 cluster 共享 glyph_id，不同 fragment_index）。
#[derive(Clone, Debug)]
pub(crate) struct DrawnStaticFragment {
    /// 源纹理的完整行快照 id。
    pub snapshot_id: super::layout_snapshot::LineSnapshotId,
    /// 该片段对应的 Core 逻辑 UTF-8 byte range（cluster 级）。
    pub byte_range: (usize, usize),
    /// 文档坐标系中的目标矩形（不含滚动偏移）。
    pub document_rect: super::layout_snapshot::SourceRect,
    /// 实际可见的裁剪区域（文档坐标，已与视口求交、已减去 effective_holes）。
    pub visible_clip: super::layout_snapshot::SourceRect,
    /// 源纹理中的 UV 区域（行纹理局部坐标，已乘 DPR）。
    pub source_uv: super::layout_snapshot::SourceRect,
    /// 该片段对应的 Core source revision（绑定 target revision）。
    pub source_revision: EditorRevision,
    /// Issue #901 第十轮问题2：字形逻辑身份。
    pub glyph_id: u64,
    /// Issue #901 第十轮问题2：同一字形内的碎片序号。
    pub fragment_index: u32,
    /// Issue #901 第十一轮问题4：原始完整 glyph rectangle（未被裁剪的完整字形矩形）。
    ///
    /// 等于 `StaticTextContributionInfo.document_rect`（cluster 的完整字形几何，
    /// 未被视口/holes 裁剪）。`build_presented_scene_from_frame` 据此填入
    /// `PixelContribution.original_glyph_rect`。
    pub original_glyph_rect: super::layout_snapshot::SourceRect,
}

/// Issue #901 第九轮阻塞2：`render_frame` 的渲染结果。
///
/// 渲染器返回本帧实际采用的静态层裁剪矩形（`effective_clip_rects`），
/// 供 `qquickitem_impl.rs` 据此计算真实静态贡献。当
/// `animation_resources_ready=false` 时，渲染器选择画完整 canonical 静态层、
/// 清空动画层，`effective_clip_rects` 为空——所有 cluster 都由静态层绘制。
/// `qquickitem_impl.rs` 用 `effective_clip_rects` 的矩形并集求补集来决定
/// 每个 cluster 的真实可见静态片段，不再用"单一矩形是否完全包含"代替。
///
/// Issue #901 第十轮问题1：新增 `drawn_animation_fragments` 和
/// `drawn_static_fragments`，让调用方据此构造 `PresentedScene`，彻底删掉
/// 前置的 `animation_resources_ready_for_static` 平行推测逻辑。
#[derive(Clone, Debug)]
pub(crate) struct FrameRenderOutcome {
    /// 静态层 rebuild 是否成功。
    pub static_rebuild_ok: bool,
    /// 本帧静态层实际采用的裁剪矩形（经过 `animation_resources_ready` 过滤）。
    /// `animation_resources_ready=false` 时为空（静态层画完整 canonical）。
    pub effective_clip_rects: Vec<qt_text_node::AnimationClipRect>,
    /// Issue #901 第九轮阻塞4: 本帧 rebuild 成功后实际加载的段落索引范围。
    /// 调用方将其保存为 per-item state，下一帧用此范围判断视口是否跨出
    /// 已加载集合。`None` 表示本帧未 rebuild 或 rebuild 失败，保留旧范围。
    pub new_loaded_para_range: Option<(usize, usize)>,
    /// Issue #901 第十轮问题1：本帧真正完成绘制的动画 fragment 列表。
    ///
    /// 只包含 `render_text_animation_layer` 实际 push 到 glyph_data 的 fragment
    ///（纹理已就绪、裁剪后非零面积）。资源缺失或 handoff 时为空。
    pub drawn_animation_fragments: Vec<DrawnAnimationFragment>,
    /// Issue #901 第十轮问题1：静态 owner 本帧实际可见的片段列表。
    ///
    /// 每个 cluster 已减去 `effective_clip_rects` 的矩形并集，可能产生多个
    /// 碎片（同一 cluster 共享 glyph_id，不同 fragment_index）。
    /// `static_rebuild_ok=false` 时为空（调用方应保留旧 PresentedScene）。
    pub drawn_static_fragments: Vec<DrawnStaticFragment>,
}

impl FrameRenderOutcome {
    /// 失败时的默认结果：rebuild 失败，无有效裁剪，无绘制片段。
    fn failed() -> Self {
        Self {
            static_rebuild_ok: false,
            effective_clip_rects: Vec::new(),
            new_loaded_para_range: None,
            drawn_animation_fragments: Vec::new(),
            drawn_static_fragments: Vec::new(),
        }
    }
}

/// Issue #901 第九轮阻塞4: per-item 已加载段落索引区间。
///
/// 替代原全局 `static LOADED_PARA_RANGE`。`render_frame` 每次 rebuild 成功后
/// 把本帧视口（±一屏边距）对应的段落索引范围 `[start, end)` 通过
/// `FrameRenderOutcome::new_loaded_para_range` 返回给调用方。调用方将其保存为
/// per-item state，下一帧传入 `StaticTextParams::loaded_para_range`。
///
/// 纯滚动帧用此范围判断视口是否跨出已加载集合
/// （`new_start < loaded_start || new_end > loaded_end`），跨出则触发 rebuild，
/// 否则只更新 `QSGTransformNode` 位移矩阵。

/// Issue #901 阻塞 1：从 `CanvasDrawPlan` 的 `TextFragment` 命令转换为
/// `TextAnimationGlyphInfo` 列表，供动画层消费。
///
/// Issue #901 问题4：不再按 `opacity < 1.0` 过滤。存活字 FLIP 的 opacity=1
/// 也要被绘制。按 `DrawCommandKind::TextFragment` 和真正 owner/几何变化来绘制，
/// 不根据 alpha<1 决定"是否属于动画"。
///
/// Issue #901 问题2：`DrawCommand.target_rect` 和 `clip_rect` 始终使用文档坐标。
/// C++ 动画层（`AnimationLayerNode`）统一施加一次 `translate(0, -scroll_y)`
/// 变换（通过 `update_animation_layer` 的 `scroll_y` 参数）。
///
/// Issue #901 问题3：动画层的动态 clip 按 `progress` 更新（通过
/// `DrawCommand.target_rect` 和 `DrawCommand.clip_rect`），与静态层排除区
/// （`static_exclusion_rects`）是不同的两个概念。
pub(crate) fn canvas_draw_plan_to_glyph_infos(
    canvas_plan: &CanvasDrawPlan,
    _texture_cache: &TextureCache,
) -> Vec<TextAnimationGlyphInfo> {
    use super::layout_snapshot::{ShapingIdentity, SourceRect};
    use super::edit_motion::DeleteEdge;

    canvas_plan
        .commands
        .iter()
        .filter(|cmd| cmd.kind == DrawCommandKind::TextFragment && cmd.opacity > 0.0)
        .map(|cmd| {
            // Issue #901 问题2：直接使用 DrawCommand 携带的完整 LineSnapshotId，
            // 不再从 u64 hash 反解。动画层通过完整 LineSnapshotId 查询 texture_cache。
            let snapshot_id = cmd.snapshot_id;
            TextAnimationGlyphInfo {
                x: cmd.target_rect.x,
                y: cmd.target_rect.y,
                w: cmd.target_rect.w,
                h: cmd.target_rect.h,
                opacity: cmd.opacity,
                snapshot_id,
                source_rect: SourceRect {
                    x: cmd.source_uv.x,
                    y: cmd.source_uv.y,
                    w: cmd.source_uv.w,
                    h: cmd.source_uv.h,
                },
                // Issue #901 问题4：把 DrawCommand.clip_rect 传给动画 glyph，
                // 让 render_text_animation_layer 对 target_rect 和 source_uv
                // 做预裁剪，使吞字/吐字的 reveal/conceal 裁剪真正作用到 Qt 图形层。
                clip_rect: SourceRect {
                    x: cmd.clip_rect.x,
                    y: cmd.clip_rect.y,
                    w: cmd.clip_rect.w,
                    h: cmd.clip_rect.h,
                },
                // Issue #901 第十一轮问题1：从 DrawCommand 原样复制 byte_range，
                // 不再硬编码 (0, 0)。render_text_animation_layer 据此填写
                // DrawnAnimationFragment.byte_range，build_presented_scene_from_frame
                // 据此填写 PixelContribution.byte_range。
                logical_range: cmd.byte_range,
                canonical_range: Some(cmd.byte_range),
                delete_edge: None,
                shaping_identity: ShapingIdentity {
                    text_content_hash: 0,
                    raw_font_fingerprint: String::new(),
                    glyph_indexes_hash: 0,
                    cluster_glyph_count: 0,
                    direction_rtl: false,
                    format_fingerprint: 0,
                },
                // Issue #901 第十一轮问题1：从 DrawCommand 原样复制 source_revision，
                // 不再用 EditorRevision::initial() 占位。
                source_revision: cmd.source_revision,
                // Issue #901 第十一轮问题1：从 (snapshot_id, byte_range) 派生 glyph_id。
                glyph_id: super::visual_canvas::PixelContribution::derive_glyph_id(
                    &cmd.snapshot_id,
                    cmd.byte_range,
                ),
                // fragment_index 暂用 0（DrawCommand 不携带 fragment 级序号）。
                fragment_index: 0,
                // Issue #901 第十一轮问题4：从 DrawCommand 传递完整字形矩形。
                original_glyph_rect: SourceRect {
                    x: cmd.original_glyph_rect.x,
                    y: cmd.original_glyph_rect.y,
                    w: cmd.original_glyph_rect.w,
                    h: cmd.original_glyph_rect.h,
                },
            }
        })
        .collect()
}

/// Issue #901 问题3：从 `CanvasDrawPlan` 的 `static_exclusion_rects` 推导
/// 静态层裁剪矩形。
///
/// 静态层的 **target 字形排除区**覆盖本次由动画画布接管的**完整目标范围**，
/// 整个过渡期间与 `progress` 无关。只在接管集合改变时重建静态 QSGTextNode，
/// 不在每帧改变挖空矩形。
///
/// 动画层的动态 clip/UV 才按 `progress` 更新（在 `canvas_draw_plan_to_glyph_infos`
/// 中用 `DrawCommand.target_rect` 和 `DrawCommand.clip_rect`）。
///
/// Issue #901 问题2：`static_exclusion_rects` 使用文档坐标，与 `AnimationClipRect`
/// 的文档坐标契约一致。C++ 静态层（`QSGTransformNode`）统一施加一次
/// `translate(0, -scroll_y)` 变换。
pub(crate) fn canvas_draw_plan_to_clip_rects(
    canvas_plan: &CanvasDrawPlan,
) -> Vec<qt_text_node::AnimationClipRect> {
    canvas_plan
        .static_exclusion_rects
        .iter()
        .map(|excl| qt_text_node::AnimationClipRect {
            x: excl.rect.x,
            y: excl.rect.y,
            w: excl.rect.w,
            h: excl.rect.h,
            // Issue #901 问题2：直接使用完整 LineSnapshotId。
            snapshot_id: excl.snapshot_id,
        })
        .collect()
}

/// 静态正文层的渲染参数 — 交给 QSGTextNode（Qt 6.7+ 公开 API）。
///
/// `layout_snapshot` 提供已排好的 VisualLine 数据（唯一 canonical 排版源）；
/// `needs_relayout` 控制是否重新排版（正文/字体/宽度变更时为 true，滚动时为 false）。
///
/// Issue #901 第六轮阻塞5: `height_index` 提供段落高度索引，用于 O(log n) 定位
/// 视口内首末可见段落，避免遍历全章 lines。`None` 表示无可用索引（fallback 到
/// lines 的 y 二分查找，仍为 O(log n)）。
pub(crate) struct StaticTextParams<'a> {
    pub layout_snapshot: Option<&'a LayoutSnapshot>,
    pub height_index: Option<&'a ParagraphHeightIndex>,
    pub scroll_y: f64,
    pub viewport_height: f64,
    pub color: &'a str,
    pub needs_relayout: bool,
    /// Issue #901 第九轮阻塞4: 上次 rebuild 成功后加载的段落索引范围。
    /// `None` 表示尚未加载任何段落（首次渲染或上次 rebuild 失败）。
    pub loaded_para_range: Option<(usize, usize)>,
    /// Issue #901 第九轮阻塞4: 稳定段落身份列表，按段落索引顺序排列。
    /// `paragraph_ids[i]` 是第 i 个段落的 `ParagraphId`。
    /// 用于在 `ParagraphLineInfo` 中填入稳定身份标识。
    pub paragraph_ids: Option<&'a [ParagraphId]>,
}

/// Issue #901 第七轮复核问题2: 计算段落真实第一行的文档 y 坐标。
///
/// `QSGTextNode::addTextLayout(QPointF(origin_x, y), cachedLayout)` 消费完整段落的
/// QTextLayout，布局内每条 QTextLine 已相对段落第一行定位。视口裁剪只决定哪些
/// QTextLine 参与绘制，绝不能用"第一条可见行"的 y 冒充整个段落的起点。
///
/// # 实现策略
///
/// - 有 `ParagraphHeightIndex` 且 `cache_slot >= 0`：用 `paragraph_top_y(cache_slot)`
///   （O(log n) 前缀和）。`cache_slot` 与 heights 数组索引一致：
///   `extract_paragraph_heights` 按 `para_start` 首次出现顺序（= 文档段落顺序）构建，
///   `full.rs` 按 `paragraph_idx` 递增分配 `cache_slot`（含空段落），两者一致。
///   `paragraph_top_y` 不含 padding，需补 `snapshot.padding`（`VisualLine.y` 从
///   padding 起累加，见 `full.rs`）。
/// - 无 height_index 或 `cache_slot < 0`：fallback 到 lines 二分查找段落第一行。
///   lines 按 y 升序且同段落行连续（文档顺序），`partition_point(|l| l.para_start < ps)`
///   返回首个 `para_start >= ps` 的行 = 该段落第一行，其 y 已含 padding。
///
/// 不用 `unwrap`/`expect`：所有越界路径回退到 `line.y`，保证不 panic。
fn paragraph_top_document_y(
    line: &crate::editor::layout::VisualLine,
    snapshot: &LayoutSnapshot,
    height_index: Option<&ParagraphHeightIndex>,
) -> f64 {
    if let Some(hi) = height_index {
        if line.cache_slot >= 0 {
            // paragraph_top_y 越界返回总高度（不 panic），加 padding 仍是合法文档 y。
            return hi.paragraph_top_y(line.cache_slot as usize) + f64::from(snapshot.padding);
        }
    }
    // Fallback: lines 二分查找段落第一行。谓词 `para_start < ps` 为 true 前缀
    // （更早段落）、false 后缀（本段及之后），返回首个 false = 本段第一行索引。
    // Issue #901 第十轮问题5: 用 resolved_lines 按需物化后缀段绝对坐标。
    let resolved_lines = snapshot.resolved_lines();
    let lines_slice = resolved_lines.as_ref();
    let first_idx = lines_slice
        .partition_point(|l| l.para_start < line.para_start);
    if first_idx < lines_slice.len() {
        lines_slice[first_idx].y
    } else {
        // 防御性回退：line 自身应在 lines 中，first_idx 应 < len；
        // 仅在 lines 为空或异常时回退到 line.y，避免越界。
        line.y
    }
}

/// Issue #901 第八轮问题3：计算当前视口（±一屏边距）对应的稳定段落索引范围。
///
/// 返回 `Some((start, end))` 表示段落 `[start, end)` 应被加载到 QSG 静态层；
/// 返回 `None` 表示无可用 `height_index` 或 `layout_snapshot`，调用方应跳过
/// 跨出检测（退化为旧行为）。
///
/// 视口边距与 `render_frame` rebuild 分支保持一致：`margin = viewport_height`，
/// `visible_top = scroll_y - margin`，`visible_bottom = scroll_y + viewport_height + margin`。
/// 这样"已加载范围"与 rebuild 实际建立的段落集合一致，跨出判断才准确。
///
/// 用 `ParagraphHeightIndex::paragraphs_in_y_range` 做 O(log n) 段落枚举，
/// 不遍历全章 lines。
fn compute_viewport_para_range(static_text: &StaticTextParams<'_>) -> Option<(usize, usize)> {
    let hi = static_text.height_index?;
    // layout_snapshot 不参与段落枚举（paragraphs_in_y_range 只用 height_index），
    // 但调用方通常同时提供两者。此处不强制要求 layout_snapshot。
    let viewport_top = static_text.scroll_y;
    let viewport_bottom = static_text.scroll_y + static_text.viewport_height;
    let margin = static_text.viewport_height.max(100.0);
    let visible_top = viewport_top - margin;
    let visible_bottom = viewport_bottom + margin;
    let (first_para, last_para_excl) = hi.paragraphs_in_y_range(visible_top, visible_bottom);
    Some((first_para, last_para_excl))
}

pub(crate) fn render_frame(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    static_text: &StaticTextParams<'_>,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
) -> FrameRenderOutcome {
    if root_raw.is_null() || item_ptr.is_null() {
        return FrameRenderOutcome::failed();
    }

    // Issue #668 评论 5646458592 问题 1: 静态正文 rebuild 的成功/失败结果。
    // 默认 true 表示无需 rebuild（如滚动帧）或 rebuild 成功；
    // false 表示 rebuild 因 layout 缺失而放弃，调用方应保留 dirty 标志。
    let mut static_rebuild_ok = true;
    // Issue #901 第九轮阻塞4: 本帧 rebuild 成功后的新加载段落范围，
    // 通过 FrameRenderOutcome 返回给调用方。
    let mut rebuild_new_range: Option<(usize, usize)> = None;

    // Layer 0: 静态正文 — QSGTextNode (Qt 6.7+ public API)
    // 消费 EditorLayout 唯一 canonical 排版结果，不再自行创建第二套 QTextLayout。
    //
    // Issue #714 评论 5740007764: 静态层在 needs_relayout=true 时重建。
    // needs_relayout 由 layout_dirty || scene_dirty 驱动，覆盖：
    //   - 正文/layout/颜色变化（layout_dirty=true，GUI 线程 request_static_repaint）
    //   - 正文动画开始/结束（scene_dirty=true）
    //
    // Issue #853：`clip_rects` 是动画层接管的完整 target cluster exclusion，
    // 在过渡期间不随 progress 改变。静态层只在所有权集合改变时重建；
    // glyph 切片由动画层逐帧更新。
    // ownership 不变的帧（纯滚动、纯光标动画、正文 progress）只更新轻量节点。
    //
    // `clip_rects` 表达静态层让给动画 owner 的目标 cluster。静态正文仍来自最新
    // canonical layout；动画层在这些位置绘制唯一的当前视觉。
    //
    // 先检查整份 ownership plan 的动画资源。缺一张就按 canonical 静态正文提交，
    // 不能只过滤静态 clip、再让动画层单独跳过缺纹理的 glyph。
    let animation_resources_ready = plan.ownership.has_animation_resources(texture_cache);

    // Issue #901 第九轮阻塞2：计算本帧静态层实际采用的裁剪矩形。
    // 当 animation_resources_ready=false 时，渲染器选择画完整 canonical 静态层、
    // 清空动画层，effective_clip_rects 为空——所有 cluster 都由静态层绘制。
    // qquickitem_impl.rs 据此用矩形差集计算每个 cluster 的真实可见静态片段。
    let effective_clip_rects: Vec<qt_text_node::AnimationClipRect> = if animation_resources_ready {
        plan.ownership.clips_for_static_rebuild().to_vec()
    } else {
        Vec::new()
    };

    // Issue #901 第八轮问题3：计算新视口对应的段落范围，与已加载范围比较。
    // 跨出已加载集合时触发 rebuild（insert 新段落节点 + remove 离开段落节点），
    // 不只平移旧一屏的 transform。这修复长距离滚动后新区域空白的问题。
    let new_para_range = compute_viewport_para_range(static_text);
    // Issue #901 第九轮阻塞4: 使用 per-item state 而非全局 static。
    let loaded_para_range = static_text.loaded_para_range.unwrap_or((0, 0));
    // 视口跨出已加载段落集合：新范围的下界 < 已加载下界，或新范围上界 > 已加载上界。
    // 无 new_para_range（无 height_index）时不触发跨出检测，退化为旧行为。
    let viewport_exceeds_loaded = match new_para_range {
        Some((new_start, new_end)) => {
            new_start < loaded_para_range.0 || new_end > loaded_para_range.1
        }
        None => false,
    };

    let should_rebuild_static = static_text.needs_relayout
        || plan.ownership.handoff_pending
        || !animation_resources_ready
        || viewport_exceeds_loaded;

    if should_rebuild_static {
        // 正文/layout/颜色变化 或 活动事务集合变化：重建静态节点（含裁剪）
        if let Some(snapshot) = static_text.layout_snapshot {
            // Issue #901 第十轮问题5: 用 resolved_lines 按需物化后缀段绝对坐标。
            let resolved_lines = snapshot.resolved_lines();
            let lines_slice = resolved_lines.as_ref();
            // Issue #901 第五轮阻塞5：只遍历视口内的行，不全文重建。
            // 长章中只有视口附近的行需要处理，成本与视口大小相关，
            // 而不是和全章长度相关。扩展一个视口高度的 margin 避免滚动时
            // 频繁重建边界段落。
            let viewport_top = static_text.scroll_y;
            let viewport_bottom = static_text.scroll_y + static_text.viewport_height;
            let margin = static_text.viewport_height.max(100.0);
            let visible_top = viewport_top - margin;
            let visible_bottom = viewport_bottom + margin;

            // Issue #901 第六轮阻塞5: 用 height_index O(log n) 定位视口内首末可见段落，
            // 再用段落 y 范围在 lines 中二分查找定位行索引范围 [start_idx, end_idx)。
            // 这样只遍历视口附近的行，不三遍扫描全章 lines。
            // lines 按 y 升序排列（文档顺序，y 递增），可安全用 partition_point 二分查找。
            let (start_idx, end_idx) =
                if let Some(hi) = static_text.height_index {
                    // 用 height_index 的段落级视口枚举（O(log n)）得到首末段落索引。
                    let (first_para, last_para_excl) =
                        hi.paragraphs_in_y_range(visible_top, visible_bottom);
                    if first_para >= last_para_excl {
                        (0usize, 0usize)
                    } else {
                        // 用段落顶/底 y 在 lines 中二分查找定位行范围。
                        // 段落粒度比行粗，首尾段落可能包含视口外的行，
                        // 后续的逐行视口检查会跳过它们。
                        let first_para_top = hi.paragraph_top_y(first_para);
                        let last_para_bottom =
                            hi.paragraph_bottom_y(last_para_excl - 1);
                        let s = lines_slice
                            .partition_point(|l| l.y + l.height < first_para_top);
                        // Issue #901 第七轮复核问题1: partition_point 要求
                        // true 前缀、false 后缀。lines 按 y 升序，`y > bottom`
                        // 是 false 前缀、true 后缀，方向相反，会使 end_idx 错误
                        // （轻则空白，重则 start>end 切片 panic）。正确谓词
                        // `y <= bottom`：true 前缀，返回首个超过 bottom 的行索引。
                        let e = lines_slice
                            .partition_point(|l| l.y <= last_para_bottom);
                        // 防御性 clamp：保证 start_idx <= end_idx <= lines.len()，
                        // 越界时退化为空区间而非 panic。
                        (s, e.max(s))
                    }
                } else {
                    // 无 height_index：直接用 lines 的 y 二分查找定位视口行范围。
                    // 仍是 O(log n)，不遍历全章。
                    let s = lines_slice
                        .partition_point(|l| l.y + l.height < visible_top);
                    // Issue #901 第七轮复核问题1: 同上，谓词方向修正为 `y <= bottom`。
                    let e = lines_slice
                        .partition_point(|l| l.y <= visible_bottom);
                    (s, e.max(s))
                };

            // Issue #901 第六轮阻塞5: 合并原三遍 for 循环为一遍。
            // 原实现分别遍历 lines 构建 paragraphs、para_y_map、visual_line_clips，
            // 每遍都是 O(全章行数)。现在只遍历 [start_idx, end_idx) 一次，
            // 同时构建三个集合，成本 O(视口内行数)。
            //
            // Issue #658: 按段落分组 VisualLine，每个段落对应一个 cache_idx。
            // cache_idx 直接从 VisualLine.cache_slot 读取（由 layout 阶段按段落出现顺序
            // 分配的稳定 slot），不再自行计数。
            let mut paragraphs: Vec<qt_text_node::ParagraphLineInfo> = Vec::new();
            let mut visual_line_clips: Vec<qt_text_node::VisualLineClipInfo> = Vec::new();
            // 段落第一行 y 的映射：para_start -> para_y
            let mut para_y_map: std::collections::HashMap<usize, f64> =
                std::collections::HashMap::new();
            let mut last_para_start: Option<usize> = None;
            // Issue #901 第十轮问题4: 维护当前段落的 ParagraphId，
            // 供 visual_line_clips 填入 paragraph_id 字段。
            let mut current_paragraph_id: u64 = 0;

            for line in &lines_slice[start_idx..end_idx] {
                // 跳过完全在扩展视口外的行（首尾段落可能含视口外行）。
                if line.y + line.height < visible_top || line.y > visible_bottom {
                    continue;
                }

                // Issue #901 第七轮复核问题2: para_y 必须是段落真实第一行文档 y，
                // 不能用"第一条可见行"的 y 冒充。QSGTextNode::addTextLayout 消费
                // 完整段落的 QTextLayout，布局内每条 QTextLine 已相对段落第一行定位；
                // 若视口从段落中间开始，用可见首行 y 会使前面被裁掉行的本地偏移
                // 重复累加，导致文字与命中/光标错位。同段落只计算一次并缓存。
                let para_y = *para_y_map
                    .entry(line.para_start)
                    .or_insert_with(|| {
                        paragraph_top_document_y(line, snapshot, static_text.height_index)
                    });

                if last_para_start != Some(line.para_start) {
                    // 新段落开始
                    last_para_start = Some(line.para_start);
                    // Issue #901 第九轮阻塞4: 用稳定 ParagraphId 作为段落身份标识。
                    // cache_slot 是段落索引，通过 paragraph_ids 查表得到 ParagraphId。
                    // 无 paragraph_ids 时用 0（ParagraphId::INVALID），不影响渲染正确性。
                    let paragraph_id = static_text
                        .paragraph_ids
                        .and_then(|ids| {
                            if line.cache_slot >= 0 {
                                ids.get(line.cache_slot as usize).map(|id| id.as_u64())
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    current_paragraph_id = paragraph_id;
                    paragraphs.push(qt_text_node::ParagraphLineInfo {
                        paragraph_text: line.para_text.clone(),
                        para_start: line.para_start,
                        paragraph_id,
                        cache_idx: line.cache_slot,
                        y: para_y,
                        indent_w: line.para_indent,
                        font_size: snapshot.font_size,
                        font_family: snapshot.font_family.clone(),
                        doc_width: snapshot.width,
                    });
                }

                // Issue #658: 构建 per-visual-line 裁剪数据，用于按视觉行裁剪动画接管区域。
                // 每个视觉行携带自己的 cache_slot、qtextline_idx、y、height、x、width，
                // 以及所属段落的 para_y（addTextLayout 偏移）。
                // Issue #901 第十轮问题4: 填入 paragraph_id，供 C++ 侧按 ParagraphId
                // 分组 visual lines 到正确的段落容器节点。
                visual_line_clips.push(qt_text_node::VisualLineClipInfo {
                    cache_idx: line.cache_slot,
                    qtextline_idx: line.qtextline_idx,
                    y: line.y,
                    height: line.height,
                    x: line.x,
                    width: line.width,
                    para_y,
                    doc_width: snapshot.width,
                    paragraph_id: current_paragraph_id,
                });
            }

            // Issue #853: 静态 exclusion 直接来自统一 owner table。
            // 不再通过 StaticLinePatch 中间结构换算。
            // Issue #736 评论 5786231506: 静态层开始裁剪之前先检查本帧动画所需的
            // snapshot texture。缺纹理的 snapshot 对应 clip 不进入 static text clip，
            // canonical 正文直接显示。不能等静态层挖完以后到了 render_text_animation_layer()
            // 才 continue。overlay 可画 -> static 被接管；overlay 不可画 -> static 同帧
            // 恢复 canonical。两边是一条原子规则。
            // Issue #853: 只让资源已准备好的动画 owner 接管 target cluster。
            let available_clip_rects = if animation_resources_ready {
                plan.ownership.clips_for_static_rebuild().to_vec()
            } else {
                Vec::new()
            };
            let available_clip_rects = qt_text_node::merge_static_clip_rects(available_clip_rects);
            let clip_rects = &available_clip_rects;

            // Issue #658 评论 5620035970 问题 4: 正文从 padding 开始画，
            // origin_x = snapshot.padding，与 VisualLine.x = padding + x_off 一致。
            // Issue #658 评论 5620035970 问题 2: 传 snapshot.layout_generation，
            // 用 (generation, cache_slot) 查找 layout，与动画/IME 路径互不干扰。
            // Issue #668: 接住 rebuild 的成功/失败结果，透传给 qquickitem_impl。
            //
            // Issue #901 第十轮问题4: 改为调用 update_text_nodes_incremental
            // 做增量 insert/remove/update，而非全量 rebuild。
            //
            // force_full_update 判断：
            // - needs_relayout: 正文/字体/宽度变化，段落内容可能变了 → update 所有已有段落
            // - !animation_resources_ready: 资源缺失，从有 clip 到无 clip 或反之 → update 所有
            // - viewport_exceeds_loaded（纯滚动跨出）: 段落内容不变 → 只 insert/remove，
            //   force_full_update=false，已有段落按 generation 签名决定是否重建
            //
            // Issue #901 第十一轮问题5：handoff_pending 只影响 clip 集合，不影响段落
            // 内容/布局。不再因 handoff_pending 就 force_full_update=true 无差别重建所有
            // 已有段落节点。clip 子节点的更新由 update_text_nodes_incremental 内部根据
            // animation_clip_rects 参数独立完成，不依赖 force_full_update。
            //
            // Issue #901 第十二轮问题5：force_full_update=false 时不再无差别 continue，
            // 而是由 update_text_nodes_incremental 内部按 per-paragraph content_generation
            // 与当前 generation 比较驱动：generation 不同则重建该段落，相同则跳过或只
            // 更新 para_y。这样即使 force_full_update=false，内容变了的段落也会被重建。
            let force_full_update = static_text.needs_relayout
                || !animation_resources_ready;

            static_rebuild_ok = qt_text_node::update_text_nodes_incremental(
                root_raw,
                item_ptr,
                &paragraphs,
                &visual_line_clips,
                static_text.scroll_y,
                static_text.color,
                clip_rects,
                f64::from(snapshot.padding),
                snapshot.layout_generation,
                force_full_update,
            );

            // Issue #901 第十轮问题4: 增量更新失败时（paragraph_id == 0 或 layout 缺失），
            // 回退到全量 rebuild。全量 rebuild 不依赖 ParagraphId 映射，能正确处理
            // paragraph_id == 0 的情况（ParagraphId::INVALID）。
            //
            // Issue #901 第十一轮问题3（渲染部分）：全量 rebuild 会删掉所有子节点，
            // g_incremental_para_map 中的 container 指针变成悬空。在全量 rebuild
            // **之前**清除该 item 的增量映射，避免悬空指针。
            // clear_incremental_para_map 由 qt_text_node.rs 实现（task 3）。
            if !static_rebuild_ok {
                qt_text_node::clear_incremental_para_map(item_ptr);
                static_rebuild_ok = qt_text_node::rebuild_text_node_from_paragraphs(
                    root_raw,
                    item_ptr,
                    &paragraphs,
                    &visual_line_clips,
                    static_text.scroll_y,
                    static_text.color,
                    clip_rects,
                    f64::from(snapshot.padding),
                    snapshot.layout_generation,
                );
            }

            // Issue #901 第九轮阻塞4: rebuild 成功后记录本帧实际加载的段落范围，
            // 通过 FrameRenderOutcome 返回给调用方保存为 per-item state。
            // rebuild 失败时不更新（返回 None，调用方保留旧范围，下一帧重试）。
            rebuild_new_range = if static_rebuild_ok { new_para_range } else { None };
        } else {
            // 没有与本帧一致的 prepared layout 时不能提交新的 owner 状态。
            static_rebuild_ok = false;
        }
    } else {
        // 纯滚动帧 或 动画 progress 变化帧：只更新 QSGTransformNode 位移矩阵，
        // 不重建静态节点。不出现 clear()、QTextLayout、createLine()、addTextLayout()。
        // Issue #714 评论 5740007764: 动画期间静态层保持上一帧（事务开始时重建的）
        // 裁剪后节点不动，避免每帧销毁/重建静态 QSGTextNode 产生闪烁。
        // Issue #901 问题5缺陷A: 此 else 分支是"本帧正文未变跳过重建"的优化点。
        // should_rebuild_static=false 意味着 needs_relayout=false 且无 handoff_pending
        // 且动画资源就绪且视口未跨出已加载段落集合，即本帧只有滚动位移或动画
        // progress 变化，正文内容未变且新视口仍在上次 rebuild 加载的段落范围内。
        // 此时跳过 rebuild_text_node_from_paragraphs 全量重建，只平移 transform，
        // 成本 O(1) 而非 O(视口内段落数)。
        // Issue #901 第八轮问题3: 视口跨出已加载集合的情况已在上方
        // should_rebuild_static 判断中被 viewport_exceeds_loaded 捕获，走 rebuild
        // 分支按需 insert/remove 段落节点。进入此 else 分支 guarantee 视口未跨出，
        // 平移旧节点集合是安全的（所有可见段落都已在 QSG 中有节点）。
        qt_text_node::update_scroll_transform(root_raw, static_text.scroll_y);
    }

    // Layer 1: 文字动画层（保留：吐字/吞字/重排动画的纹理切片）
    // Issue #709 评论 issue-body-709: 动画层单独画 InsertReveal / DeleteConceal / ReflowMove，
    // 不把静态正文重新补回去。静态层在 clip_count > 0 时已通过 complement 区间
    // 把被动画接管的区域从静态正文里挖掉，动画层只负责画那些被接管的区域。
    // 两层在文档区域上互斥，避免静态正文盖住吐字/吞字动画。
    // Issue #715: 传 scroll_y 给动画层，让 AnimationLayerNode 设置和静态层一样的
    // translate(0, -scroll_y) 矩阵。动画 glyph 继续保留文档坐标，不在 Rust 侧逐个减 scroll_y。
    // 静态 rebuild 失败时保留前一帧的 static + animation 组合，不更新一半新状态。
    // 正常终点、取消和文档切换都走同一原子交接：只有静态层 rebuild 成功后，
    // 才在本次 update_paint_node 中更新/清空动画层。失败时保留旧 static + animation。
    //
    // Issue #901 第八轮问题2（渲染器部分）：渲染接口返回本帧实际采用的 owner/glyph。
    // 当 animation_resources_ready=false 或 handoff_pending=true 时，should_rebuild_static
    // 为 true，静态层走 rebuild 画完整 canonical 正文（available_clip_rects 为空，
    // 不挖空任何区域）；动画层 glyphs=&[]（下方判断）被清空。即资源缺失的片段由
    // 静态 owner 画，动画层不画任何 glyph。PresentedScene 侧由 Task 3 在
    // qquickitem_impl.rs::build_presented_scene_from_frame 修复为排除动画接管区域、
    // 验证纹理存在、绑定 target revision，与本处渲染器侧的"资源缺失→静态 owner
    // 画、动画层清空"决策一致。两层通过 animation_resources_ready 和 handoff_pending
    // 同一组标志做对称决策，不出现"静态挖了空但动画没画"或"动画画了但静态没挖空"。
    //
    // Issue #901 第十轮问题1：render_text_animation_layer 返回本帧真正绘制的
    // fragment 列表，供 FrameRenderOutcome.drawn_animation_fragments 使用。
    let mut drawn_animation_fragments: Vec<DrawnAnimationFragment> = Vec::new();
    if static_rebuild_ok {
        let glyphs = if animation_resources_ready && !plan.ownership.handoff_pending {
            plan.ownership.animated_glyphs.as_slice()
        } else {
            &[]
        };
        drawn_animation_fragments = render_text_animation_layer(
            root_raw,
            item_ptr,
            glyphs,
            texture_cache,
            static_text.scroll_y,
        );
    }
    // Layer 2: 选区/预输入背景
    // Issue #677 评论 5654174714: scroll_y 作为每帧轻量状态传给 renderer，
    // selection/preedit 几何保持文档坐标，由 renderer 在绘制时做视口换算。
    // Issue #677 评论 5654686856: viewport_height 一并传入，renderer 做轻量视口裁剪。
    render_selection_preedit_layer(
        root_raw,
        item_ptr,
        plan,
        static_text.scroll_y,
        static_text.viewport_height,
    );
    // Layer 3: 光标
    // Issue #727 评论 5755858583 问题1: scroll_y 传给 render_cursor_layer，
    // cursor layer 的 QSGTransformNode 统一做 translate(0, -scroll_y)。
    render_cursor_layer(root_raw, item_ptr, plan, static_text.scroll_y);

    // Issue #901 第十轮问题1+问题3：计算静态 owner 本帧实际可见的片段。
    //
    // 静态 QSGTextNode 在 `effective_clip_rects` 挖空后实际绘制的 cluster 片段。
    // 这里在渲染器内部完成计算，让调用方不再用前置的
    // `animation_resources_ready_for_static` 平行推测。
    //
    // 关键修复（问题3）：无论 cluster 是否有纹理，都统一减去 effective_holes。
    // 旧实现无纹理时直接登记整个 visible_rect 不减 holes，导致缺纹理静态字
    // 在动画排除区内仍被记录为完整静态字。现统一用 rect_subtract_holes
    // 计算真实可见静态片段，无纹理时也减 holes。
    //
    // static_rebuild_ok=false 时不计算（调用方应保留旧 PresentedScene）。
    let drawn_static_fragments: Vec<DrawnStaticFragment> = if static_rebuild_ok {
        compute_drawn_static_fragments(
            static_text,
            &effective_clip_rects,
            plan,
        )
    } else {
        Vec::new()
    };

    FrameRenderOutcome {
        static_rebuild_ok,
        effective_clip_rects,
        new_loaded_para_range: rebuild_new_range,
        drawn_animation_fragments,
        drawn_static_fragments,
    }
}

/// Issue #901 第十轮问题1+问题3：计算静态 owner 本帧实际可见的片段。
///
/// 从 `layout_snapshot` 的 cluster 提取，每个 cluster 用 `rect_subtract_holes`
/// 减去 `effective_clip_rects` 的矩形并集，得到静态层实际绘制的部分。
/// 无论 cluster 是否有纹理，都统一减去 effective_holes（问题3 修复）。
///
/// 此函数是 `qquickitem_impl.rs` 中 `static_text_contributions` 构造逻辑的
/// 渲染器侧镜像，让调用方可以直接用 `drawn_static_fragments` 构造
/// `RenderPlan.static_text_contributions`，不再自行推测。
fn compute_drawn_static_fragments(
    static_text: &StaticTextParams<'_>,
    effective_clip_rects: &[qt_text_node::AnimationClipRect],
    plan: &RenderPlan,
) -> Vec<DrawnStaticFragment> {
    use super::layout_snapshot::SourceRect;
    use super::visual_canvas::PixelContribution;

    let snapshot = match static_text.layout_snapshot {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Issue #901 第十轮问题1：source_revision 用 RenderPlan 携带的 target
    // revision。RenderPlan.ownership.target_layout_revision 是 Option<LayoutRevision>，
    // 但我们需要 EditorRevision。此处用 EditorRevision::initial() 作为占位，
    // 调用方在构造 PresentedScene 时会用 canvas_plan.target_revision 覆盖。
    let static_source_revision = EditorRevision::initial();

    // effective_holes 转换为 (x, y, w, h) 元组供矩形差集使用。
    let effective_holes: Vec<(f64, f64, f64, f64)> = effective_clip_rects
        .iter()
        .map(|c| (c.x, c.y, c.w, c.h))
        .collect();

    let viewport_top = static_text.scroll_y;
    let viewport_bottom = static_text.scroll_y + static_text.viewport_height;
    // Issue #901 第十二轮问题1补充: viewport_top/viewport_bottom 保留供
    // 诊断和断言使用，视口裁剪已由上游 qquickitem_impl 写入 info.visible_clip，
    // 本函数不再重复做视口求交，直接消费 visible_clip。
    let _ = (viewport_top, viewport_bottom);

    let mut fragments = Vec::new();

    // Issue #901 第十轮问题3：用 prepared_frame 的 line_snapshots 而非
    // layout_snapshot.lines。layout_snapshot.lines 是 VisualLine（排版行），
    // 没有 cluster 信息。cluster 信息在 EditorLayoutSnapshot.line_snapshots
    // 中，由 qquickitem_impl 的 canonical_visual_snapshot 提供。
    // 但 render_frame 的 StaticTextParams 只有 LayoutSnapshot，没有
    // EditorLayoutSnapshot。因此此处用 plan.static_text_contributions
    //（由调用方从 canonical_visual_snapshot 构造）作为输入，减去 effective_holes
    // 后输出 DrawnStaticFragment。
    //
    // 这样设计的原因：
    // 1. cluster 信息（byte_range, source_rect）只在 EditorLayoutSnapshot 中，
    //    LayoutSnapshot（VisualLine 列表）没有。
    // 2. 调用方已经从 canonical_visual_snapshot 构造了 static_text_contributions，
    //    包含完整的 cluster 信息。
    // 3. 渲染器只需在此基础上减去 effective_holes，得到真实可见静态片段。
    //
    // 问题3 修复：无论有无纹理，都统一减去 effective_holes。
    // 旧实现（qquickitem_impl.rs:462-556）有纹理时减 holes、无纹理时不减，
    // 现统一为减 holes。
    for info in &plan.static_text_contributions {
        // Issue #901 第十二轮问题1补充:
        // doc_rect 是裁剪前的完整字形几何（cluster 真实高度），
        // 用于 UV 比例计算和 original_glyph_rect，避免视口边缘"半字高度"
        // 冒充完整字形高度导致 FLIP 变换基准错误。
        // visible_rect 直接采用上游已与视口求交的 info.visible_clip，
        // 不再在本函数重复做视口裁剪。
        let doc_rect = (
            info.document_rect.x,
            info.document_rect.y,
            info.document_rect.w,
            info.document_rect.h,
        );
        let visible_rect = (
            info.visible_clip.x,
            info.visible_clip.y,
            info.visible_clip.w,
            info.visible_clip.h,
        );
        if visible_rect.3 <= 0.0 {
            continue;
        }
        // Issue #901 第十轮问题3：统一减去 effective_holes，无论有无纹理。
        let sub_fragments = rect_subtract_holes(visible_rect, &effective_holes);
        for (frag_idx, frag) in sub_fragments.iter().enumerate() {
            let (fx, fy, fw, fh) = *frag;
            if fw <= 0.0 || fh <= 0.0 {
                continue;
            }
            // 片段的 source_uv：按片段在 cluster 中的比例裁剪 source_rect。
            let u_scale = if doc_rect.2 > 0.0 {
                info.source_uv.w / doc_rect.2
            } else {
                0.0
            };
            let v_scale = if doc_rect.3 > 0.0 {
                info.source_uv.h / doc_rect.3
            } else {
                0.0
            };
            let frag_source_uv_x = info.source_uv.x + (fx - doc_rect.0) * u_scale;
            let frag_source_uv_y = info.source_uv.y + (fy - doc_rect.1) * v_scale;
            let frag_source_uv_w = fw * u_scale;
            let frag_source_uv_h = fh * v_scale;
            let glyph_id = if info.glyph_id != 0 {
                info.glyph_id
            } else {
                PixelContribution::derive_glyph_id(&info.snapshot_id, info.byte_range)
            };
            fragments.push(DrawnStaticFragment {
                snapshot_id: info.snapshot_id,
                byte_range: info.byte_range,
                document_rect: SourceRect {
                    x: fx,
                    y: fy,
                    w: fw,
                    h: fh,
                },
                visible_clip: SourceRect {
                    x: fx,
                    y: fy,
                    w: fw,
                    h: fh,
                },
                source_uv: SourceRect {
                    x: frag_source_uv_x,
                    y: frag_source_uv_y,
                    w: frag_source_uv_w,
                    h: frag_source_uv_h,
                },
                source_revision: static_source_revision,
                glyph_id,
                fragment_index: frag_idx as u32,
                // Issue #901 第十一轮问题4：完整 cluster 几何（未裁剪前的值）。
                // info.document_rect 是 cluster 的真实字形几何，sub_fragments 是
                // 它被视口/holes 裁剪后的子片段。original_glyph_rect 保存完整值。
                original_glyph_rect: SourceRect {
                    x: info.document_rect.x,
                    y: info.document_rect.y,
                    w: info.document_rect.w,
                    h: info.document_rect.h,
                },
            });
        }
    }

    fragments
}

/// Issue #901 第十轮问题2：从矩形 `rect` 中挖掉所有 `holes`，返回剩余的轴对齐矩形列表。
///
/// 用于计算 cluster 的真实可见静态片段：cluster 的可见区域减去动画层接管的
/// 区域（`effective_clip_rects` 的并集），得到静态层实际绘制的部分。
///
/// 算法：从 `[rect]` 开始，对每个 hole，对结果中的每个矩形执行单矩形差集
/// （最多产生 4 个子矩形：上、下、左、右），过滤零面积矩形。
fn rect_subtract_holes(
    rect: (f64, f64, f64, f64),
    holes: &[(f64, f64, f64, f64)],
) -> Vec<(f64, f64, f64, f64)> {
    let mut fragments = vec![rect];
    for &hole in holes {
        let mut next = Vec::new();
        for &frag in &fragments {
            next.extend(rect_subtract_one(frag, hole));
        }
        fragments = next;
    }
    fragments
}

/// 单矩形差集：从 `r` 中挖掉 `h`，返回剩余的轴对齐矩形列表（最多 4 个）。
///
/// 如果 `r` 和 `h` 无交集，返回 `[r]`。否则返回 `r` 中不被 `h` 覆盖的部分
/// （上、下、左、右条带），过滤零面积矩形。
fn rect_subtract_one(
    r: (f64, f64, f64, f64),
    h: (f64, f64, f64, f64),
) -> Vec<(f64, f64, f64, f64)> {
    let (rx, ry, rw, rh) = r;
    let (hx, hy, hw, hh) = h;
    // 交集
    let ix = rx.max(hx);
    let iy = ry.max(hy);
    let ix2 = (rx + rw).min(hx + hw);
    let iy2 = (ry + rh).min(hy + hh);
    let iw = ix2 - ix;
    let ih = iy2 - iy;
    if iw <= 0.0 || ih <= 0.0 {
        // 无交集，返回原矩形
        return vec![r];
    }
    let mut result = Vec::new();
    // top 条带
    if iy > ry {
        result.push((rx, ry, rw, iy - ry));
    }
    // bottom 条带
    if iy2 < ry + rh {
        result.push((rx, iy2, rw, ry + rh - iy2));
    }
    // left 条带
    if ix > rx {
        result.push((rx, iy, ix - rx, ih));
    }
    // right 条带
    if ix2 < rx + rw {
        result.push((ix2, iy, rx + rw - ix2, ih));
    }
    result
}

fn render_text_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    glyphs: &[super::render_plan::TextAnimationGlyphInfo],
    texture_cache: &TextureCache,
    scroll_y: f64,
) -> Vec<DrawnAnimationFragment> {
    // Issue #658 评论 5630650436: 空动画帧也进入 update_animation_layer，让 AnimationLayerNode
    // 自己把 node 和 texture 一次清干净。不要在这里提前 return，否则
    // update_animation_layer() 根本不调用，activeSet sweep 根本没运行，
    // AnimationLayerNode::m_texture_cache 里的 QSGTexture 继续留着。
    let mut glyph_data: Vec<f64> = Vec::new();
    let mut glyph_images: Vec<qmetaobject::QImage> = Vec::new();
    let mut source_rects: Vec<f64> = Vec::new();
    let mut snapshot_ids: Vec<u64> = Vec::new();
    // Issue #901 第十轮问题1：收集本帧真正绘制的 fragment。
    let mut drawn_fragments: Vec<DrawnAnimationFragment> = Vec::new();

    for (glyph_idx, glyph) in glyphs.iter().enumerate() {
        // Issue #736 评论 5778543593 修改1: 缺该 snapshot_id 的纹理时，这个
        // 文字动画 frame 不进入 glyph_data。缺纹理直接 continue 跳过整个
        // glyph，不再塞 1×1 空图把"DeleteConceal 已生成但纹理没拿到"伪装成
        // "动画层正常执行只是什么也看不到"。有纹理时才 push 四个 vec，这样
        // 四个 vec 长度始终一致，glyph_count == glyph_images.len() 自然通过。
        let texture = match texture_cache.get_line(&glyph.snapshot_id) {
            Some(texture) => texture,
            None => continue,
        };

        // Issue #901 问题4：用 clip_rect 对 target_rect 做预裁剪，并按比例
        // 调整 source_uv，使吞字/吐字的 reveal/conceal 裁剪真正作用到 Qt
        // 图形层。不需要改 C++ 签名 —— 裁剪后的 target_rect 和 source_uv
        // 直接传给 update_animation_layer。
        //
        // 矩形交集 + UV 映射：
        //   ix = max(tx, cx), iy = max(ty, cy)
        //   ix2 = min(tx+tw, cx+cw), iy2 = min(ty+th, cy+ch)
        //   new_target = (ix, iy, ix2-ix, iy2-iy)
        //   u_scale = sw/tw, v_scale = sh/th
        //   new_source = (sx + (ix-tx)*u_scale, sy + (iy-ty)*v_scale,
        //                 (ix2-ix)*u_scale, (iy2-iy)*v_scale)
        let tx = glyph.x;
        let ty = glyph.y;
        let tw = glyph.w;
        let th = glyph.h;
        let sx = glyph.source_rect.x;
        let sy = glyph.source_rect.y;
        let sw = glyph.source_rect.w;
        let sh = glyph.source_rect.h;
        let cx = glyph.clip_rect.x;
        let cy = glyph.clip_rect.y;
        let cw = glyph.clip_rect.w;
        let ch = glyph.clip_rect.h;

        let ix = tx.max(cx);
        let iy = ty.max(cy);
        let ix2 = (tx + tw).min(cx + cw);
        let iy2 = (ty + th).min(cy + ch);
        let iw = ix2 - ix;
        let ih = iy2 - iy;

        // 裁剪后无可见面积则跳过此 glyph。
        if iw <= 0.0 || ih <= 0.0 {
            continue;
        }

        // UV 按比例调整。tw/th 为 0 时不会到达这里（上面 iw/ih 判断已过滤），
        // 但防御性处理避免除零。
        let u_scale = if tw > 0.0 { sw / tw } else { 0.0 };
        let v_scale = if th > 0.0 { sh / th } else { 0.0 };
        let new_sx = sx + (ix - tx) * u_scale;
        let new_sy = sy + (iy - ty) * v_scale;
        let new_sw = iw * u_scale;
        let new_sh = ih * v_scale;

        glyph_data.extend_from_slice(&[ix, iy, iw, ih, glyph.opacity]);

        source_rects.extend_from_slice(&[new_sx, new_sy, new_sw, new_sh]);

        let cache_key = glyph.snapshot_id.to_cache_key();
        snapshot_ids.push(cache_key);
        glyph_images.push(texture.clone());

        // Issue #901 第十一轮问题1：记录本帧真正绘制的 fragment。
        // byte_range/source_revision/glyph_id/fragment_index 全部从 glyph 原样传递，
        // 不再用 EditorRevision::initial() 占位、不重新计算 glyph_id。
        let byte_range = if glyph.logical_range.1 > glyph.logical_range.0 {
            glyph.logical_range
        } else if let Some(cr) = glyph.canonical_range {
            cr
        } else {
            glyph.logical_range
        };
        drawn_fragments.push(DrawnAnimationFragment {
            snapshot_id: glyph.snapshot_id,
            target_rect: super::layout_snapshot::SourceRect {
                x: ix,
                y: iy,
                w: iw,
                h: ih,
            },
            source_uv: super::layout_snapshot::SourceRect {
                x: new_sx,
                y: new_sy,
                w: new_sw,
                h: new_sh,
            },
            opacity: glyph.opacity,
            byte_range,
            // Issue #901 第十一轮问题1：用 glyph.source_revision，不再占位。
            source_revision: glyph.source_revision,
            // Issue #901 第十一轮问题1：用 glyph.glyph_id，不再重新计算。
            glyph_id: glyph.glyph_id,
            fragment_index: glyph.fragment_index,
            // Issue #901 第十一轮问题4：传递完整字形矩形。
            original_glyph_rect: super::layout_snapshot::SourceRect {
                x: glyph.original_glyph_rect.x,
                y: glyph.original_glyph_rect.y,
                w: glyph.original_glyph_rect.w,
                h: glyph.original_glyph_rect.h,
            },
        });
    }

    let glyph_count = glyph_data.len() / 5;
    if glyph_count > 0 && glyph_count == glyph_images.len() {
        let glyph_data_ptr = glyph_data.as_ptr();
        let image_ptrs: Vec<*const qmetaobject::QImage> = glyph_images
            .iter()
            .map(|img| img as *const qmetaobject::QImage)
            .collect();
        let image_ptrs_ptr = image_ptrs.as_ptr();
        let source_rects_ptr = source_rects.as_ptr();
        let snapshot_ids_ptr = snapshot_ids.as_ptr();

        scene_graph::update_animation_layer(
            root_raw,
            item_ptr,
            glyph_count as i32,
            glyph_data_ptr,
            image_ptrs_ptr,
            source_rects_ptr,
            snapshot_ids_ptr,
            scroll_y,
        );
    } else {
        scene_graph::clear_animation_layer(root_raw, item_ptr);
    }

    drawn_fragments
}

fn render_cursor_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    scroll_y: f64,
) {
    // Issue #679 评论 5657313927: 直接画 CursorRenderState，
    // 不再理解 Snap/Tween/driver/Timestamp。
    // Issue #727 评论 5755858583 问题1: cursor.y 是文档坐标，scroll_y 传给
    // update_cursor_node 由 QSGTransformNode 统一做视口变换。
    let cursor = &plan.cursor;
    let cursor_style = &plan.cursor_style;

    if !cursor.visible {
        scene_graph::update_cursor_node(
            root_raw,
            item_ptr,
            cursor.x,
            cursor.y,
            cursor_style.width,
            cursor.h,
            0.0,
            cursor_style.color.as_ptr(),
            cursor_style.color.len(),
            scroll_y,
        );
        return;
    }

    scene_graph::update_cursor_node(
        root_raw,
        item_ptr,
        cursor.x,
        cursor.y,
        cursor_style.width,
        cursor.h,
        cursor.opacity,
        cursor_style.color.as_ptr(),
        cursor_style.color.len(),
        scroll_y,
    );
}

fn render_selection_preedit_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    scroll_y: f64,
    viewport_height: f64,
) {
    let sp = &plan.selection_preedit;
    let total_count = sp.selection_ranges.len() + sp.preedit_ranges.len();
    if total_count == 0 {
        scene_graph::update_selection_preedit_layer(root_raw, item_ptr, 0, std::ptr::null());
        return;
    }

    // Issue #677 评论 5654174714: 颜色是每帧轻量状态，从 RenderPlan.selection_preedit_style
    // 读取；带透明度的最终颜色在 renderer 内部计算（selection 0x33, preedit 0x1A），
    // 与原 build_selection_preedit_plan_from_snapshot 中的逻辑保持一致。
    let base_color = &plan.selection_preedit_style.selection_color;
    let (base_r, base_g, base_b) = if base_color.starts_with('#') && base_color.len() >= 7 {
        (
            f64::from(u8::from_str_radix(&base_color[1..3], 16).unwrap_or(0)) / 255.0,
            f64::from(u8::from_str_radix(&base_color[3..5], 16).unwrap_or(0)) / 255.0,
            f64::from(u8::from_str_radix(&base_color[5..7], 16).unwrap_or(0)) / 255.0,
        )
    } else {
        // 与原 fallback "#3381D1D1" / "#1A81D1D1" 的 RGB 部分一致。
        (
            0x81 as f64 / 255.0,
            0xD1 as f64 / 255.0,
            0xD1 as f64 / 255.0,
        )
    };
    let selection_alpha = 0x33 as f64 / 255.0;
    let preedit_alpha = 0x1A as f64 / 255.0;

    let mut rect_data: Vec<f64> = Vec::with_capacity(total_count * 10);

    // Issue #677 评论 5654686856: 每帧轻量视口裁剪，只跳过完全在屏幕外的矩形。
    // 滚动只改变轻量显示状态，不重新排版，也不会因为旧 frame 曾经裁掉某些行而丢选区
    // （因为 frame 现在保存完整文档坐标几何，见 build_selection_preedit_plan_from_snapshot）。
    for sel in &sp.selection_ranges {
        // Issue #677 评论 5654174714: sel.y 是文档坐标，绘制时减 scroll_y 得到视口坐标。
        let screen_y = sel.y - scroll_y;
        if screen_y + sel.h < 0.0 || screen_y > viewport_height {
            continue;
        }
        rect_data.extend_from_slice(&[
            sel.x,
            screen_y,
            sel.w,
            sel.h,
            base_r,
            base_g,
            base_b,
            selection_alpha,
            0.0,
            0.0,
        ]);
    }

    for pre in &sp.preedit_ranges {
        let screen_y = pre.y - scroll_y;
        if screen_y + pre.h < 0.0 || screen_y > viewport_height {
            continue;
        }
        let underline = if pre.underline { 1.0 } else { 0.0 };
        rect_data.extend_from_slice(&[
            pre.x,
            screen_y,
            pre.w,
            pre.h,
            base_r,
            base_g,
            base_b,
            preedit_alpha,
            underline,
            0.0,
        ]);
    }

    // Issue #677 评论 5654686856: 用 rect_data 实际长度计算 count，因为视口裁剪可能跳过部分矩形。
    let actual_count = rect_data.len() / 10;
    if actual_count == 0 {
        scene_graph::update_selection_preedit_layer(root_raw, item_ptr, 0, std::ptr::null());
    } else {
        scene_graph::update_selection_preedit_layer(
            root_raw,
            item_ptr,
            actual_count as i32,
            rect_data.as_ptr(),
        );
    }
}
