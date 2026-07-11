use writer_core::editor::CursorRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorBlinkMode {
    Normal,
    Suppressed,
}

#[derive(Clone, Debug)]
pub(crate) enum CursorTransition {
    Snap,
    Tween {
        old_rect: CursorRect,
        new_rect: CursorRect,
        duration_ms: u64,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct CursorAnimationPlan {
    pub should_be_visible: bool,
    pub blink_mode: CursorBlinkMode,
    pub transition: CursorTransition,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_h: f64,
}
