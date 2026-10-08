//! sujian_editor_item 内部状态测试。
//! 正文动画旧状态机测试已随 #853 单状态过渡删除；独立光标时间轴测试保留。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::render_plan::{
    CursorRenderState, CursorStyle, PreparedEditorFrame, SelectionPreeditPlan,
    SelectionPreeditStyle,
};
use super::*;
use crate::editor::layout::{run_on_qt_thread, LayoutParams};
use qmetaobject::QString;
use std::time::{Duration, Instant};

fn default_layout_params() -> LayoutParams {
    LayoutParams {
        width: 400.0,
        font_size: 22.0,
        font_family: "Noto Sans CJK SC".to_string(),
        line_spacing: 1.5,
        text_indent: 0.0,
        padding: 16.0,
    }
}

// =========================================================================
// 测试 1: emit_content_changed 后旧 prepared_frame 失效
// =========================================================================

/// `emit_content_changed()` 必须使旧 `prepared_frame` 失效：
/// 内部先 `prepared_frame = None`，再 `request_static_repaint` →
/// `prepare_editor_frame` 用新 text_revision 重新准备 frame。
/// 验证旧 frame 的 revision 与新 frame 不同（旧 frame 已被替换）。
#[test]
fn emit_content_changed_invalidates_old_prepared_frame() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello世界"));

        // 构造一个 PreparedEditorFrame 并设置，模拟 GUI 线程已 prepare 好一帧。
        let text = item.pipeline.committed_text().to_string();
        let revision = item.pipeline.text_revision();
        let params = default_layout_params();
        let snapshot = item.editor_layout.snapshot(&text, params, revision).clone();
        let old_generation = snapshot.layout_generation;
        item.prepared_frame = Some(PreparedEditorFrame {
            layout_snapshot: snapshot,
            selection_preedit: SelectionPreeditPlan::default(),
        });
        assert!(
            item.prepared_frame.is_some(),
            "设置 prepared_frame 后应为 Some"
        );

        // 调用 emit_content_changed — 旧 frame 必须失效，新 frame 被准备
        item.emit_content_changed();
        assert!(
            item.prepared_frame.is_some(),
            "emit_content_changed 后 prepared_frame 应为 Some（新 frame 已准备）"
        );
        let new_frame = item.prepared_frame.as_ref().expect("new frame");
        // text_revision 必须 bump（旧 frame 失效）
        assert_ne!(
            new_frame.layout_snapshot.text_revision, revision,
            "emit_content_changed 后 text_revision 必须 bump（旧 frame 失效）"
        );
        // layout_generation 也应变化（旧 generation 释放，新 generation 分配）
        assert_ne!(
            new_frame.layout_snapshot.layout_generation, old_generation,
            "emit_content_changed 后 layout_generation 必须变化（旧 frame 失效）"
        );
        println!(
            "[BEHAVIOR_VERIFY] emit_content_changed: old frame invalidated (revision {} -> {}, generation {} -> {})",
            revision,
            new_frame.layout_snapshot.text_revision,
            old_generation,
            new_frame.layout_snapshot.layout_generation
        );
    });
}

/// 连续两次 emit_content_changed，每次 text_revision 都增加，
/// prepared_frame 始终为 Some（新 frame）。
#[test]
fn emit_content_changed_twice_bumps_revision_each_time() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("测试文本"));

        item.emit_content_changed();
        let rev1 = item
            .prepared_frame
            .as_ref()
            .expect("第一次 emit 后 prepared_frame 应为 Some")
            .layout_snapshot
            .text_revision;

        item.emit_content_changed();
        assert!(
            item.prepared_frame.is_some(),
            "第二次 emit_content_changed 后 prepared_frame 仍为 Some"
        );
        let rev2 = item
            .prepared_frame
            .as_ref()
            .expect("second frame")
            .layout_snapshot
            .text_revision;
        assert_ne!(
            rev2, rev1,
            "第二次 emit_content_changed 后 text_revision 必须再次增加"
        );
        println!(
            "[BEHAVIOR_VERIFY] emit_content_changed twice: revision {} -> {}",
            rev1, rev2
        );
    });
}

// =========================================================================
// 测试 2: build_render_plan_full 产出 drawn_caret_rect
// =========================================================================

