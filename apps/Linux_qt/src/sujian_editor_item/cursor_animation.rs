use super::edit_motion::CursorRect;

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
    /// Issue #712 评论 5739517945: 来自 `CaretRect.baseline_y` 的真实 baseline，
    /// 替代旧的 `cursor_h * 0.8` 估算。由 `build_cursor_plan()` 从
    /// `layout_res.baseline_y` 传入，`apply_plan()` 据此维护 `visual_baseline_y`。
    pub cursor_baseline_y: f64,
    /// Issue #810 评论 问题2: 光标隐藏是否因选区（has_selection）导致。
    ///
    /// `should_be_visible = false` 有多种原因：editor disabled、不在视口、has_selection。
    /// 只有因 has_selection 隐藏时才需要保留 visual rect 供选区收起后恢复 Tween；
    /// 其它原因隐藏时维持原行为（visual 落到 target、清 animation）。
    /// 由 `build_cursor_plan()` 设置：`has_selection && !should_be_visible`。
    pub hidden_by_selection: bool,
}
