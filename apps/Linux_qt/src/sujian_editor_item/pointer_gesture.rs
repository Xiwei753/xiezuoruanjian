//! Issue #819 评论 5956495850 第 6 节：左键指针手势状态机。
//!
//! 这是左键 press / move / release / long-press 的唯一 owner。
//! `pointer_drag_selecting` 和 `selection_gesture_active` 不再由 QML 和 Rust
//! 两边分别修改，全部收进这个状态机。
//!
//! 状态转换图：
//! ```text
//! Idle ──press──► Pressed { origin, hit_index, started_at }
//! Pressed ──move(超过拖动阈值)──► DragSelecting { anchor }
//! Pressed ──activate_long_press──► LongPressSelecting { anchor }
//! DragSelecting ──move──► DragSelecting { anchor }   (持续扩选)
//! LongPressSelecting ──move──► LongPressSelecting { anchor } (从 anchor 持续扩选)
//! DragSelecting/LongPressSelecting/Pressed ──release──► Idle
//! 任意状态 ──cancel──► Idle
//! ```
//!
//! 设计约束（Issue #819 评论第 6 节）：
//! - 状态机内部维护 `pointer_drag_selecting` 和 `selection_gesture_active`
//!   两个对外可见的布尔值，render_plan_builder / rendering 只读这两个值。
//! - `selection_gesture_active` 覆盖整个选择手势生命周期（press 到 release），
//!   包括长按选词；`pointer_drag_selecting` 只在真正发生拖选（move 超过阈值）
//!   后为 true。
//! - 长按由外部 Timer 在到点时调 `activate_long_press`，状态机不自己计时，
//!   也不接管 pointer grab / MouseMove / Release。

use std::time::Instant;

/// 拖选触发阈值（像素）。press 后移动超过这个距离才进入 DragSelecting，
/// 避免手抖把轻点误判成拖选。
const DRAG_THRESHOLD_PX: f32 = 3.0;

/// 左键指针手势状态。
#[derive(Clone, Debug)]
pub(crate) enum PointerGesturePhase {
    /// 无手势。
    Idle,
    /// 已 press，尚未判定是拖选还是长按。
    Pressed {
        /// press 时的屏幕坐标（用于计算 move 距离）。
        origin: (f32, f32),
        /// press 时的 hit_test 结果（byte index），作为拖选 anchor。
        hit_index: usize,
        /// press 时刻，供外部 Timer 判断长按时延。当前状态机不自己计时，
        /// 保留此字段供未来需要时延判断的场景使用。
        started_at: Instant,
    },
    /// 拖选进行中。anchor 是拖选起点（byte index）。
    DragSelecting { anchor: usize },
    /// 长按选词进行中。anchor 是长按选词起点（byte index）。
    /// 长按激活后继续 move 会从 anchor 持续扩选。
    LongPressSelecting { anchor: usize },
}

/// 左键指针手势状态机。
///
/// 唯一 owner of `pointer_drag_selecting` 和 `selection_gesture_active`。
/// `qquickitem_impl.rs` 的 mouse_event 和 QML Timer 都通过这个状态机驱动
/// 手势生命周期，不再直接修改两个布尔字段。
#[derive(Clone, Debug)]
pub(crate) struct PointerGestureState {
    phase: PointerGesturePhase,
    /// 只在真正发生拖选（move 超过阈值 / 长按激活）后为 true。
    /// 替代旧 `SujianEditorItem::pointer_drag_selecting` 字段。
    pointer_drag_selecting: bool,
    /// 覆盖整个选择手势生命周期：从 press 到 release。
    /// 替代旧 `SujianEditorItem::selection_gesture_active` 字段。
    selection_gesture_active: bool,
}

impl Default for PointerGestureState {
    fn default() -> Self {
        Self {
            phase: PointerGesturePhase::Idle,
            pointer_drag_selecting: false,
            selection_gesture_active: false,
        }
    }
}

impl PointerGestureState {
    /// 当前是否处于拖选状态（move 超过阈值后）。
    pub(crate) fn pointer_drag_selecting(&self) -> bool {
        self.pointer_drag_selecting
    }

    /// 当前是否处于选择手势生命周期内（press 到 release 之间）。
    pub(crate) fn selection_gesture_active(&self) -> bool {
        self.selection_gesture_active
    }

    /// 当前是否处于长按选词状态。
    pub(crate) fn is_long_press_selecting(&self) -> bool {
        matches!(self.phase, PointerGesturePhase::LongPressSelecting { .. })
    }

    /// 当前是否处于 DragSelecting 或 LongPressSelecting（需要持续扩选）。
    pub(crate) fn is_selecting(&self) -> bool {
        matches!(
            self.phase,
            PointerGesturePhase::DragSelecting { .. }
                | PointerGesturePhase::LongPressSelecting { .. }
        )
    }

