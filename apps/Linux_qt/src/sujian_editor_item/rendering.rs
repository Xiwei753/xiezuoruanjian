use crate::editor::layout::CaretAffinity;
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::QQuickItem;

use super::animation_coordinator::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::cursor_controller::CursorUpdateResult;
use super::SujianEditorItem;

/// 光标动画状态 — 使用事务 Timeline 的 progress 而非独立时间源。
///
/// Issue #516: 光标不再维护独立 Choreographer/start_time，
/// 而是消费与文字动画相同的 Timeline progress。
#[derive(Clone, Debug)]
pub struct CursorAnimationState {
    pub start_x: f64,
    pub start_y: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub progress: f64,
}

impl CursorAnimationState {
    pub fn current_position(&self) -> (f64, f64) {
        let t = self.progress.clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3i32);
        let x = self.start_x + (self.target_x - self.start_x) * eased;
        let y = self.start_y + (self.target_y - self.start_y) * eased;
        (x, y)
    }

    pub fn is_finished(&self) -> bool {
        self.progress >= 1.0
    }
}

impl SujianEditorItem {
    // has_active_animation() removed: animation display lifecycle is now managed
    // by ActiveVisualTransactionQueue in Scene Graph (child[1]).

    // cleanup_finished_animations() removed: transaction completion is handled
    // atomically via transactionId + generation in updatePaintNode.

    // Issue #658: render_to_image() / paint_onto() / ScrollBuffer deleted.
    // 静态正文不再栅格化为整块 QImage，改由 QSGTextNode（Qt 6.7+ 公开 API）渲染。
    // 保留的 QImage/QPainter 只服务于动画快照（line_snapshot_builder 生成的行纹理切片）。

    /// Update cursor visual position from layout-computed position.
    ///
    /// **IMPORTANT**: This method MUST only be called from the GUI thread.
    /// It directly emits signals and calls inputMethod()->update().
    pub(crate) fn update_cursor_visual_position(&mut self) -> CursorUpdateResult {
        let scroll_y = f64::from(self.current_scroll_y);
        let layout_res =
            self.editor_layout_cursor_rect(self.buffer.cursor, self.cursor_ctrl.affinity, scroll_y);

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        let vp_h = f64::from(self.current_viewport_height.max(1.0));
        let _is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let _is_preediting = !self.pipeline.composition().preedit_text.is_empty();

        let cursor_plan = CursorAnimationPlan {
            should_be_visible: self.current_editor_enabled
                && !self.buffer.has_selection()
                && cursor_y + cursor_h > 0.0
                && cursor_y < vp_h
                && !self.current_is_scrolling,
            blink_mode: if self.current_coordinated_text_cursor_animation_enabled
                && self
                    .pipeline
                    .animation_coordinator_mut()
                    .has_active_insert()
            {
                CursorBlinkMode::Suppressed
            } else {
                CursorBlinkMode::Normal
            },
            transition: if self.current_smooth_cursor_enabled {
                CursorTransition::Tween {
                    old_rect: writer_core::editor::CursorRect {
                        x: self.cursor_ctrl.visual_x,
                        top: self.cursor_ctrl.visual_y,
                        bottom: self.cursor_ctrl.visual_y + cursor_h,
                        baseline_y: self.cursor_ctrl.visual_y + cursor_h * 0.8,
                    },
                    new_rect: writer_core::editor::CursorRect {
                        x: cursor_x,
                        top: cursor_y,
                        bottom: cursor_y + cursor_h,
                        baseline_y: cursor_y + cursor_h * 0.8,
                    },
                }
            } else {
                CursorTransition::Snap
            },
            cursor_x,
            cursor_y,
            cursor_h,
        };

        let result = self.cursor_ctrl.apply_plan(&cursor_plan);

        if result.needs_repaint {
            let item = self as &dyn QQuickItem;
            item.update();
        }

        if result.ime_needs_update
            || result.visibility_changed
            || result.blink_changed
            || result.visual_position_changed
        {
            self.cursor_rect_changed();
        }

        if result.ime_needs_update {
            let obj_ptr = self.get_cpp_object();
            if !obj_ptr.is_null() {
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                    QGuiApplication::inputMethod()->update(Qt::ImQueryInput);
                });
            }
        }

        {
            let mut line_info = String::new();
            if let Some(snapshot) = self.editor_layout.cache() {
                if let Some(line) = snapshot.lines.iter().find(|l| l.id == visual_line_id) {
                    let font_size = f64::from(snapshot.font_size);
                    let font_family = &snapshot.font_family;
                    let ascent = if line.qt_ascent > 0.0 {
                        line.qt_ascent
                    } else {
                        crate::editor::layout::get_font_ascent(font_family, snapshot.font_size)
                    };
                    let descent = if line.qt_descent > 0.0 {
                        line.qt_descent
                    } else {
                        crate::editor::layout::get_font_descent(font_family, snapshot.font_size)
                    };
                    let baseline =
                        crate::editor::layout::text_baseline_y(line, font_size, font_family);
                    let cursor_top_doc = cursor_y + scroll_y;
                    let cursor_top_to_baseline = baseline - cursor_top_doc;
                    let cursor_bottom_to_baseline = cursor_top_doc + cursor_h - baseline;
                    line_info = format!(
                        ", line.y={:.1}, line.height={:.1}, visual_line_id={}, font_ascent={:.1}, font_descent={:.1}, text_baseline_y={:.1}, cursor_top_to_baseline={:.1}, cursor_bottom_to_baseline={:.1}, cursor_h={:.1}, qt_ascent={:.1}, qt_descent={:.1}",
                        line.y, line.height, line.id, ascent, descent, baseline,
                        cursor_top_to_baseline, cursor_bottom_to_baseline, cursor_h,
                        line.qt_ascent, line.qt_descent
                    );
                }
            }
            crate::sujian_editor_item::editor_debug_log(&format!(
                "update_cursor_visual_position: cursor={}, target_x={:.1}, target_y={:.1}, visual_x={:.1}, visual_y={:.1}, is_animating={}, scroll_y={:.1}{}",
                self.buffer.cursor, self.cursor_ctrl.target_x, self.cursor_ctrl.target_y, self.cursor_ctrl.visual_x, self.cursor_ctrl.visual_y, self.cursor_ctrl.animation.is_some(), scroll_y, line_info
            ));
        }

        if self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.dirty = true;
            self.request_frame_update();
        }

        if self.buffer.has_selection() {
            let anchor_layout = self.editor_layout_cursor_rect(
                self.buffer.selection_anchor,
                CaretAffinity::Downstream,
                scroll_y,
            );
            self.cursor_ctrl.anchor_visual_x = Some(anchor_layout.x);
            self.cursor_ctrl.anchor_visual_y = Some(anchor_layout.y);
        } else {
            self.cursor_ctrl.anchor_visual_x = None;
            self.cursor_ctrl.anchor_visual_y = None;
        }

        result
    }
}

// Static text always renders full content. Animation must not affect text correctness.
