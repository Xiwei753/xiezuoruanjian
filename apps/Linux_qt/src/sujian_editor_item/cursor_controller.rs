use super::animation_coordinator::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::rendering::CursorAnimationState;
use crate::editor::layout::CaretAffinity;
use std::time::{Duration, Instant};

const BLINK_INTERVAL_MS: u64 = 530;

pub struct CursorController {
    pub target_x: f64,
    pub target_y: f64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub visual_h: f64,
    pub visible: bool,
    pub dirty: bool,
    pub affinity: CaretAffinity,
    pub current_visual_line_id: Option<usize>,
    pub last_scroll_y: f64,
    pub ime_cursor_rect_h: f64,
    pub anchor_visual_x: Option<f64>,
    pub anchor_visual_y: Option<f64>,
    pub animation: Option<CursorAnimationState>,
    pub force_snap_next: bool,
    pub blink_visible: bool,
    pub blink_last_toggle: Instant,
    pub blink_reset_requested: bool,
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
            anchor_visual_x: None,
            anchor_visual_y: None,
            animation: None,
            force_snap_next: false,
            blink_visible: true,
            blink_last_toggle: Instant::now(),
            blink_reset_requested: false,
        }
    }

    pub fn cursor_should_be_visible(&self) -> bool {
        self.visible
    }

    pub fn cursor_blink_opacity(&self, blink_mode: CursorBlinkMode) -> f64 {
        if !self.visible {
            return 0.0;
        }
        if blink_mode == CursorBlinkMode::Suppressed {
            return 1.0;
        }
        if self.blink_visible {
            1.0
        } else {
            0.0
        }
    }

    pub fn apply_plan(&mut self, plan: &CursorAnimationPlan) -> CursorUpdateResult {
        let old_x = self.target_x;
        let old_y = self.target_y;
        let old_visible = self.visible;
        let old_blink_visible = self.blink_visible;

        self.target_x = plan.cursor_x;
        self.target_y = plan.cursor_y;
        self.visual_h = plan.cursor_h;
        self.ime_cursor_rect_h = plan.cursor_h;
        self.visible = plan.should_be_visible;

        let visibility_changed = old_visible != plan.should_be_visible;

        if !plan.should_be_visible {
            self.animation = None;
            self.visual_x = plan.cursor_x;
            self.visual_y = plan.cursor_y;
            self.blink_visible = true;
            if old_visible {
                self.dirty = true;
            }
            let position_changed =
                (old_x - plan.cursor_x).abs() > 0.01 || (old_y - plan.cursor_y).abs() > 0.01;
            return CursorUpdateResult {
                ime_needs_update: position_changed,
                needs_repaint: old_visible,
                visibility_changed,
                blink_changed: old_blink_visible != self.blink_visible,
                visual_position_changed: position_changed,
            };
        }

        match &plan.transition {
            CursorTransition::Snap => {
                self.visual_x = plan.cursor_x;
                self.visual_y = plan.cursor_y;
                self.animation = None;
            }
            CursorTransition::Tween {
                old_rect,
                new_rect,
                duration_ms: _,
            } => {
                let start_x = old_rect.x;
                let start_y = old_rect.top;
                let target_x = new_rect.x;
                let target_y = new_rect.top;

                if let Some(ref anim) = self.animation {
                    if (anim.target_x - target_x).abs() > 0.01
                        || (anim.target_y - target_y).abs() > 0.01
                    {
                        let (cur_x, cur_y) = anim.current_position();
                        self.animation = Some(CursorAnimationState {
                            start_x: cur_x,
                            start_y: cur_y,
                            target_x,
                            target_y,
                            progress: 0.0,
                        });
                        self.visual_x = cur_x;
                        self.visual_y = cur_y;
                    } else if anim.is_finished() {
                        self.visual_x = anim.target_x;
                        self.visual_y = anim.target_y;
                        self.animation = None;
                    } else {
                        let (cur_x, cur_y) = anim.current_position();
                        self.visual_x = cur_x;
                        self.visual_y = cur_y;
                    }
                } else {
                    let prev_vx = self.visual_x;
                    let prev_vy = self.visual_y;
                    if (prev_vx - target_x).abs() > 0.01 || (prev_vy - target_y).abs() > 0.01 {
                        self.animation = Some(CursorAnimationState {
                            start_x: if (start_x - prev_vx).abs() < 0.01 && (start_y - prev_vy).abs() < 0.01 {
                                prev_vx
                            } else {
                                start_x
                            },
                            start_y: if (start_x - prev_vx).abs() < 0.01 && (start_y - prev_vy).abs() < 0.01 {
                                prev_vy
                            } else {
                                start_y
                            },
                            target_x,
                            target_y,
                            progress: 0.0,
                        });
                    } else {
                        self.visual_x = target_x;
                        self.visual_y = target_y;
                    }
                }
            }
        }

        self.force_snap_next = false;

        let pos_changed = (self.visual_x - old_x).abs() > 0.01
            || (self.visual_y - old_y).abs() > 0.01
            || !old_visible;
        if pos_changed {
            self.dirty = true;
            self.blink_visible = true;
            self.blink_last_toggle = Instant::now();
        }

        let blink_changed = old_blink_visible != self.blink_visible;

        let position_changed =
            (old_x - plan.cursor_x).abs() > 0.01 || (old_y - plan.cursor_y).abs() > 0.01;

        CursorUpdateResult {
            ime_needs_update: position_changed,
            needs_repaint: pos_changed || self.animation.is_some(),
            visibility_changed,
            blink_changed,
            visual_position_changed: pos_changed,
        }
    }

    pub fn update_animation_progress(&mut self, progress: f64) -> bool {
        if let Some(ref mut anim) = self.animation {
            anim.progress = progress.clamp(0.0, 1.0);
            if anim.is_finished() {
                self.visual_x = anim.target_x;
                self.visual_y = anim.target_y;
                self.animation = None;
                self.dirty = false;
                false
            } else {
                let (cx, cy) = anim.current_position();
                self.visual_x = cx;
                self.visual_y = cy;
                true
            }
        } else {
            self.dirty = false;
            false
        }
    }

    pub fn tick_animation(&mut self) -> bool {
        if let Some(ref anim) = self.animation {
            if anim.is_finished() {
                self.visual_x = anim.target_x;
                self.visual_y = anim.target_y;
                self.animation = None;
                self.dirty = false;
                false
            } else {
                let (cx, cy) = anim.current_position();
                self.visual_x = cx;
                self.visual_y = cy;
                true
            }
        } else {
            self.dirty = false;
            false
        }
    }

    pub fn tick_blink(&mut self, blink_mode: CursorBlinkMode) -> bool {
        if !self.visible {
            if !self.blink_visible {
                return false;
            }
            self.blink_visible = false;
            return true;
        }

        if blink_mode == CursorBlinkMode::Suppressed {
            if !self.blink_visible {
                self.blink_visible = true;
                return true;
            }
            return false;
        }

        if self.blink_reset_requested {
            self.blink_visible = true;
            self.blink_last_toggle = Instant::now();
            self.blink_reset_requested = false;
            return true;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.blink_last_toggle);
        if elapsed >= Duration::from_millis(BLINK_INTERVAL_MS) {
            self.blink_visible = !self.blink_visible;
            self.blink_last_toggle = now;
            return true;
        }
        false
    }

    #[allow(dead_code)]
    pub fn reset_blink(&mut self) {
        self.blink_visible = true;
        self.blink_last_toggle = Instant::now();
    }
}

pub struct CursorUpdateResult {
    pub ime_needs_update: bool,
    pub needs_repaint: bool,
    pub visibility_changed: bool,
    pub blink_changed: bool,
    pub visual_position_changed: bool,
}
