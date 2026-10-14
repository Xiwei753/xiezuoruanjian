//! Issue #904：正文视觉过渡由 `text_motion` 的单一局部动画计划完成。
//!
//! 本模块只保留动画开关定义。旧的 `VisualEditState` / `LinuxEditorAnimationCoordinator` /
//! `VisualFrame` / `RenderOwnershipPlan` / `EditVisualTimeline` / `render_plan_builder`，以及
//! Issue #901 的 `PresentedScene` / `CanvasController` / `CanvasDrawPlan` / `RevisionChain` /
//! `SceneTransition` 状态机都已删除。

/// 正文吞吐动画只由用户的「打字动画」开关决定；协调模式不再强制打开。
pub(crate) const fn text_animation_enabled(typing: bool, _coordinated: bool) -> bool {
    typing
}

/// 光标平滑动画只由用户的「光标动画」开关决定；协调模式不再强制打开。
pub(crate) const fn cursor_animation_enabled(smooth_cursor: bool, _coordinated: bool) -> bool {
    smooth_cursor
}

pub(crate) const fn any_animation_enabled(
    typing: bool,
    smooth_cursor: bool,
    coordinated: bool,
) -> bool {
    text_animation_enabled(typing, coordinated)
        || cursor_animation_enabled(smooth_cursor, coordinated)
}