    /// 取长按/拖选的 anchor（byte index）。非选择状态返回 None。
    pub(crate) fn selection_anchor(&self) -> Option<usize> {
        match self.phase {
            PointerGesturePhase::DragSelecting { anchor }
            | PointerGesturePhase::LongPressSelecting { anchor } => Some(anchor),
            _ => None,
        }
    }

    /// 左键 press。开始一个新手势窗口，清除上一轮手势的残留状态。
    ///
    /// `hit_index` 是 press 位置 hit_test 得到的 byte index，作为潜在拖选 anchor。
    /// 调用方负责在外部先 click_at 设置 cursor，状态机不碰 Core 选区。
    pub(crate) fn press(&mut self, origin: (f32, f32), hit_index: usize, now: Instant) {
        // Issue #810 评论 问题2: press 开始一个新指针手势窗口。
        // 旧的拖选/选择手势状态必须清除，避免上一轮手势的 Snap/隐藏
        // 状态污染本次点击。
        self.phase = PointerGesturePhase::Pressed {
            origin,
            hit_index,
            started_at: now,
        };
        // press 即进入选择手势生命周期（selection_gesture_active = true），
        // 让 render_plan_builder 在 press 后立即走 hard_snap，避免 press→move
        // 之间的一帧用 has_selection 代替手势状态导致光标跳动。
        // 但 pointer_drag_selecting 仍为 false，直到 move 超过阈值。
        self.pointer_drag_selecting = false;
        self.selection_gesture_active = true;
    }

    /// 左键 move。超过拖动阈值后进入 DragSelecting；已在 DragSelecting /
    /// LongPressSelecting 时保持原状态（调用方继续 drag_select_at 扩选）。
    ///
    /// 返回值指示本次 move 是否新进入了 DragSelecting（调用方据此决定是否
    /// 第一次调 drag_select_at 设置 anchor）。已在选择状态时返回 false，
    /// 调用方仍应继续调 drag_select_at 持续扩选。
    pub(crate) fn move_pos(&mut self, pos: (f32, f32)) -> MoveOutcome {
        match self.phase {
            PointerGesturePhase::Pressed {
                origin, hit_index, ..
            } => {
                let dx = pos.0 - origin.0;
                let dy = pos.1 - origin.1;
                if dx * dx + dy * dy >= DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX {
                    // 超过阈值，进入 DragSelecting。
                    self.phase = PointerGesturePhase::DragSelecting { anchor: hit_index };
                    self.pointer_drag_selecting = true;
                    self.selection_gesture_active = true;
                    MoveOutcome::StartedDragSelect { anchor: hit_index }
                } else {
                    // 仍在 press 窗口内，未超过阈值。
                    MoveOutcome::StillPressed
                }
            }
            PointerGesturePhase::DragSelecting { anchor } => {
                // 已在拖选，保持状态，调用方继续 drag_select_at。
                MoveOutcome::ContinueDragSelect { anchor }
            }
            PointerGesturePhase::LongPressSelecting { anchor } => {
                // Issue #819 评论第 6 节：LongPressSelecting 后继续 Move，
                // 从当前 anchor 持续扩选。状态保持 LongPressSelecting，
                // 调用方用 drag_select_at 从 anchor 扩选。
                MoveOutcome::ContinueLongPressSelect { anchor }
            }
            PointerGesturePhase::Idle => {
                // 没有 press 就 move，忽略（理论上不应发生）。
                MoveOutcome::Ignored
            }
        }
    }

    /// 长按激活。由外部 QML Timer 在到点时调用（通过 Rust 的
    /// `activate_pointer_long_press` qt_method）。
    ///
    /// 只有处于 Pressed 状态时才生效；已进入 DragSelecting 则忽略
    /// （拖选优先于长按）。激活后进入 LongPressSelecting，调用方据此
    /// 调 long_press_at 选词。
    ///
    /// 返回 true 表示成功激活（调用方应继续调 long_press_at），
    /// false 表示当前状态不接受长按激活。
    pub(crate) fn activate_long_press(&mut self) -> bool {
        match self.phase {
            PointerGesturePhase::Pressed { hit_index, .. } => {
                self.phase = PointerGesturePhase::LongPressSelecting { anchor: hit_index };
                // 长按是明确的选择手势，pointer_drag_selecting 也置 true，
                // 与旧 begin_selection_gesture + long_press_at 行为一致。
                self.pointer_drag_selecting = true;
                self.selection_gesture_active = true;
                true
            }
            // 已在拖选/长按/Idle 时不激活。
            _ => false,
        }
    }

    /// 左键 release。统一结束选择手势，回到 Idle。
    ///
    /// 调用方负责在外部调 cursor_ctrl.record_selection_head_rect() 等
    /// 手势结束收尾；状态机只管自己的 phase 和两个布尔字段。
    pub(crate) fn release(&mut self) {
        self.phase = PointerGesturePhase::Idle;
        self.pointer_drag_selecting = false;
        self.selection_gesture_active = false;
    }