/// `build_render_plan_full()` 必须产出 `drawn_caret_rect: Some((x, y, h))`，
/// 不接受 None。这是本帧真正绘制出去的 caret rect。
#[test]
fn build_render_plan_full_produces_drawn_caret_rect() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello"));

        let cursor_render_state = CursorRenderState::default();
        let selection_preedit = SelectionPreeditPlan::default();
        let cursor_style = CursorStyle::default();
        let selection_preedit_style = SelectionPreeditStyle::default();
        let frame_now = Instant::now();

        let plan = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                cursor_render_state,
                selection_preedit,
                cursor_style,
                selection_preedit_style,
                false,
                frame_now,
                None,
            );

        assert!(
            plan.drawn_caret_rect.is_some(),
            "build_render_plan_full 必须产出 drawn_caret_rect (Some)"
        );
        let (x, y, h) = plan.drawn_caret_rect.expect("drawn_caret_rect 已设");
        assert!(x.is_finite(), "drawn_caret_rect.x 必须有限，实际: {}", x);
        assert!(y.is_finite(), "drawn_caret_rect.y 必须有限，实际: {}", y);
        println!(
            "[BEHAVIOR_VERIFY] build_render_plan_full: drawn_caret_rect=({:.4}, {:.4}, {:.4})",
            x, y, h
        );
    });
}

/// `build_render_plan_full` 传入不同 cursor_render_state 时，
/// drawn_caret_rect 反映传入的 cursor 位置。
#[test]
fn build_render_plan_full_drawn_caret_reflects_cursor_state() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello"));

        let cursor_render_state = CursorRenderState {
            visible: true,
            x: 42.0,
            y: 17.0,
            h: 22.0,
            opacity: 1.0,
        };
        let plan = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                cursor_render_state,
                SelectionPreeditPlan::default(),
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                false,
                Instant::now(),
                None,
            );
        let (x, _y, _h) = plan.drawn_caret_rect.expect("drawn_caret_rect 已设");
        assert!(
            (x - 42.0).abs() < 0.01,
            "drawn_caret_rect.x 应反映传入的 cursor_render_state.x=42.0，实际: {:.4}",
            x
        );
        println!(
            "[BEHAVIOR_VERIFY] build_render_plan_full: drawn_caret_rect reflects cursor state"
        );
    });
}

// ── Issue #826 评论 36：独立 cursor timeline 的每帧唯一采样点 ──

/// 共享 fixture：一条 `start_x -> target_x` 的 Tween plan。
///
/// `apply_plan()` 只创建 Tween（`progress = 0`、`started_at = None`），
/// 真正的推进必须由 `CursorController::tick_animation(frame_now)` 完成。
fn cursor_tween_plan(
    start_x: f64,
    target_x: f64,
    duration_ms: u64,
) -> super::cursor_animation::CursorAnimationPlan {
    use super::cursor_animation::{CursorAnimationPlan, CursorTransition};
    use super::edit_motion::CursorRect;
    CursorAnimationPlan {
        should_be_visible: true,
        transition: CursorTransition::Tween {
            old_rect: CursorRect {
                x: start_x,
                top: 0.0,
                bottom: 20.0,
                baseline_y: 16.0,
            },
            new_rect: CursorRect {
                x: target_x,
                top: 0.0,
                bottom: 20.0,
                baseline_y: 16.0,
            },
            duration_ms,
        },
        cursor_x: target_x,
        cursor_y: 0.0,
        cursor_h: 20.0,
        cursor_baseline_y: 16.0,
        hidden_by_selection: false,
    }
}

