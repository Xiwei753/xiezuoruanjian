use super::edit_motion::CursorRect;
use super::layout_revision::LayoutRevision;
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
    /// Issue #738 评论 5787277777: 当前 canonical layout basis revision。
    ///
    /// `build_render_plan_full` 构建 glyph / clip 时只接受已经 reconcile 到这个
    /// revision 的 unit（`tx.layout_basis_revision >= frame_context.layout_basis_revision`）。
    /// 这个守卫放在计划层，避免以后又新增一条入口忘了先做 reconcile，旧绝对坐标
    /// 重新混进 scene graph。
    pub layout_basis_revision: LayoutRevision,
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
/// 协同模式每帧的**唯一** caret 采样。由 `sample_coordinated_motion_frame` 在
/// 同一个 `frame_now` 上采样 owner 事务的 cursor track 一次得到。
///
/// Issue #815 评论 6042062633 修改 3: cursor layer 和文字层（InsertReveal/DeleteConceal
/// 的吞吐 clip）消费的是同一份采样。文字层不准再自己算一次时间，也不准把 caret track
/// 的 progress 换算成独立 0..1 visible fraction。
///
/// `None` 表示本帧无有效 caret motion track（无活跃正文事务 / epoch 不一致 /
/// 无 cursor_visual_track）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SampledCaretFrame {
    /// caret 在文档坐标系的 x（横向裁切边界）。
    pub x: f64,
    /// caret 在文档坐标系的 y（跨行裁切判断）。
    pub y: f64,
    /// caret 所在 visual line id（跨行裁切判断）。
    pub visual_line_id: Option<usize>,
    /// Issue #727 约束 3: caret track 的当前 progress（0..1）。
    pub progress: f64,
    /// Issue #815 评论 6042062633 修改 3: 本帧 caret 的完整 rect（文档坐标）。
    ///
    /// 和 x/y/visual_line_id/progress 出自同一次 track 采样，光标层直接用它画 caret，
    /// 不再自己重新采样一次 track 算位置/高度。
    pub rect: CursorRect,
}

impl Default for SampledCaretFrame {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            visual_line_id: None,
            progress: 0.0,
            rect: CursorRect {
                x: 0.0,
                top: 0.0,
                bottom: 0.0,
                baseline_y: 0.0,
            },
        }
    }
}

/// Issue #815 评论 6042062633 修改 3: 一帧的统一协同运动结果。
///
/// 每帧每笔 owner 事务只采样一次 caret track，得到一份 `SampledCaretFrame`；
/// 这同一份采样同时喂给光标层和文字吞吐层。文字层不得再推导一份 caret，
/// 也不得自己再按 `frame_now` 算一次时间。
///
/// `owner_key` 标记这份采样属于哪笔事务：只有同 key 的 `CaretTrack` unit 才能消费。
/// 其它失去 ownership 的 `CaretTrack` unit 立刻收口到终态，只允许 `Timed` Reflow
/// 继续播完。避免 editor.anim.keep 保留旧事务时旧事务消费新事务的 caret。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CoordinatedMotionFrame {
    /// Issue #727 问题5: owner tx key; only same-key CaretDriven unit consumes.
    pub owner_key: Option<VisualTransactionKey>,
    /// 本帧采样的 caret geometry。`None` 表示无有效 caret motion track。
    pub caret: Option<SampledCaretFrame>,
}

#[derive(Clone, Debug, Default)]
/// Issue #707 评论 5723616999: 改 `pub` 让集成测试能访问 `drawn_caret_rect` 字段。
/// 加 `Default` 让集成测试能构造实例验证字段可读写。
pub struct RenderPlan {
    // 除 drawn_caret_rect 外全部收回 pub(crate)：字段类型都是平台端内部渲染
    // 状态（TextAnimationPlan / CursorRenderState / FrameContext 等），
    // 集成测试只需要 drawn_caret_rect 这个 (x, y, h) 裸元组。
    //
    // Issue #727 约束 3 要求的"本帧统一采样的协同运动帧"不再挂在 RenderPlan 上：
    // build_render_plan_full 在入口处 sample 一次后全程用局部变量消费
    // （caret.is_some() 门禁 + owner_key 过滤），挂到帧上后没有任何读者，
    // 只是一份和局部变量重复的副本，所以这里不再冗余存一份。
    pub(crate) text_animation: TextAnimationPlan,
    pub(crate) selection_preedit: SelectionPreeditPlan,
    /// Issue #679 评论 5657313927: 改为纯显示数据 CursorRenderState，
    /// 不再携带 CursorAnimationPlan（Snap/Tween/driver 由 GUI 线程消费）。
    pub(crate) cursor: CursorRenderState,
    pub(crate) frame_context: FrameContext,
    pub(crate) cursor_style: CursorStyle,
    /// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
    pub(crate) selection_preedit_style: SelectionPreeditStyle,
    /// Issue #727 评论 5755858583 问题2: 动画期间静态正文层需要隐藏的裁剪矩形。
    /// 直接存储文档坐标 x/y/w/h，由 active units 的 AnimatedSlice.static_hidden_document_rects
    /// 收集而来。不再通过 StaticLinePatch 中间结构。
    pub(crate) clip_rects: Vec<AnimationClipRect>,
    /// Issue #701 评论 5699573227 第三阶段 (F5): 光标 frame state 采样结果。
    pub(crate) cursor_sample_outcome: CursorSampleOutcome,
    /// Issue #705: 本帧真正绘制出去的 caret rect `(x, y, h)`。
    ///
    /// 正文协同动画时,这个 rect 就是同帧文字事务算出的实际光标位置;
    /// 纯光标动画时,就是该 Tween 本帧位置;Snap 时就是目标位置。
    /// `qquickitem_impl` 每帧生成 RenderPlan 后,把
    /// `cursor_ctrl.visual_x/visual_y/visual_h` 同步成此值。下一次
    /// 输入、删除、鼠标点击创建新事务时,只允许从这个"上一帧真正
    /// 画出来的位置" rebase。
    pub drawn_caret_rect: Option<(f64, f64, f64)>,
}