    /// 取消手势（失焦、cancel 事件等）。与 release 等价，回到 Idle。
    pub(crate) fn cancel(&mut self) {
        self.phase = PointerGesturePhase::Idle;
        self.pointer_drag_selecting = false;
        self.selection_gesture_active = false;
    }
}

/// `move_pos` 的返回值，指示调用方后续应采取的动作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MoveOutcome {
    /// 仍在 Pressed 窗口内，未超过拖动阈值。调用方无需 drag_select_at。
    StillPressed,
    /// 本次 move 新进入 DragSelecting。调用方应开始 drag_select_at，
    /// 用返回的 anchor 作为拖选起点。
    StartedDragSelect { anchor: usize },
    /// 已在 DragSelecting，继续 drag_select_at 从 anchor 扩选。
    ContinueDragSelect { anchor: usize },
    /// LongPressSelecting 后继续 move，从 anchor 持续扩选。
    ContinueLongPressSelect { anchor: usize },
    /// 没有 press 就 move，忽略。
    Ignored,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn idle_default_flags_false() {
        let s = PointerGestureState::default();
        assert!(!s.pointer_drag_selecting());
        assert!(!s.selection_gesture_active());
        assert!(!s.is_long_press_selecting());
        assert!(!s.is_selecting());
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn press_sets_selection_gesture_active_but_not_drag() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 5, now());
        assert!(!s.pointer_drag_selecting());
        assert!(s.selection_gesture_active());
        assert!(!s.is_selecting());
    }

    #[test]
    fn move_below_threshold_stays_pressed() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 5, now());
        let outcome = s.move_pos((11.0, 20.0));
        assert_eq!(outcome, MoveOutcome::StillPressed);
        assert!(!s.pointer_drag_selecting());
        assert!(s.selection_gesture_active());
    }

    #[test]
    fn move_above_threshold_enters_drag_selecting() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 5, now());
        let outcome = s.move_pos((20.0, 20.0));
        assert_eq!(outcome, MoveOutcome::StartedDragSelect { anchor: 5 });
        assert!(s.pointer_drag_selecting());
        assert!(s.selection_gesture_active());
        assert!(s.is_selecting());
        assert_eq!(s.selection_anchor(), Some(5));
    }

    #[test]
    fn move_after_drag_select_continues() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 5, now());
        let _ = s.move_pos((20.0, 20.0));
        let outcome = s.move_pos((30.0, 20.0));
        assert_eq!(outcome, MoveOutcome::ContinueDragSelect { anchor: 5 });
        assert!(s.pointer_drag_selecting());
    }

    #[test]
    fn activate_long_press_from_pressed() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 7, now());
        let activated = s.activate_long_press();
        assert!(activated);
        assert!(s.is_long_press_selecting());
        assert!(s.pointer_drag_selecting());
        assert!(s.selection_gesture_active());
        assert_eq!(s.selection_anchor(), Some(7));
    }

    #[test]
    fn activate_long_press_from_idle_is_noop() {
        let mut s = PointerGestureState::default();
        let activated = s.activate_long_press();
        assert!(!activated);
        assert!(!s.is_long_press_selecting());
    }

    #[test]
    fn activate_long_press_after_drag_select_is_noop() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 7, now());
        let _ = s.move_pos((20.0, 20.0));
        let activated = s.activate_long_press();
        assert!(!activated);
        assert!(!s.is_long_press_selecting());
    }

    #[test]
    fn move_after_long_press_continues_long_press_select() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 7, now());
        let _ = s.activate_long_press();
        let outcome = s.move_pos((15.0, 25.0));
        assert_eq!(outcome, MoveOutcome::ContinueLongPressSelect { anchor: 7 });
        assert!(s.is_long_press_selecting());
    }

    #[test]
    fn release_clears_everything() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 5, now());
        let _ = s.move_pos((20.0, 20.0));
        s.release();
        assert!(!s.pointer_drag_selecting());
        assert!(!s.selection_gesture_active());
        assert!(!s.is_selecting());
        assert_eq!(s.selection_anchor(), None);
    }

    #[test]
    fn cancel_clears_everything() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 5, now());
        s.cancel();
        assert!(!s.pointer_drag_selecting());
        assert!(!s.selection_gesture_active());
        assert!(!s.is_selecting());
    }

    #[test]
    fn press_resets_previous_gesture() {
        let mut s = PointerGestureState::default();
        s.press((10.0, 20.0), 5, now());
        let _ = s.move_pos((20.0, 20.0));
        assert!(s.pointer_drag_selecting());
        // 新 press 应清除上一轮手势。
        s.press((100.0, 100.0), 42, now());
        assert!(!s.pointer_drag_selecting());
        assert!(s.selection_gesture_active());
    }
}
