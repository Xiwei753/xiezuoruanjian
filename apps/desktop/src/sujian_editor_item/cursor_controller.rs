// cursor_controller — Isolated cursor visual state
// =============================================================================
// Extracted from SujianEditorItem to enforce the boundary:
// cursor visual position / IME / animation must NOT directly touch
// buffer, layout, or QSG nodes.
//
// The controller only computes target positions and animation state.
// The QML signal emission and inputMethod()->update() are performed
// directly on the GUI thread by the caller (update_cursor_visual_position).

use super::rendering::CursorAnimationState;
use crate::editor::layout::CaretAffinity;

/// Isolated cursor visual state — no buffer, no layout, no QSG.
///
/// Invariants:
/// - `target_cursor_x/y` are the layout-computed cursor positions (document coords).
/// - `visual_x/y` are the positions actually rendered (may lag due to animation).
/// - `CursorUpdateResult.ime_needs_update` indicates when the cursor position
///   changed and IME needs updating (performed on the GUI thread by the caller).
pub struct CursorController {
    // Target position (from layout)
    pub target_x: f64,
    pub target_y: f64,

    // Visual position (what's actually rendered, may be mid-animation)
    pub visual_x: f64,
    pub visual_y: f64,
    pub visual_h: f64,

    // Visibility
    pub visible: bool,
    pub dirty: bool,

    // Affinity
    pub affinity: CaretAffinity,

    // Visual line tracking
    pub current_visual_line_id: Option<usize>,
    pub last_scroll_y: f64,

    // IME
    pub ime_cursor_rect_h: f64,

    // Animation
    pub animation: Option<CursorAnimationState>,

    // Snap control
    pub force_snap_next: bool,
}

impl CursorController {
    pub fn new() -> Self {
        Self {
            target_x: 0.0,
            target_y: 0.0,
            visual_x: 0.0,
            visual_y: 0.0,
            visual_h: 0.0,
            visible: false,
            dirty: false,
            affinity: CaretAffinity::Downstream,
            current_visual_line_id: None,
            last_scroll_y: 0.0,
            ime_cursor_rect_h: 0.0,

            animation: None,
            force_snap_next: false,
        }
    }

