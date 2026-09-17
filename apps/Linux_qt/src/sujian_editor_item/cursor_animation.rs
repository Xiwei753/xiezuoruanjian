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
/// Issue #702: `duration_ms` 让纯光标移动（无正文事务）拥有自己的 timeline，
/// 用 Scene Graph 当前帧 frame_now 推进 from→to 动画。
#[derive(Clone, Debug, Default)]
pub(crate) enum CursorTransition {
    #[default]
    Snap,
    Tween {
        old_rect: CursorRect,
        new_rect: CursorRect,
        driver_key: VisualTransactionKey,
        /// Issue #702: 纯光标移动动画时长（毫秒）。
        /// 输入/删除存在正文视觉事务时此字段仍传入，但 progress 优先从
        /// driver 事务的 Timeline 读取；纯方向键/Home/End 等没有正文事务时
        /// 由 CursorAnimationState 自己的 timeline 推进。
        duration_ms: u64,
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