/// 评论 36 要求①：视觉光标 Tween 必须跨 Scene Graph 帧真实推进。
///
/// 直接构造 `visual_x = 0`、`target_x = 100`、`duration = 100ms`，用
/// `apply_plan()` 创建 Tween，然后逐帧 tick：t0 记起点、t0+50ms 必须在
/// 0..100 中间、t0+100ms 必须精确落 target 并清 animation。
#[test]
fn cursor_tween_progresses_across_scene_graph_frames() {
    run_on_qt_thread(|| {
        use super::cursor_controller::CursorController;

        let mut ctrl = CursorController::new();
        ctrl.visible = true;
        ctrl.visual_x = 0.0;
        ctrl.visual_y = 0.0;
        ctrl.apply_plan(&cursor_tween_plan(0.0, 100.0, 100));

        let anim = ctrl.animation.as_ref().expect("apply_plan 必须创建 Tween");
        assert_eq!(
            anim.started_at, None,
            "Tween 刚创建时 started_at 必须是 None（等首帧 frame_now 启动）"
        );
        assert_eq!(anim.progress, 0.0, "Tween 刚创建时 progress 必须是 0");
        assert_eq!(anim.duration_ms, 100, "duration_ms 必须来自 plan");
        assert_eq!(anim.target_x, 100.0, "target_x 必须来自 plan");

        let t0 = Instant::now();
        assert!(ctrl.tick_animation(t0), "首帧 tick 后动画仍在进行");
        assert_eq!(
            ctrl.animation.as_ref().unwrap().started_at,
            Some(t0),
            "首帧必须用本帧 frame_now 初始化 started_at"
        );
        assert_eq!(ctrl.visual_x, 0.0, "首帧仍停在 start_x");

        let mid = t0 + Duration::from_millis(50);
        assert!(ctrl.tick_animation(mid), "半程 tick 后动画仍在进行");
        assert!(
            ctrl.visual_x > 0.0 && ctrl.visual_x < 100.0,
            "半程必须真实推进到 0..100 中间，实际 {}",
            ctrl.visual_x
        );

        let end = t0 + Duration::from_millis(100);
        assert!(!ctrl.tick_animation(end), "到点后 tick 必须返回 false");
        assert!(
            (ctrl.visual_x - 100.0).abs() < 1e-9,
            "到点必须精确落 target_x，实际 {}",
            ctrl.visual_x
        );
        assert!(ctrl.animation.is_none(), "到点必须清掉 animation");

        println!("[BEHAVIOR_VERIFY] 评论36 ① cursor tween 跨帧推进 ok");
    });
}

/// 评论 36 要求②：render state 读的是**本帧已推进**的 visual，不是陈旧 visual。
///
/// 50ms tick 之后马上取 `build_cursor_render_state_for_frame()`，
/// 断言 `state.x == cursor_ctrl.visual_x`、`state.x != start_x`、`state.x != target_x`。
#[test]
fn render_state_uses_sampled_cursor_position_not_stale_visual() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.cursor_ctrl.visible = true;
        item.cursor_ctrl.visual_x = 0.0;
        item.cursor_ctrl.visual_y = 0.0;
        item.cursor_ctrl
            .apply_plan(&cursor_tween_plan(0.0, 100.0, 100));

        let t0 = Instant::now();
        item.cursor_ctrl.tick_animation(t0);
        item.cursor_ctrl
            .tick_animation(t0 + Duration::from_millis(50));

        let state = item.build_cursor_render_state_for_frame();
        assert!(
            (state.x - item.cursor_ctrl.visual_x).abs() < 1e-9,
            "render state 必须读本帧已推进的 visual，实际 state.x={} visual_x={}",
            state.x,
            item.cursor_ctrl.visual_x
        );
        assert!(
            state.x.abs() > 1e-6,
            "中间帧不得仍停在 start_x（陈旧 visual），实际 {}",
            state.x
        );
        assert!(
            (state.x - 100.0).abs() > 1e-6,
            "中间帧不得已经等于 target_x，实际 {}",
            state.x
        );

        println!("[BEHAVIOR_VERIFY] 评论36 ② render state 读采样后的位置 ok");
    });
}

/// 评论 36 要求③：真实 `click_at` 路径 —— 逻辑 caret 立即变，视觉 caret 随之追上。
#[test]
fn pointer_click_moves_visual_caret_after_logical_cursor_changed() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello World"));
        item.current_viewport_height = 600.0;
        item.set_cursor_animation_duration_ms(120);

        item.click_at(0.0, 0.0, false);
        let old_logical = item.pipeline.cursor();
        let old_visual = item.cursor_ctrl.visual_x;

        item.click_at(300.0, 10.0, false);
        let new_logical = item.pipeline.cursor();
        assert_ne!(
            new_logical, old_logical,
            "点击后逻辑 caret 必须立即改变（old={} new={}）",
            old_logical, new_logical
        );
        let target = item.cursor_ctrl.target_x;
        assert!(
            item.cursor_ctrl.animation.is_some(),
            "指针点击移动 caret 必须创建视觉 Tween（不能只创建不推进）"
        );

        let t0 = Instant::now();
        item.cursor_ctrl.tick_animation(t0);
        item.cursor_ctrl
            .tick_animation(t0 + Duration::from_millis(60));
        let mid = item.cursor_ctrl.visual_x;
        assert!(
            (mid - old_visual).abs() > 1e-6,
            "视觉光标必须朝新 caret 移动，mid={} old={}",
            mid,
            old_visual
        );
        assert!(
            (mid - target).abs() > 1e-6,
            "半程不得直接跳到 target，mid={} target={}",
            mid,
            target
        );

        item.cursor_ctrl
            .tick_animation(t0 + Duration::from_millis(400));
        assert!(
            (item.cursor_ctrl.visual_x - target).abs() < 1e-9,
            "最终视觉 caret 必须等于 target，实际 {} vs {}",
            item.cursor_ctrl.visual_x,
            target
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "追上后必须清掉 animation（否则尾部条件会空转重绘）"
        );

        println!("[BEHAVIOR_VERIFY] 评论36 ③ pointer click 视觉 caret 追随 ok");
    });
}

