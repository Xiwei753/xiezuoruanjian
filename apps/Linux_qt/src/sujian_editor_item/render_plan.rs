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

/// Issue #679 评论 5657313927: 纯显示数据 — render thread 直接画这个，
/// 不再理解 Snap/Tween/driver/Timestamp。由 qquickitem_impl 从 cursor_ctrl
/// 的 visual_x/y/h/visible 和 blink opacity 构造。
#[derive(Clone, Debug, Default)]
pub(crate) struct CursorRenderState {
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub opacity: f64,
}

/// Issue #701 评论 5699573227 第三阶段 (F5): 光标 frame state 采样结果。
///
/// 由 `build_render_plan_full` 内部用同一份 `AnimationFrameSample` 采样，
/// 供 `update_paint_node` 推进 `cursor_ctrl` 的 visual_x/visual_y。
/// 文字层和光标层都使用同一份 frame state，消除 GUI tick 与 Scene Graph
/// 渲染帧之间的采样偏差。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CursorSampleOutcome {
    /// 无 CursorOnly 动画，或事务处于 Pending/Prepared（保持当前 visual_x/y）。
    Idle,
    /// 事务处于 Rendering/Paused，返回 progress（已 clamp 到 [0,1]）。
    Running(f64),
    /// 事务已完成或不存在，光标应落到 target。
    Finished,
    /// Issue #702 评论 5707770318: 正文协同光标帧。
    ///
    /// 有活跃正文事务时由 `compute_coordinated_cursor_position()` 计算的
    /// 本帧光标位置。携带 `(x, y, h)` 三元组，供 `qquickitem_impl` 同步
    /// `cursor_ctrl.visual_x/visual_y/visual_h` 到屏幕真正画出的位置。
    ///
    /// 关键语义：此变体**不走** `CursorAnimationState` 的独立 timeline。
    /// `qquickitem_impl` 收到 `Coordinated` 时只同步 visual 位置，
    /// 不启动 `started_at`，并清除残留的纯光标 animation。
    /// 正文光标只由 `compute_coordinated_cursor_position()` 驱动。
    Coordinated { x: f64, y: f64, h: f64 },
}

#[derive(Clone, Debug)]
pub(crate) struct RenderPlan {
    pub text_animation: TextAnimationPlan,
    pub selection_preedit: SelectionPreeditPlan,
    /// Issue #679 评论 5657313927: 改为纯显示数据 CursorRenderState，
    /// 不再携带 CursorAnimationPlan（Snap/Tween/driver 由 GUI 线程消费）。
    pub cursor: CursorRenderState,
    pub frame_context: FrameContext,
    pub cursor_style: CursorStyle,
    /// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
    pub selection_preedit_style: SelectionPreeditStyle,
    /// 动画期间静态正文层需要隐藏的区域。
    /// 由 active transaction 的 static_patches 提供，包含精确的行级裁剪信息。
    pub static_patches: Vec<StaticLinePatch>,
    /// Issue #701 评论 5699573227 第三阶段 (F5): 光标 frame state 采样结果。
    pub cursor_sample_outcome: CursorSampleOutcome,
}
