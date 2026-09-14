use writer_core::editor::CursorRect;

use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum CursorBlinkMode {
    #[default]
    Normal,
    Suppressed,
}

/// Issue #679 评论 5657313927: Tween 带上 `driver_key`，让光标动画消费
/// 对应视觉事务的 Timeline progress，不再维护独立时间源。
#[derive(Clone, Debug, Default)]
pub(crate) enum CursorTransition {
    #[default]
    Snap,
    Tween {
        old_rect: CursorRect,
        new_rect: CursorRect,
        driver_key: VisualTransactionKey,
    },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CursorAnimationPlan {
    pub should_be_visible: bool,
    pub transition: CursorTransition,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_h: f64,
}