    /// Update cursor visual state from layout-computed position.
    ///
    /// Returns `CursorUpdateResult` indicating what deferred work the caller
    /// needs to do on the GUI thread.
    ///
    /// **IMPORTANT**: This method MUST only be called from the GUI thread.
    /// It must NOT touch any GUI objects (signals, inputMethod, QML bindings).
    pub fn update(
        &mut self,
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        visual_line_id: usize,
        scroll_y: f64,
        smooth_enabled: bool,
        smooth_duration_ms: u32,
        is_scrolling: bool,
        is_selecting: bool,
        is_preediting: bool,
        editor_enabled: bool,
        has_selection: bool,
        viewport_height: f64,
    ) -> CursorUpdateResult {
        use std::time::Instant;

        let old_x = self.target_x;
        let old_y = self.target_y;
        let old_visible = self.visible;

        self.target_x = cursor_x;
        self.target_y = cursor_y;
        self.visual_h = cursor_h;
        self.ime_cursor_rect_h = cursor_h;
        self.current_visual_line_id = Some(visual_line_id);

        let scroll_changed = (self.last_scroll_y - scroll_y).abs() > 0.01;
        self.last_scroll_y = scroll_y;

        let position_changed = (old_x - cursor_x).abs() > 0.01 || (old_y - cursor_y).abs() > 0.01;

        // Determine visibility
        let in_viewport = cursor_y + cursor_h > 0.0 && cursor_y < viewport_height;
        let new_visible = editor_enabled && !has_selection && in_viewport && !is_scrolling;
        self.visible = new_visible;

        if !new_visible {
            self.animation = None;
            self.visual_x = cursor_x;
            self.visual_y = cursor_y;
            if old_visible {
                self.dirty = true;
            }
            return CursorUpdateResult {
                ime_needs_update: position_changed,
                needs_repaint: old_visible,
            };
        }

        // Determine if we should snap (no animation)
        let should_snap = is_scrolling
            || is_selecting
            || is_preediting
            || !old_visible
            || self.force_snap_next
            || scroll_changed;

        // Large-distance snap: if the cursor moved more than 80px or more
        // than one line height, snap immediately instead of animating.
        // Only small same-line movements (keyboard left/right) should animate.
        let dx = (cursor_x - self.visual_x).abs();
        let dy = (cursor_y - self.visual_y).abs();
        let large_distance = dx > 80.0 || dy > 80.0;

        let now = Instant::now();

        // Respect user setting for smooth cursor duration (30~1000ms).
        // Core already validates the range; client must not silently override.
        let effective_duration_ms = if smooth_enabled {
            (smooth_duration_ms as u64).clamp(30, 1000)
        } else {
            0
        };

        let (visual_x, visual_y, new_animation) = if should_snap
            || !smooth_enabled
            || large_distance
        {
            (cursor_x, cursor_y, None)
        } else if let Some(ref anim) = self.animation {
            if (anim.target_x - cursor_x).abs() > 0.01 || (anim.target_y - cursor_y).abs() > 0.01 {
                let (cur_x, cur_y) = anim.current_position(now);
                let new_anim = CursorAnimationState {
                    start_x: cur_x,
                    start_y: cur_y,
                    target_x: cursor_x,
                    target_y: cursor_y,
                    start_time: now,
                    duration_ms: effective_duration_ms,
                };
                (cur_x, cur_y, Some(new_anim))
            } else if anim.is_finished(now) {
                (anim.target_x, anim.target_y, None)
            } else {
                let (cur_x, cur_y) = anim.current_position(now);
                (cur_x, cur_y, Some(anim.clone()))
            }
        } else {
            let prev_vx = self.visual_x;
            let prev_vy = self.visual_y;
            if (prev_vx - cursor_x).abs() > 0.01 || (prev_vy - cursor_y).abs() > 0.01 {
                let new_anim = CursorAnimationState {
                    start_x: prev_vx,
                    start_y: prev_vy,
                    target_x: cursor_x,
                    target_y: cursor_y,
                    start_time: now,
                    duration_ms: effective_duration_ms,
                };
                (prev_vx, prev_vy, Some(new_anim))
            } else {
                (cursor_x, cursor_y, None)
            }
        };

        self.animation = new_animation;
        self.visual_x = visual_x;
        self.visual_y = visual_y;
        self.force_snap_next = false;

        let pos_changed =
            (visual_x - old_x).abs() > 0.01 || (visual_y - old_y).abs() > 0.01 || !old_visible;
        if pos_changed {
            self.dirty = true;
        }

        CursorUpdateResult {
            ime_needs_update: position_changed,
            needs_repaint: pos_changed || self.animation.is_some(),
        }
    }

    /// Advance cursor animation by one frame.
    /// Returns true if the animation is still active (needs another frame).
    pub fn tick_animation(&mut self) -> bool {
        use std::time::Instant;
        let now = Instant::now();
        if let Some(ref anim) = self.animation {
            if anim.is_finished(now) {
                self.visual_x = anim.target_x;
                self.visual_y = anim.target_y;
                self.animation = None;
                self.dirty = false;
                false
            } else {
                let (cx, cy) = anim.current_position(now);
                self.visual_x = cx;
                self.visual_y = cy;
                true
            }
        } else {
            self.dirty = false;
            false
        }
    }
}

/// Result of a cursor update — tells the caller what deferred
/// GUI-thread work is needed.
pub struct CursorUpdateResult {
    /// True if the cursor position changed and IME needs updating
    /// (performed directly on the GUI thread by the caller).
    pub ime_needs_update: bool,
    /// True if a repaint is needed.
    pub needs_repaint: bool,
}
