//! Issue #826: Linux_Qt 正文动画。
//!
//! 四层互相独立，互不拥有对方状态：
//!
//! 1. **canonical 正文** — Core 立即提交，永远是最新真实内容。
//! 2. [`edit_frontier`] — 唯一的遮罩前沿，控制「本轮改掉的字现在露出多少」。
//! 3. [`reflow_motion`] — 独立的 Reflow 层，控制「没改的字移动到哪」。
//! 4. [`shaping_transition`] — 不可拆 shaping cluster 的 old/new 原子交接。
//!    **底层规则：Core range 可以按字符切；Qt 视觉 owner 绝不能切开 shaping
//!    cluster。** 前三层各自收到的 range 都必须先经过这一层的「整块归属」判定。
//! 5. 光标 — 只管视觉 Tween（`cursor_animation.rs` / `cursor_controller.rs`）。
//!
//! IME preedit 是独立临时显示层（`ime_visual.rs` + `SelectionPreeditPlan`），
//! 不进 EditFrontier，不 carry/rebase preedit glyph。

pub(crate) mod composition;
pub(crate) mod coordinator;
pub(crate) mod edit_frontier;
/// Issue #826 评论 2：独立的 Reflow 层——没被改、但因换行/插入/删除而移动的文字。
pub(crate) mod reflow_motion;
pub(crate) mod render_plan_builder;
/// Issue #826 评论 24：不可拆 shaping cluster 的原子视觉交接。
pub(crate) mod shaping_transition;

pub(crate) use coordinator::{
    blink_mode_for_frontier, CursorMoveInputs, EditFrontierRequest, LinuxEditorAnimationCoordinator,
};
#[allow(unused_imports)]
pub(crate) use edit_frontier::{
    ConcealSourceLine, EditFrontierKind, EditFrontierSample, EditFrontierState, FrontierGlyph,
    FrontierRect,
};
#[allow(unused_imports)]
pub(crate) use reflow_motion::{ReflowSpan, ReflowSpanFrame, ReflowState};
#[allow(unused_imports)]
pub(crate) use shaping_transition::{
    visible_source_slice, CurrentVisualCluster, ShapingTransitionFrame, ShapingTransitionGroup,
    ShapingTransitionSide, ShapingTransitionState, VisualClusterAtom,
};
