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
    /// `None` 表示尚未启动（第一帧），由 `CursorController::tick_animation`
    /// 在首次采样时用 `frame_now` 初始化（Issue #826 评论 36 的唯一推进入口）。
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
/// 供 `CursorController::tick_animation` 推进光标 Tween 使用
///（Issue #826 评论 36：生产只留这一个推进入口），
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
    /// Issue #709 评论 issue-body-709: 两条时间线互斥。
    /// - 协同模式的正文事务（`coordinated_animation_enabled` + 本次是
    ///   TextTransaction + 光标可见 + 无强制 Snap）：不建独立 CursorOnly 动画，
    ///   由 coordinator 的单条 `CoordinatedCaretMotion` 接管（Issue #826 评论 38：
    ///   start = 当前 visual，target = 最新 canonical caret，时钟与前沿统一，
    ///   光标画的位置就是前沿吞吐边界的位置）。
    /// - 其余（协同关 / 纯鼠标点击/方向键/Home/End / Snap）：走纯光标 Tween，
    ///   `CursorAnimationState` 拥有自己的 timeline（started_at + duration_ms），
    ///   不依赖任何正文事务。
    ///
    /// 两种跟随态互斥，不为了修可见性再引入第二套光标动画。
    pub(crate) fn update_cursor_visual_position(&mut self) -> CursorUpdateResult {
        // Issue #724 评论 5751268664 缺口2: 自动跟随滚动期间使用屏幕锚点替代
        // current_scroll_y 算 caret viewport 坐标。QML 侧 begin_auto_follow_scroll()
        // 调 set_auto_follow_anchor_with_target(y, h, target_y) 把滚动前上一帧实际画出的
        // caret viewport y/h 和滚动目标值传进来。contentY 变化时 caret 仍画在锚点位置，
        // 不被滚动拖走。当 current_scroll_y 到达 target_scroll_y 时清除锚点，
        // 不再由 80ms Timer 决定生命周期。end_auto_follow_scroll() 调
        // clear_auto_follow_anchor() 也可释放锚点。
        // Issue #724 评论 5752398265: anchor 只在最终绘制时（build_render_plan_full）
        // 覆盖屏幕 y/h，不污染 find_cursor_transaction_for_target / build_cursor_plan
        // 的逻辑 cursor_y。scroll_y 始终用真实 current_scroll_y。
        // Issue #727 评论 5757225958 问题1: cursor_ctrl.target_y/visual_y 统一保存
        // 文档坐标（不减 scroll_y），和 scene graph cursor layer（QSGTransformNode 做
        // translate(0, -scroll_y)）的假设一致。改用 editor_layout_cursor_rect_doc()
        // 获取文档坐标 caret。QML/IME 边界方法在返回前减 current_scroll_y 转视口坐标。
        let scroll_y = f64::from(self.current_scroll_y);
        let layout_res =
            self.editor_layout_cursor_rect_doc(self.pipeline.cursor(), self.cursor_ctrl.affinity);

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        // Issue #810 评论 问题2: 分离 Core selection 状态与平台 selection gesture 状态。
        //
        // 旧逻辑：`let is_selecting = self.pipeline.selection_anchor() != self.pipeline.cursor();`
        // 把"选区存在"（Core 业务真相）直接当成"用户正在拖选"（平台手势状态），
        // 传给 build_cursor_plan 的 is_selecting 参数驱动 hard_snap。
        // 问题：长按/拖选 release 后选区仍存在，is_selecting 仍为 true，普通光标
        // 移动被强制 Snap，无法 Tween。
        //
        // 新逻辑：
        // - has_selection（Core 真相）：只决定普通 caret 当前是否显示（should_be_visible）。
        // - selection_gesture_active（平台手势）：只决定拖选期间是否强制 Snap。
        // 两者分离：选区存在但手势已结束 → caret 隐藏但不强制 Snap，恢复时 Tween。
        let has_selection = self.pipeline.has_selection();
        let selection_gesture_active = self.selection_gesture_active;
        let is_preediting = !self.pipeline.composition().preedit_text.is_empty();

        // Issue #826 评论 38：协同模式下正文编辑的光标不再建独立 Tween。
        //
        // coordinated ON + 本次是正文事务 + 光标可见 + 无强制 Snap 时，由
        // coordinator 的单条 CoordinatedCaretMotion 接管（start = 当前 visual，
        // target = 最新 canonical caret，时钟与前沿统一）；接管成功则直接落
        // 协同跟随态，`cursor_ctrl.animation` 保持 None。
        //
        // 其余情况（协同关 / 纯光标移动 / Snap / 选区隐藏 / 前沿不存在…）走
        // 原来的独立 Tween，并清掉可能残留的协同 motion——任何独立的光标决策
        // 都意味着用户亲自接管了光标（见 `clear_coordinated_caret`）。
        let should_be_visible =
            self.current_editor_enabled && !has_selection && !is_preediting;
        let hard_snap_requested = self.cursor_ctrl.force_snap_next
            || selection_gesture_active
            || self.current_is_scrolling;
        let coordinated_follow = self.current_coordinated_animation_enabled
            && self.cursor_ctrl.last_move_source
                == super::cursor_controller::CursorMoveSource::TextTransaction
            && should_be_visible
            && !hard_snap_requested;
        let result = if coordinated_follow {
            let taken = self.pipeline.animation_coordinator_mut()
                .begin_or_retarget_coordinated_caret(
                    self.cursor_ctrl.visual_x,
                    self.cursor_ctrl.visual_y,
                    cursor_x,
                    cursor_y,
                    Instant::now(),
                );
            if taken {
                self.apply_coordinated_cursor_follow(
                    cursor_x,
                    cursor_y,
                    cursor_h,
                    layout_res.baseline_y,
                )
            } else {
                // 前沿不存在（CursorOnly 等）：协同接不上，回退独立 Tween。
                self.pipeline
                    .animation_coordinator_mut()
                    .clear_coordinated_caret();
                let cursor_plan = self.pipeline.animation_coordinator().build_cursor_plan(
                    &super::animation::CursorMoveInputs {
                        cursor_x,
                        cursor_y,
                        cursor_h,
                        editor_enabled: self.current_editor_enabled,
                        has_selection,
                        is_scrolling: self.current_is_scrolling,
                        selection_gesture_active,
                        is_preediting,
                        smooth_cursor_enabled: self.current_smooth_cursor_enabled,
                        duration_ms: u64::from(self.current_cursor_animation_duration_ms),
                        visual_x: self.cursor_ctrl.visual_x,
                        visual_y: self.cursor_ctrl.visual_y,
                        force_snap_next: self.cursor_ctrl.force_snap_next,
                        baseline_y: layout_res.baseline_y,
                    },
                );
                self.cursor_ctrl.apply_plan(&cursor_plan)
            }
        } else {
            self.pipeline
                .animation_coordinator_mut()
                .clear_coordinated_caret();
            // Issue #826: 光标动画与正文动画完全解耦。
            //
            // 逻辑 caret 已经在 Core 里立即变成当前 selection；这里只把当前视觉位置
            // 与目标 caret rect 交给 `build_cursor_plan`，让它决定 Snap 还是 Tween。
            // 不再查"哪笔正文事务拥有这条 caret"，不再传 cursor_owner_epoch。
            let cursor_plan = self.pipeline.animation_coordinator().build_cursor_plan(
                &super::animation::CursorMoveInputs {
                    cursor_x,
                    cursor_y,
                    cursor_h,
                    editor_enabled: self.current_editor_enabled,
                    has_selection,
                    is_scrolling: self.current_is_scrolling,
                    selection_gesture_active,
                    is_preediting,
                    smooth_cursor_enabled: self.current_smooth_cursor_enabled,
                    duration_ms: u64::from(self.current_cursor_animation_duration_ms),
                    visual_x: self.cursor_ctrl.visual_x,
                    visual_y: self.cursor_ctrl.visual_y,
                    force_snap_next: self.cursor_ctrl.force_snap_next,
                    baseline_y: layout_res.baseline_y,
                },
            );

            // Issue #679 评论 5657313927 (步骤 5): apply_plan。
            self.cursor_ctrl.apply_plan(&cursor_plan)
        };

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
                    // Issue #727 评论 5757225958 问题1: cursor_y 已是文档坐标，
                    // 不需要再加 scroll_y 还原。
                    let cursor_top_doc = cursor_y;
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
                self.pipeline.cursor(), self.cursor_ctrl.target_x, self.cursor_ctrl.target_y, self.cursor_ctrl.visual_x, self.cursor_ctrl.visual_y, self.cursor_ctrl.animation.is_some(), scroll_y, line_info
            ));
        }

        if self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.dirty = true;
            self.request_frame_update();
        }

        if self.pipeline.has_selection() {
            // Issue #727 评论 5757225958 问题1: anchor_visual_y 统一保存文档坐标，
            // properties.rs::anchor_rect_y 在边界返回 doc_y - current_scroll_y。
            let anchor_layout = self.editor_layout_cursor_rect_doc(
                self.pipeline.selection_anchor(),
                CaretAffinity::Downstream,
            );
            self.cursor_ctrl.anchor_visual_x = Some(anchor_layout.x);
            self.cursor_ctrl.anchor_visual_y = Some(anchor_layout.y);
        } else {
            self.cursor_ctrl.anchor_visual_x = None;
            self.cursor_ctrl.anchor_visual_y = None;
        }

        result
    }

    /// Issue #826 评论 38：协同跟随态的光标落点（不建独立 Tween）。
    ///
    /// 调用前 coordinator 已建好/retarget 好本轮的 `CoordinatedCaretMotion`
    /// （时钟与前沿统一）。这里只把逻辑目标记到 `cursor_ctrl`（target/visual_h/
    /// baseline/可见性），`animation` 保持 `None` —— 真正的逐帧推进由 Scene Graph
    /// 帧调 [`Self::tick_coordinated_caret_with_time`] 从 motion 采样后写入
    /// `visual_x/visual_y`，与前沿吞吐边界同 progress。
    pub(crate) fn apply_coordinated_cursor_follow(
        &mut self,
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        baseline_y: f64,
    ) -> CursorUpdateResult {
        // 防御性消费：协同分支要求无强制 Snap，这里应为 false；即使将来调用方
        // 变了，也不让一枚陈旧的 force-snap 漏到下一次光标移动。
        let _ = self.cursor_ctrl.take_force_snap_next();

        let old_x = self.cursor_ctrl.target_x;
        let old_y = self.cursor_ctrl.target_y;
        let old_visible = self.cursor_ctrl.visible;
        let old_blink_visible = self.cursor_ctrl.blink_visible;

        self.cursor_ctrl.target_x = cursor_x;
        self.cursor_ctrl.target_y = cursor_y;
        self.cursor_ctrl.visual_h = cursor_h;
        self.cursor_ctrl.ime_cursor_rect_h = cursor_h;
        self.cursor_ctrl.visible = true;
        self.cursor_ctrl.visibility_state =
            super::cursor_controller::CursorVisibilityState::Visible;
        self.cursor_ctrl.selection_head_rect = None;
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.visual_baseline_y = baseline_y;
        // 新建/retarget motion 就是明确的状态变化信号（对标 apply_plan 的
        // started_or_rebased_tween）：blink 从可见态重新开始。
        self.cursor_ctrl.blink_visible = true;
        self.cursor_ctrl.blink_last_toggle = Instant::now();
        self.cursor_ctrl.dirty = true;

        let item = self as &dyn QQuickItem;
        item.update();

        let visibility_changed = !old_visible;
        let blink_changed = old_blink_visible != self.cursor_ctrl.blink_visible;
        let position_changed =
            (old_x - cursor_x).abs() > 0.01 || (old_y - cursor_y).abs() > 0.01;
        CursorUpdateResult {
            ime_needs_update: position_changed,
            needs_repaint: true,
            visibility_changed,
            blink_changed,
            visual_position_changed: position_changed,
        }
    }

    /// Issue #826 评论 38：协同 caret 的**每帧唯一采样点**（对标独立 Tween 的
    /// `CursorController::tick_animation`）。
    ///
    /// 有协同 motion 时从它采样并写入 `visual_x/visual_y`，返回 true（调用方
    /// 不得再调独立 `tick_animation`，否则同一帧 visual 被写两次）。
    /// 无 motion 时返回 false，调用方走独立光标 timeline。
    ///
    /// 必须在 `tick_text_animations_with_time` **之前**调：motion 到终点时这里
    /// 把 visual 精确落到 target 并清掉 motion，随后的 `tick()` 才收前沿；
    /// 反过来会留下一帧 0.99 的亚像素残留。
    pub(crate) fn tick_coordinated_caret_with_time(&mut self, frame_now: Instant) -> bool {
        let sample = self
            .pipeline
            .animation_coordinator_mut()
            .sample_coordinated_caret(frame_now);
        let Some(sample) = sample else {
            return false;
        };
        self.cursor_ctrl.visual_x = sample.x;
        self.cursor_ctrl.visual_y = sample.y;
        if sample.finished {
            self.cursor_ctrl.dirty = false;
        }
        true
    }
}

// Static text always renders full content. Animation must not affect text correctness.