/// 评论 36 要求④：方向键走**同一条** cursor timeline（不为鼠标/键盘造两套）。
#[test]
fn keyboard_cursor_move_uses_same_timeline() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello"));
        item.current_viewport_height = 600.0;
        item.set_cursor_animation_duration_ms(120);

        item.click_at(0.0, 0.0, false);
        // 收干净点击留下的状态，保证接下来的 Tween 完全由键盘移动产生。
        item.cursor_ctrl.finish_animation_to_target();
        item.cursor_ctrl.force_snap_next = false;

        let before = item.pipeline.cursor();
        item.move_cursor_horizontal(true, false);
        let after = item.pipeline.cursor();
        assert!(
            after > before,
            "方向键必须先改逻辑 caret：{} -> {}",
            before,
            after
        );
        assert!(
            item.cursor_ctrl.animation.is_some(),
            "键盘移动必须复用同一条 cursor timeline（创建 Tween）"
        );
        let target = item.cursor_ctrl.target_x;

        let t0 = Instant::now();
        item.cursor_ctrl.tick_animation(t0);
        item.cursor_ctrl
            .tick_animation(t0 + Duration::from_millis(400));
        assert!(
            (item.cursor_ctrl.visual_x - target).abs() < 1e-9,
            "键盘移动后视觉 caret 必须追到 target，实际 {} vs {}",
            item.cursor_ctrl.visual_x,
            target
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "追上后必须清 animation"
        );

        println!("[BEHAVIOR_VERIFY] 评论36 ④ keyboard 走同一条 timeline ok");
    });
}

/// 补充（未覆盖位置：评论 36 要求的 4 条全部由测试**手动**调 `tick_animation`，
/// 没有一条验证生产渲染链 `update_paint_node` 真的每帧调了它 —— 而 BLOCKER 本身
/// 正是「生产路径没有任何调用点」。同时覆盖改法 4：推进不在回写方法里、
/// 旧 `CursorSampleOutcome` 分支不得复活）。
#[test]
fn scene_graph_frame_is_the_only_production_cursor_timeline_sample_point() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sujian_editor_item/qquickitem_impl.rs"
    ))
    .unwrap_or_else(|e| panic!("read qquickitem_impl.rs failed: {}", e));

    const CALL: &str = "self.cursor_ctrl.tick_animation(frame_now);";
    let call_count = src.matches(CALL).count();
    assert_eq!(
        call_count, 1,
        "生产每帧只允许一个 cursor timeline 推进调用点，实际 {}",
        call_count
    );

    let frame_now_pos = src
        .find("let frame_now = frame_start;")
        .expect("整帧唯一的 frame_now 必须存在");
    let tick_pos = src
        .find(CALL)
        .expect("update_paint_node 必须调用 cursor_ctrl.tick_animation(frame_now)");
    let state_pos = src
        .find("let cursor_render_state = self.build_cursor_render_state_for_frame();")
        .expect("build_cursor_render_state_for_frame 必须存在");
    assert!(
        frame_now_pos < tick_pos && tick_pos < state_pos,
        "顺序必须是 frame_now -> tick_animation -> build_cursor_render_state_for_frame"
    );

    assert!(
        !src.contains("CursorSampleOutcome"),
        "旧的 CursorSampleOutcome（正文事务驱动 caret progress）不得复活"
    );
    assert!(
        !src.contains("update_animation_progress("),
        "推进 progress 不得塞回 item 层（回写方法只同步 drawn caret）"
    );

    println!("[BEHAVIOR_VERIFY] 评论36 补充：生产采样点唯一且顺序正确");
}

