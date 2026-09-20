use crate::editor::layout::CaretAffinity;
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::QQuickItem;
use std::time::Instant;

use super::cursor_controller::CursorUpdateResult;
use super::SujianEditorItem;

/// 光标动画状态 — 使用事务 Timeline 的 progress 而非独立时间源。
///
/// Issue #516: 光标不再维护独立 Choreographer/start_time，
/// 而是消费与文字动画相同的 Timeline progress。
/// Issue #702 评论 5707449688 问题 2: 纯光标移动彻底和文字事务 key 解耦，
/// `CursorAnimationState` 不再保存 `driver_key`。`started_at`/`duration_ms`
/// 让 CursorAnimationState 拥有自己的 timeline，用 Scene Graph 当前帧的
/// `frame_now` 推进 from→to 动画。
#[derive(Clone, Debug)]
pub struct CursorAnimationState {
    pub start_x: f64,
    pub start_y: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub progress: f64,
    /// Issue #702: 纯光标移动自己的 timeline 起始时间。
    /// `None` 表示尚未启动（第一帧），由 `sample_cursor_only_position`
    /// 在首次采样时用 `frame_now` 初始化。
    pub started_at: Option<Instant>,
    /// Issue #702: 纯光标移动自己的 timeline 时长（毫秒）。
    /// 输入/删除存在正文视觉事务时，光标继续消费同一帧进度，
    /// 此字段仅用于纯方向键/Home/End 等没有正文事务的 from→to 动画。
    pub duration_ms: u64,
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

    /// Issue #702: 用 Scene Graph 当前帧的 `frame_now` 推进纯光标 from→to 动画。
    /// 首次调用（`started_at` 为 `None`）时返回 0.0 并通过返回值的第二分量
    /// `true` 表示尚未启动，调用方负责用 `frame_now` 初始化 `started_at`。
    pub fn sample_progress(&self, frame_now: Instant) -> (f64, bool) {
        if self.duration_ms == 0 {
            return (1.0, false);
        }
        let start = match self.started_at {
            Some(s) => s,
            None => return (0.0, true),
        };
        let elapsed_ms = frame_now.duration_since(start).as_millis() as f64;
        let p = (elapsed_ms / self.duration_ms as f64).clamp(0.0, 1.0);
        (p, false)
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
    ///
    /// Issue #709 评论 issue-body-709: 两条时间线互斥：
    /// - 有正文事务时（found_tx = Some）：光标由正文协同接管，不创建独立 CursorOnly 动画。
    ///   Insert/Delete 的光标位置由正文事务的同一条 Timeline 决定，
    ///   build_cursor_plan 收到 (old_cursor_rect, new_cursor_rect) 后走 Coordinated 路径。
    /// - 无正文事务时（found_tx = None，纯鼠标点击/方向键/Home/End）：走纯光标 Tween，
    ///   CursorAnimationState 拥有自己的 timeline（started_at + duration_ms），
    ///   不依赖任何正文事务。
    /// 两条时间线互斥，不为了修可见性再引入第二套光标动画。
    pub(crate) fn update_cursor_visual_position(&mut self) -> CursorUpdateResult {
        // Issue #724 评论 5751268664 缺口2: 自动跟随滚动期间使用屏幕锚点替代
        // current_scroll_y 算 caret viewport 坐标。QML 侧 begin_auto_follow_scroll()
        // 调 set_auto_follow_anchor(y, h) 把滚动前上一帧实际画出的 caret viewport y/h 传进来，
        // contentY 变化时 caret 仍画在锚点位置，不被滚动拖走。end_auto_follow_scroll()
        // 调 clear_auto_follow_anchor() 释放锚点，恢复使用 current_scroll_y。
        let scroll_y = match self.current_auto_follow_anchor {
            Some((_anchor_y, _anchor_h)) => {
                // 锚点存在时，editor_layout_cursor_rect 仍用真实 scroll_y 算文档坐标，
                // 但 build_cursor_plan 收到的 cursor_y 改为锚点 y，让 caret 画在锚点位置。
                // 这里通过把 scroll_y 替换为"使 caret viewport y 等于 anchor_y"的等效值实现：
                // caret_doc_y - effective_scroll_y = anchor_y → effective_scroll_y = caret_doc_y - anchor_y
                // 但 caret_doc_y 依赖 scroll_y，这里先用真实 scroll_y 算一次 caret_doc_y，
                // 再用 anchor_y 反推 effective_scroll_y。
                // 简化实现：直接用真实 scroll_y 算文档坐标，build_cursor_plan 收到 anchor_y 作为 cursor_y。
                f64::from(self.current_scroll_y)
            }
            None => f64::from(self.current_scroll_y),
        };
        let layout_res =
            self.editor_layout_cursor_rect(self.buffer.cursor, self.cursor_ctrl.affinity, scroll_y);

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        // Issue #724 评论 5751268664 缺口2: 自动跟随滚动期间用锚点 y/h 替代
        // 当前 scroll_y 算出的 viewport y/h，让 caret 画在锚点位置。
        let (cursor_y, cursor_h) = match self.current_auto_follow_anchor {
            Some((anchor_y, anchor_h)) => (anchor_y, anchor_h.max(cursor_h)),
            None => (cursor_y, cursor_h),
        };

        let vp_h = f64::from(self.current_viewport_height.max(1.0));
        let is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let is_preediting = !self.pipeline.composition().preedit_text.is_empty();

        // Issue #679 评论 5657313927 (步骤 2): 根据当前 target 查 coordinator 里
        // 是否已经有对应的正文/预输入视觉事务。
        // Issue #705 评论 5717380886: 传入当前 cursor_owner_epoch。
        // epoch 不一致时 find_cursor_transaction_for_target 不返回该事务，
        // 文字事务继续播自己的 glyph/reflow，但不再驱动 caret。
        // Issue #709 评论 issue-body-709: found_tx 决定走哪条时间线：
        // - Some：正文协同光标接管（Insert/Delete），不创建独立 CursorOnly 动画
        // - None：纯光标 Tween（鼠标点击/方向键/Home/End），CursorAnimationState 自己的 timeline
        let found_tx = self
            .pipeline
            .animation_coordinator()
            .find_cursor_transaction_for_target(
                cursor_x,
                cursor_y,
                cursor_h,
                self.cursor_ctrl.cursor_owner_epoch,
            );

        // Issue #686 评论 5664857575 领域2：存在活动正文事务时，光标位置由最新正文事务
        // 的同一条 Timeline 决定。纯光标移动（方向键、Home/End、鼠标点击后的平滑移动）
        // 不再创建 CursorOnly 文字事务，由 CursorAnimationState 自己的 timeline 推进。

        let (old_cursor_rect, new_cursor_rect) = match found_tx {
            Some((_key, old_r, new_r)) => (old_r, new_r),
            None => (None, None),
        };

        // Issue #679 评论 5657313927 (步骤 4): 调唯一的 build_cursor_plan。
        // Issue #702 评论 5707449688 问题 2: 不再传 driver_key，纯光标 Tween 由
        // CursorAnimationState 自己的 timeline 推进。
        // Issue #705 评论 5717380886: 传入当前 cursor_owner_epoch。
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
            self.cursor_ctrl.visible,
            self.cursor_ctrl.blink_visible,
            self.cursor_ctrl.visual_x,
            self.cursor_ctrl.visual_y,
            self.cursor_ctrl.force_snap_next,
            self.cursor_ctrl.animation.as_ref(),
            self.cursor_ctrl.cursor_owner_epoch,
            self.cursor_ctrl.last_move_source,
            layout_res.baseline_y,
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
