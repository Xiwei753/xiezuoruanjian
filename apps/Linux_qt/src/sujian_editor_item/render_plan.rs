use super::cursor_animation::CursorAnimationPlan;
use super::layout_snapshot::{LineSnapshotId, SourceRect};
use super::static_line_patch::StaticLinePatch;
use super::transaction_key::VisualTransactionKey;
use crate::editor::layout::LayoutSnapshot;

#[derive(Clone, Debug)]
pub(crate) struct TextAnimationGlyphInfo {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextAnimationPlan {
    pub glyphs: Vec<TextAnimationGlyphInfo>,
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

#[derive(Clone, Debug, Default)]
pub(crate) struct FrameContext {
    pub active_transaction_keys: Vec<VisualTransactionKey>,
    pub keys_to_complete: Vec<VisualTransactionKey>,
    pub keys_to_cancel: Vec<VisualTransactionKey>,
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

#[derive(Clone, Debug)]
pub(crate) struct RenderPlan {
    pub text_animation: TextAnimationPlan,
    pub selection_preedit: SelectionPreeditPlan,
    pub cursor: CursorAnimationPlan,
    pub frame_context: FrameContext,
    pub cursor_style: CursorStyle,
    /// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
    pub selection_preedit_style: SelectionPreeditStyle,
    /// 动画期间静态正文层需要隐藏的区域。
    /// 由 active transaction 的 static_patches 提供，包含精确的行级裁剪信息。
    pub static_patches: Vec<StaticLinePatch>,
}
