//! Linux Qt 正文过渡只持有一份 `VisualEditState`，从成功提交的 `VisualFrame`
//! 到最新 canonical layout 生成唯一 `RenderOwnershipPlan`。光标仍由唯一
//! `CursorController` 管理；协同模式只把同帧视觉 caret 几何传给正文裁切计划。

pub(crate) mod composition;
pub(crate) mod coordinator;
pub(crate) mod render_plan_builder;
pub(crate) mod visual_edit_state;
pub(crate) mod visual_frame;

/// Coordinated mode enables both tracks and lets compatible text reveal/conceal
/// boundaries follow the sampled visual caret in the same RenderPlan. Other
/// transitions keep their explicit VisualEditState geometry. Outside coordinated
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

pub(crate) use coordinator::{
    blink_mode_for_text_animation, CursorMoveInputs, LinuxEditorAnimationCoordinator,
    VisualEditRequest,
};
