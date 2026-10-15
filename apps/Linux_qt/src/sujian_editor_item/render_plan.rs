use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
use super::qt_text_node::AnimationClipRect;
use super::texture_cache::TextureCache;
use crate::editor::layout::LayoutSnapshot;
use crate::sujian_editor_item::cursor_controller::CursorMoveSource;
use crate::sujian_editor_item::edit_motion::DeleteEdge;
use writer_core::editor::EditorRevision;

/// Issue #904：动画层本帧要画的一个局部文字覆盖层。
///
/// 由 `text_motion::TextMotionFrame.overlays` 生成，位置 / 源 UV / clip 都已算好。
/// 与静态层同一坐标系（文档坐标，不含滚动偏移），C++ 动画层统一施加一次
/// `translate(0, -scroll_y)`。
#[derive(Clone, Debug)]
pub(crate) struct TextAnimationGlyphInfo {
    /// 文档坐标 x（不含滚动偏移）。
    pub x: f64,
    /// 文档坐标 y（不含滚动偏移）。
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
    /// 动态裁剪矩形（文档坐标）：渲染时只绘制 `target_rect ∩ clip_rect`。
    ///
    /// 按同一个 `progress` 更新（吐字/吞字据此推进），与静态层孔洞是两个概念：
    /// 静态孔洞覆盖目标字形完整范围（与 progress 无关），动态 clip 随 progress 变化。
    pub clip_rect: SourceRect,
    /// 逻辑 cluster identity；旧侧与新侧 glyph 都保留各自的范围。
    pub logical_range: (usize, usize),
    /// 此 glyph 在当前 canonical revision 中对应的范围。纯删除旧字没有该身份。
    pub canonical_range: Option<(usize, usize)>,
    /// 删除 glyph 的 caret 边缘；其他动画 glyph 为 None。
    pub delete_edge: Option<DeleteEdge>,
    pub shaping_identity: ShapingIdentity,
    /// 该动画 glyph 对应的 Core source revision（旧字为编辑前，新字为编辑后）。
    pub source_revision: EditorRevision,
    /// 字形逻辑身份，由 `text_motion::derive_fragment_id` 派生。
    pub glyph_id: u64,
    /// 同一字形内的碎片序号。
    pub fragment_index: u32,
    /// 原始完整 glyph rectangle（未被裁剪的完整字形矩形）。
    pub original_glyph_rect: SourceRect,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SelectionPreeditPlan {
    pub has_selection: bool,
    pub selection_ranges: Vec<SelectionRange>,
    pub has_preedit: bool,
    pub preedit_ranges: Vec<PreeditRange>,
}

/// Issue #677 评论 5653944889: GUI 线程一次性准备好的不可变帧数据。
///
/// `layout_snapshot` 和 `selection_preedit` 来自同一次 `EditorLayout::snapshot()` 调用，
/// 确保 render thread（`update_paint_node()`）只读取已经准备好的数据，
/// 不再进入排版生命周期（`EditorLayout::snapshot()` /
/// `begin_layout_generation()` / `clear_layout_generation()`）。
///
/// 不变性：
/// - 只在 GUI 线程上由 `prepare_editor_frame()` 构造并一次性替换 `prepared_frame`。
/// - render thread 只读 `layout_snapshot` 和 `selection_preedit`，不调用任何排版方法。
/// - `prepared_frame = None` 表示需要 GUI 侧重新准备，render thread 跳过静态正文渲染。
///
/// Issue #677 评论 5654174714: `selection_preedit` 只保存文档坐标几何
/// （`SelectionRange.y` / `PreeditRange.y` 是文档坐标，不减 `scroll_y`）。
/// `scroll_y`、selection/preedit 颜色、cursor 颜色、动画进度属于每帧轻量状态，
/// 不进入 `PreparedEditorFrame`；由 `update_paint_node()` 读取当前值后通过
/// `RenderPlan` 的轻量 style 字段（`SelectionPreeditStyle` / `CursorStyle`）
/// 传给 renderer，renderer 在绘制时做 `screen_y = doc_y - scroll_y` 换算。
#[derive(Clone, Debug)]
pub(crate) struct PreparedEditorFrame {
    /// 当前已准备好的 canonical 排版快照。
    pub layout_snapshot: LayoutSnapshot,
    /// 从同一个 snapshot 算好的选区/preedit 文档坐标几何（不含颜色/scroll_y）。
    pub selection_preedit: SelectionPreeditPlan,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectionRange {
    pub x: f64,
    /// 文档坐标 y（不减 scroll_y）。视口换算由 renderer 在绘制时完成。
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct PreeditRange {
    pub x: f64,
    /// 文档坐标 y（不减 scroll_y）。视口换算由 renderer 在绘制时完成。
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub underline: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CursorStyle {
    pub color: String,
    pub width: f64,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self {
            color: "#006497".to_string(),
            width: 2.0,
        }
    }
}

/// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
/// 不进入排版，也不固化进 `PreparedEditorFrame`；由 `update_paint_node()` 读取当前
/// `current_selection_color` 后传给 renderer。preedit 透明度在 renderer 计算。
#[derive(Clone, Debug)]
pub(crate) struct SelectionPreeditStyle {
    pub selection_color: String,
}

impl Default for SelectionPreeditStyle {
    fn default() -> Self {
        Self {
            selection_color: "#3381D1".to_string(),
        }
    }
}

/// Issue #679 评论 5657313927: 纯显示数据 — render thread 直接画这个，
/// 不再理解 Snap/Tween/driver/Timestamp。由 qquickitem_impl 从 cursor_ctrl
/// 的 visual_x/y/h/visible 和 blink opacity 构造。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CursorRenderState {
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub baseline_y: f64,
    pub opacity: f64,
    /// The active move intent and the route currently owned by CursorController.
    pub movement_source: Option<CursorMoveSource>,
    pub driver_revision: Option<LayoutRevision>,
    pub driver_transition_id: u64,
    pub document_session: u64,
    pub target_x: f64,
    pub target_y: f64,
    pub path_start_x: f64,
    pub path_start_y: f64,
}

/// Issue #904：本帧要画的局部文字覆盖层 + 静态层要挖掉的局部孔洞。
///
/// 取代 #901 的 `RenderOwnership`：不再有整屏 ownership/revision/handoff
/// 状态机，只有「这一帧动画层画哪些局部片段」和「静态层排除哪些局部矩形」。
#[derive(Clone, Debug, Default)]
pub(crate) struct LocalTextPlan {
    /// 动画层本帧的局部文字覆盖层（吐字/吞字/位移）。
    pub overlays: Vec<TextAnimationGlyphInfo>,
    /// 静态层本帧需要排除的局部矩形，与 `overlays` 原子提交。
    pub static_holes: Vec<AnimationClipRect>,
    /// 本帧接管所需的全部动画源纹理 id。
    ///
    /// 用「本计划全部片段要用到的源纹理」而不是「本帧可见的 overlay 纹理」判断
    /// 就绪：progress=0 时 Reveal 尚无可见像素，但它的目标字形已被静态孔洞排除，
    /// 若此时漏检纹理，新字会先消失。
    pub required_textures: Vec<LineSnapshotId>,
    /// 本帧仍活动的整体段落 y 偏移（复用已有 transform，不逐字复制纹理）。
    ///
    /// 元素为 `(paragraph_id, 相对目标位置的额外 y 偏移)`；动画结束为空，
    /// 段落直接留在目标矩阵。
    pub paragraph_y_offsets: Vec<(u64, f64)>,
}

impl LocalTextPlan {
    /// 检查本帧接管所需的动画源纹理是否已在 `texture_cache` 中就绪。
    ///
    /// 缺任一张就整体不接管：静态层画完整 canonical 正文，动画层不画。
    /// 静态挖孔与动画覆盖层用同一标志，避免"静态挖了空但动画没画"或反之。
    pub(crate) fn has_animation_resources(&self, texture_cache: &TextureCache) -> bool {
        self.required_textures
            .iter()
            .all(|id| texture_cache.contains_line(id))
    }
}

/// Issue #904：静态正文（QSGTextNode）本帧实际绘制的轻量纯数据信息。
///
/// 从 `EditorLayoutSnapshot.line_snapshots`（`PreparedLineSnapshot`）按 **cluster**
/// 提取每个可见字形的 `LineSnapshotId`、Core UTF-8 byte range、文档坐标几何和源
/// 纹理 UV。`RenderPlan` 用它登记静态层已画出的正文身份，供帧回执
/// （`frame_submission`）保留本帧活跃纹理。
///
/// `document_rect` / `visible_clip` / `source_uv` 使用 `SourceRect`（本模块统一的
/// 几何载体）。`glyph_id` 由 `text_motion::derive_fragment_id` 派生；同一 cluster
/// 被裁成多块时共享同一 `glyph_id`、不同 `fragment_index`。
#[derive(Clone, Debug)]
pub(crate) struct StaticTextContributionInfo {
    /// 源纹理的完整行快照 id（来自 `PreparedLineSnapshot.id`）。
    pub snapshot_id: LineSnapshotId,
    /// 该贡献对应的 Core 逻辑 UTF-8 byte range `(start, end)`（半开区间），
    /// 来自 `LineClusterSnapshot.byte_start / byte_end`（cluster 级，非整行）。
    pub byte_range: (usize, usize),
    /// 文档坐标系中的目标矩形（不含滚动偏移）。
    pub document_rect: SourceRect,
    /// 实际可见的裁剪区域（文档坐标，已与视口求交）；
    /// `visible_clip.h <= 0` 的行（完全在视口外）不登记贡献。
    pub visible_clip: SourceRect,
    /// 源纹理中的 UV 区域（行纹理局部坐标，已乘 DPR）。
    pub source_uv: SourceRect,
    /// 该静态贡献对应的 Core source revision（当前 canonical 的 target revision）。
    pub source_revision: EditorRevision,
    /// 字形逻辑身份，由 `text_motion::derive_fragment_id` 派生。
    pub glyph_id: u64,
    /// 同一字形内的碎片序号；单一 cluster 未被裁剪时为 0。
    pub fragment_index: u32,
}

#[derive(Clone, Debug, Default)]
/// Issue #707 评论 5723616999: 改 `pub` 让集成测试能访问 `drawn_caret_rect` 字段。
/// 加 `Default` 让集成测试能构造实例验证字段可读写。
/// 一个 immutable RenderPlan 汇总本帧的局部文字效果、selection/preedit 和唯一 caret。
/// renderer 不读取动画状态，也不重新决定 owner。
pub struct RenderPlan {
    // 除 drawn_caret_rect 外全部收回 pub(crate)：字段类型都是平台端内部渲染
    // 状态（LocalTextPlan / CursorRenderState 等），集成测试只需要
    // drawn_caret_rect 这个 (x, y, h) 裸元组。
    //
    // 光标采样在 CursorController 内完成，RenderPlan 只保存最终要显示的位置。
    pub(crate) local_text: LocalTextPlan,
    pub(crate) selection_preedit: SelectionPreeditPlan,
    /// Issue #679 评论 5657313927: 改为纯显示数据 CursorRenderState，
    /// 不再携带 CursorAnimationPlan（Snap/Tween/driver 由 GUI 线程消费）。
    pub(crate) cursor: CursorRenderState,
    pub(crate) cursor_style: CursorStyle,
    /// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
    pub(crate) selection_preedit_style: SelectionPreeditStyle,
    /// Issue #904：静态 QSGTextNode 本帧绘制的正文行贡献信息。
    ///
    /// 从 `EditorLayoutSnapshot.line_snapshots` 提取，登记静态正文身份，
    /// 供帧回执保留本帧活跃纹理。未变化的段落沿用已有 QSGTextNode，
    /// 只有局部动画接管的小块由动画层绘制。
    pub(crate) static_text_contributions: Vec<StaticTextContributionInfo>,
    /// Issue #705: 本帧真正绘制出去的 caret rect `(x, y, h)`。
    ///
    /// 该 rect 是 CursorController 本帧采样后的实际光标位置；Snap 时就是目标位置。
    /// `qquickitem_impl` 每帧生成 RenderPlan 后,把
    /// `cursor_ctrl.visual_x/visual_y/visual_h` 同步成此值。下一次
    /// 下一次光标目标更新时从这个"上一帧真正画出来的位置" rebase。
    pub drawn_caret_rect: Option<(f64, f64, f64)>,
}
