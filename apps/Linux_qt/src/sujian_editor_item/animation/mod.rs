//! Issue #901：正文视觉过渡已迁移至 `visual_canvas` 模块。
//!
//! 本模块只保留 composition 临时显示层处理和动画开关定义。旧的
//! `VisualEditState` / `LinuxEditorAnimationCoordinator` / `VisualFrame` /
//! `RenderOwnershipPlan` / `EditVisualTimeline` / `render_plan_builder` 已被
//! `visual_canvas` 下的 `PresentedScene` / `CanvasController` / `CanvasDrawPlan` /
//! `RevisionChain` / `SceneTransition` 替代并删除。

pub(crate) mod composition;

/// Coordinated mode enables both tracks and lets compatible text reveal/conceal
/// boundaries follow the sampled visual caret in the same RenderPlan. Other
/// transitions keep their explicit visual canvas geometry. Outside coordinated
/// mode, each user toggle controls its own track independently.
pub(crate) const fn text_animation_enabled(typing: bool, coordinated: bool) -> bool {
    coordinated || typing
}

pub(crate) const fn cursor_animation_enabled(smooth_cursor: bool, coordinated: bool) -> bool {
    coordinated || smooth_cursor
}

pub(crate) const fn any_animation_enabled(
    typing: bool,
    smooth_cursor: bool,
    coordinated: bool,
) -> bool {
    text_animation_enabled(typing, coordinated)
        || cursor_animation_enabled(smooth_cursor, coordinated)
}
