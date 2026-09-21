use super::layout_snapshot::{LineSnapshotId, SourceRect};
use super::qt_text_node::AnimationClipRect;
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

/// Issue #707 评论 5723616999: Default 实现 — Idle 是自然默认值。
impl Default for CursorSampleOutcome {
    fn default() -> Self {
        Self::Idle
    }
}

/// Issue #727 评论 5754041813 约束 3: 一帧采样的 caret geometry。
///
/// 由 `build_render_plan_full` 在入口处统一采样一次，供 cursor layer 和文字
/// reveal/conceal 共享同一份 caret geometry。文字层不再自己重新采样 caret。
///
/// `None` 表示本帧无有效 caret motion track（无活跃正文事务 / epoch 不一致 /
/// 无 cursor_visual_track），InsertReveal / DeleteConceal 不应生成动画 glyph，
/// static canonical text 直接完整显示。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SampledCaretFrame {
    /// caret 在文档坐标系的 x（横向裁切边界）。
    pub x: f64,
    /// caret 在文档坐标系的 y（跨行裁切判断）。
    pub y: f64,
    /// caret 所在 visual line id（跨行裁切判断）。
    pub visual_line_id: Option<usize>,
    /// Issue #727 约束 3: caret track 的当前 progress（0..1）。
    /// CaretDriven unit 的可见比例从这里推导，不再由 unit 自己的时间线驱动。
    pub progress: f64,
}

impl Default for SampledCaretFrame {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            visual_line_id: None,
            progress: 0.0,
        }
    }
}

/// Issue #727 评论 5754041813 约束 3: 一帧的统一协同运动结果。
///
/// `build_render_plan_full` 入口处先采样 caret motion 得到一份 `SampledCaretFrame`，
/// 有才让 InsertReveal / DeleteConceal 用它的 x/y/visual_line_id 裁文字。
/// 没有 caret frame 就不生成 reveal/conceal glyph，static canonical text 直接完整显示。
/// RenderPlan 里同一份 `SampledCaretFrame` 同时喂 cursor layer 和文字 reveal/conceal，
/// 不能文字自己再推导一份 caret。
///
/// Issue #727 评论 5757225958 问题5: 携带 `owner_key` 字段，只有同 key 的
/// CaretDriven unit 能消费此 caret frame。其它失去 ownership 的 CaretDriven unit
/// 直接回 canonical，只允许 Timed Reflow 继续。避免 editor.anim.keep 保留旧事务时
/// 旧 CaretDriven unit 消费新事务的 caret。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CoordinatedMotionFrame {
    /// Issue #727 问题5: owner tx key; only same-key CaretDriven unit consumes.
    pub owner_key: Option<VisualTransactionKey>,
    /// 本帧采样的 caret geometry。`None` 表示无有效 caret motion track。
    pub caret: Option<SampledCaretFrame>,
}

impl CoordinatedMotionFrame {
    /// Issue #727 评论 5757225958 问题5: 构造带 owner_key 的 frame。
    #[cfg(test)]
    pub fn with_owner_key(
        owner_key: Option<VisualTransactionKey>,
        caret: Option<SampledCaretFrame>,
    ) -> Self {
        Self { owner_key, caret }
    }
}

#[derive(Clone, Debug, Default)]
/// Issue #707 评论 5723616999: 改 `pub` 让集成测试能访问 `drawn_caret_rect` 字段。
/// 加 `Default` 让集成测试能构造实例验证字段可读写。
pub struct RenderPlan {
    pub text_animation: TextAnimationPlan,
    pub selection_preedit: SelectionPreeditPlan,
    /// Issue #679 评论 5657313927: 改为纯显示数据 CursorRenderState，
    /// 不再携带 CursorAnimationPlan（Snap/Tween/driver 由 GUI 线程消费）。
    pub cursor: CursorRenderState,
    pub frame_context: FrameContext,
    pub cursor_style: CursorStyle,
    /// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
    pub selection_preedit_style: SelectionPreeditStyle,
    /// Issue #727 评论 5755858583 问题2: 动画期间静态正文层需要隐藏的裁剪矩形。
    /// 直接存储文档坐标 x/y/w/h，由 active units 的 AnimatedSlice.static_hidden_document_rects
    /// 收集而来。不再通过 StaticLinePatch 中间结构。
    pub clip_rects: Vec<AnimationClipRect>,
    /// Issue #701 评论 5699573227 第三阶段 (F5): 光标 frame state 采样结果。
    pub cursor_sample_outcome: CursorSampleOutcome,
    /// Issue #705: 本帧真正绘制出去的 caret rect `(x, y, h)`。
    ///
    /// 正文协同动画时,这个 rect 就是同帧文字事务算出的实际光标位置;
    /// 纯光标动画时,就是该 Tween 本帧位置;Snap 时就是目标位置。
    /// `qquickitem_impl` 每帧生成 RenderPlan 后,把
    /// `cursor_ctrl.visual_x/visual_y/visual_h` 同步成此值。下一次
    /// 输入、删除、鼠标点击创建新事务时,只允许从这个"上一帧真正
    /// 画出来的位置" rebase。
    pub drawn_caret_rect: Option<(f64, f64, f64)>,
    /// Issue #727 约束 3: 本帧统一采样的协同运动帧。
    ///
    /// 由 `build_render_plan_full` 入口处采样一次，供 cursor layer 和文字
    /// reveal/conceal 共享同一份 caret geometry。`caret` 为 `None` 时
    /// InsertReveal / DeleteConceal 不生成动画 glyph。
    pub coordinated_motion_frame: CoordinatedMotionFrame,
}
