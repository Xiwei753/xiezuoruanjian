//! Issue #826: Linux_Qt 正文动画。
//!
//! 四层互相独立，互不拥有对方状态：
//!
//! 1. **canonical 正文** — Core 立即提交，永远是最新真实内容。
//! 2. [`edit_frontier`] — 唯一的遮罩前沿，控制「本轮改掉的字现在露出多少」。
//! 3. [`reflow_motion`] — 独立的 Reflow 层，控制「没改的字移动到哪」。
//! 4. 光标 — 只管视觉 Tween（`cursor_animation.rs` / `cursor_controller.rs`）。
//!
//! IME preedit 是独立临时显示层（`ime_visual.rs` + `SelectionPreeditPlan`），
//! 不进 EditFrontier，不 carry/rebase preedit glyph。

pub(crate) mod composition;
pub(crate) mod coordinator;
pub(crate) mod edit_frontier;
/// Issue #826 评论 2：独立的 Reflow 层——没被改、但因换行/插入/删除而移动的文字。
pub(crate) mod reflow_motion;
pub(crate) mod render_plan_builder;

pub(crate) use coordinator::{
    blink_mode_for_frontier, CursorMoveInputs, EditFrontierRequest, LinuxEditorAnimationCoordinator,
};
#[allow(unused_imports)]
pub(crate) use edit_frontier::{
    EditFrontierKind, EditFrontierSample, EditFrontierState, FrontierGlyph, FrontierRect,
};
#[allow(unused_imports)]
pub(crate) use reflow_motion::{ReflowSpan, ReflowSpanFrame, ReflowState};