/// 补充（未覆盖位置：要求①只测一条**不中断**的 Tween，没测中途第二次移动的
/// rebase —— rebase 必须重置 `started_at = None` 等下一帧，并从当前 visual 继续，
/// 不能把 timeline 接到旧起点上）。
#[test]
fn cursor_tween_rebase_midway_restarts_timeline_from_current_visual() {
    run_on_qt_thread(|| {
        use super::cursor_controller::CursorController;

        let mut ctrl = CursorController::new();
        ctrl.visible = true;
        ctrl.visual_x = 0.0;
        ctrl.visual_y = 0.0;
        ctrl.apply_plan(&cursor_tween_plan(0.0, 100.0, 200));

        let t0 = Instant::now();
        ctrl.tick_animation(t0);
        ctrl.tick_animation(t0 + Duration::from_millis(100));
        let mid = ctrl.visual_x;
        assert!(
            mid > 0.0 && mid < 100.0,
            "前置：半程必须在 0..100 中间，实际 {}",
            mid
        );

        // 中途第二次移动（target 100 -> 200）。
        ctrl.apply_plan(&cursor_tween_plan(mid, 200.0, 200));
        assert_eq!(
            ctrl.animation.as_ref().unwrap().started_at,
            None,
            "rebase 必须把 started_at 重置为 None 等下一帧 frame_now"
        );
        let t1 = t0 + Duration::from_millis(120);
        assert!(ctrl.tick_animation(t1), "rebase 后首帧 tick 仍在进行");
        assert_eq!(
            ctrl.animation.as_ref().unwrap().started_at,
            Some(t1),
            "rebase 后的首帧用新 frame_now 启动 timeline"
        );
        let settled = ctrl.visual_x;
        assert!(
            (settled - mid).abs() < 1e-6,
            "rebase 后首帧必须停在当前 visual（mid={} settled={}）",
            mid,
            settled
        );

        ctrl.tick_animation(t1 + Duration::from_millis(100));
        let later = ctrl.visual_x;
        assert!(
            later > settled && later < 200.0,
            "rebase 后必须朝新 target 继续推进，实际 {}（settled={}）",
            later,
            settled
        );

        ctrl.tick_animation(t1 + Duration::from_millis(400));
        assert!(
            (ctrl.visual_x - 200.0).abs() < 1e-9,
            "最终必须落新 target，实际 {}",
            ctrl.visual_x
        );
        assert!(ctrl.animation.is_none(), "到点必须清 animation");

        println!("[BEHAVIOR_VERIFY] 评论36 补充：rebase 重置 timeline 并从当前 visual 继续");
    });
}

/// 补充（未覆盖位置：要求的 4 条只断「Tween 推到 target」，没断**渲染循环契约** ——
/// 动画没结束时尾部条件必须继续 `request_frame_update()`，结束后必须不再空转；
/// 且 `build_cursor_render_state_for_frame()` 最终读到的仍是 target 而非被回写拉回旧值）。
#[test]
fn cursor_timeline_finish_stops_frame_requests_and_keeps_target_render_state() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.cursor_ctrl.visible = true;
        item.cursor_ctrl.visual_x = 0.0;
        item.cursor_ctrl.visual_y = 0.0;
        item.cursor_ctrl
            .apply_plan(&cursor_tween_plan(0.0, 100.0, 100));

        let t0 = Instant::now();
        item.cursor_ctrl.tick_animation(t0);
        let mid_now = t0 + Duration::from_millis(50);
        item.cursor_ctrl.tick_animation(mid_now);

        // update_paint_node 尾部条件：正文动画 inactive 时，是否继续请求下一帧
        // 完全由 cursor_ctrl.animation 是否还在决定。
        let coord_active = item
            .pipeline
            .animation_coordinator()
            .has_active_text_animation(mid_now);
        assert!(
            !coord_active,
            "前置：本测试没有正文编辑，正文动画应 inactive"
        );
        assert!(
            coord_active || item.cursor_ctrl.animation.is_some(),
            "动画未结束时尾部条件必须为真（继续 request_frame_update）"
        );

        let end = t0 + Duration::from_millis(200);
        item.cursor_ctrl.tick_animation(end);
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "到点后 animation 必须清掉，否则尾部条件永久为真 -> 空转重绘"
        );
        assert!(
            !(coord_active || item.cursor_ctrl.animation.is_some()),
            "动画结束后尾部条件必须为假（不再 request_frame_update）"
        );

        let state = item.build_cursor_render_state_for_frame();
        assert!(
            (state.x - 100.0).abs() < 1e-9,
            "结束后的 render state 必须仍是 target，实际 {}",
            state.x
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "回写不得复活 animation"
        );

        println!("[BEHAVIOR_VERIFY] 评论36 补充：结束后停止 request_frame_update");
    });
}
