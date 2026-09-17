use writer_core::editor::CursorRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum CursorBlinkMode {
    #[default]
    Normal,
    Suppressed,
}

/// Issue #679 评论 5657313927: Tween 原先带 `driver_key` 让光标动画消费对应视觉事务的
/// Timeline progress。Issue #702 评论 5707449688 问题 2: 纯光标移动彻底和文字事务 key
/// 解耦，Tween 不再保存 driver_key，`duration_ms` 让 CursorAnimationState 拥有自己的
/// timeline，用 Scene Graph 当前帧 frame_now 推进 from→to 动画。
#[derive(Clone, Debug, Default)]
pub(crate) enum CursorTransition {
    #[default]
    Snap,
    Tween {
        old_rect: CursorRect,
        new_rect: CursorRect,
        /// Issue #702: 纯光标移动动画时长（毫秒）。
        /// CursorAnimationState 用自己的 timeline（started_at + duration_ms），
        /// 由 Scene Graph 当前帧 frame_now 推进 from→to 动画。
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
