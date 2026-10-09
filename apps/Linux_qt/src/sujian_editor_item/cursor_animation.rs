use super::cursor_controller::CursorMoveSource;
use super::edit_motion::CursorRect;
use super::layout_revision::LayoutRevision;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum CursorBlinkMode {
    #[default]
    Normal,
    Suppressed,
}

/// 光标目标由 CursorController 覆盖更新。Tween 自己用 Scene Graph 当前帧推进，
/// 不读取正文 transition 的 progress 或生命周期。
#[derive(Clone, Debug, Default)]
pub(crate) enum CursorTransition {
    #[default]
    Snap,
    Tween {
        old_rect: CursorRect,
        new_rect: CursorRect,
        /// 光标动画时长（毫秒）。CursorAnimationState 用自己的 timeline
        ///（started_at + duration_ms），
        /// 由 Scene Graph 当前帧 frame_now 推进 from→to 动画。
        duration_ms: u64,
    },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CursorAnimationPlan {
    pub should_be_visible: bool,
    pub transition: CursorTransition,
    /// Stable identity of the cursor route that VisualEditState may follow.
    pub movement_source: CursorMoveSource,
    pub driver_revision: Option<LayoutRevision>,
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
