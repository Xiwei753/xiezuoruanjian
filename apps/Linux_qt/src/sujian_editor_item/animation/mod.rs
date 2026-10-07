//! Linux Qt 正文过渡只持有一份 `VisualEditState`，从成功提交的 `VisualFrame`
//! 到最新 canonical layout 生成唯一 `RenderOwnershipPlan`。光标与 IME 独立。

pub(crate) mod composition;
pub(crate) mod coordinator;
pub(crate) mod render_plan_builder;
pub(crate) mod visual_edit_state;
pub(crate) mod visual_frame;

pub(crate) use coordinator::{
    blink_mode_for_text_animation, CursorMoveInputs, LinuxEditorAnimationCoordinator,
    VisualEditRequest,
};
