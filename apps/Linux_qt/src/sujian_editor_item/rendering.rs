use crate::editor::layout::CaretAffinity;
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::QQuickItem;

use super::cursor_controller::CursorUpdateResult;
use super::transaction_key::VisualTransactionKey;
use super::SujianEditorItem;

/// 光标动画状态 — 使用事务 Timeline 的 progress 而非独立时间源。
///
/// Issue #516: 光标不再维护独立 Choreographer/start_time，
/// 而是消费与文字动画相同的 Timeline progress。
/// Issue #679 评论 5657313927: 保存 `driver_key` 指向驱动本段光标动画的视觉事务，
/// `tick_cursor_animation` 按 key 取样 Timeline progress。
#[derive(Clone, Debug)]
pub struct CursorAnimationState {
    pub driver_key: VisualTransactionKey,
    pub start_x: f64,
    pub start_y: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub progress: f64,
}

impl CursorAnimationState {
    pub fn current_position(&self) -> (f64, f64) {
        let t = self.progress.clamp(0.0, 1.0);
        let eased = ease_out_cubic(t);
        let x = self.start_x + (self.target_x - self.start_x) * eased;
        let y = self.start_y + (self.target_y - self.start_y) * eased;
        (x, y)
    }

    pub fn is_finished(&self) -> bool {
        self.progress >= 1.0
    }
}

/// Issue #701 评论 5699573227 第三阶段 (F5): ease-out-cubic 缓动函数。
///
/// 供 `build_render_plan_full` 内部 `sample_cursor_only_position` 使用，
/// 避免在 `animation_coordinator.rs` 内联 easing 公式（Issue #690 步骤 2 要求
/// 协调器内不得内联各自的 easing 公式）。CursorOnly 的三次曲线保持独立，
/// 不并入协同曲线（`AnimatedSlice::ease_out_quad`）。
pub fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3i32)
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
    ///
    /// Issue #679 评论 5657313927: 不再手写第二套 CursorAnimationPlan，
    /// 统一调 `AnimationCoordinator::build_cursor_plan()`。
    /// CursorOnly 的创建也统一放到这里：方向键、Home/End、程序化移动全走同一入口。
    pub(crate) fn update_cursor_visual_position(&mut self) -> CursorUpdateResult {
        let scroll_y = f64::from(self.current_scroll_y);
        let layout_res =
            self.editor_layout_cursor_rect(self.buffer.cursor, self.cursor_ctrl.affinity, scroll_y);

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        let vp_h = f64::from(self.current_viewport_height.max(1.0));
        let is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let is_preediting = !self.pipeline.composition().preedit_text.is_empty();

        // Issue #679 评论 5657313927 (步骤 2): 根据当前 target 查 coordinator 里
        // 是否已经有对应的正文/预输入视觉事务。
        let mut found_tx = self
            .pipeline
            .animation_coordinator()
            .find_cursor_transaction_for_target(cursor_x, cursor_y, cursor_h);
        let mut created_cursor_only_key: Option<VisualTransactionKey> = None;

        // Issue #686 评论 5664857575 领域2：存在活动正文事务时，光标位置由最新正文事务
        // 的同一条 Timeline 决定，不再额外创建 CursorOnly。CursorOnly 只用于没有正文事务
        // 的纯光标移动（方向键、Home/End、鼠标点击后的平滑移动）。
        let has_active_text_tx = self
            .pipeline
            .animation_coordinator()
            .active_text_transaction_key()
            .is_some();

        // Issue #679 评论 5657313927 (步骤 3): 如果没有事务、当前又确实应该平滑移动
        // （不是点击强制 snap、不是滚动、不是选择），且 smooth cursor 开启，
        // 就创建一个 CursorOnly，拿到它的 key。
        // 注意：handle_cursor_only 是 &mut self，需要先做可变操作。
        // Issue #679 评论 5658087764 (3): 创建 CursorOnly 前先判断光标是否真的移动了，
        // 避免创建没有实际位移的事务白白压住 blink/请求动画帧。
        let needs_cursor_motion = (self.cursor_ctrl.visual_x - cursor_x).abs() > 0.01
            || (self.cursor_ctrl.visual_y - cursor_y).abs() > 0.01;

        if found_tx.is_none()
            && !has_active_text_tx
            && needs_cursor_motion
            && self.current_smooth_cursor_enabled
            && !self.cursor_ctrl.force_snap_next
            && !self.current_is_scrolling
            && !is_selecting
            && !is_preediting
            && self.current_editor_enabled
        {
            let old_cursor_rect = self.current_cursor_rect_for_transaction();
            let new_cursor_rect = Some(writer_core::editor::CursorRect {
                x: cursor_x,
                top: cursor_y,
                bottom: cursor_y + cursor_h,
                baseline_y: cursor_y + cursor_h * 0.8,
            });
            if let Some(key) = self
                .pipeline
                .animation_coordinator_mut()
                .handle_cursor_only(old_cursor_rect, new_cursor_rect)
            {
                created_cursor_only_key = Some(key);
                found_tx = self
                    .pipeline
                    .animation_coordinator()
                    .find_cursor_transaction_for_target(cursor_x, cursor_y, cursor_h);
            }
        }

        let (old_cursor_rect, new_cursor_rect, driver_key) = match found_tx {
            Some((key, old_r, new_r)) => (old_r, new_r, Some(key)),
            None => (None, None, created_cursor_only_key),
        };

        // Issue #679 评论 5657313927 (步骤 4): 调唯一的 build_cursor_plan。
        // Tween 必须带 driver key（从找到的或刚创建的事务获取）。
        let cursor_plan = self.pipeline.animation_coordinator().build_cursor_plan(
            old_cursor_rect,
            new_cursor_rect,
            cursor_x,
            cursor_y,
            cursor_h,
            self.current_editor_enabled,
            self.buffer.has_selection(),
            vp_h,
            self.current_is_scrolling,
            is_selecting,
            is_preediting,
            self.current_smooth_cursor_enabled,
            self.current_cursor_animation_duration_ms,
            self.current_coordinated_text_cursor_animation_enabled,
            f64::from(self.current_scroll_y),
            self.cursor_ctrl.last_scroll_y,
            self.cursor_ctrl.visible,
            self.cursor_ctrl.blink_visible,
            self.cursor_ctrl.visual_x,
            self.cursor_ctrl.visual_y,
            self.cursor_ctrl.force_snap_next,
            self.cursor_ctrl.animation.as_ref(),
            driver_key,
        );

        // Issue #679 评论 5657313927 (步骤 5): apply_plan。
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
