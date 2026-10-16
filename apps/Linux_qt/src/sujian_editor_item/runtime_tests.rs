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

// Issue #919 复核：每个 cpp! 块独立编译，本文件自查缓存的 QImage 内容时
// 需要在本文件内 include 头文件。
cpp! {{
    #include <QtGui/QImage>
}}

/// Issue #919 复核：`LineTextureStore::contains()` 本质是 `HashMap::contains_key`，
/// 只证明缓存里有这个 id 的条目，并不证明存进去的是含像素的行图。此前
/// `real_edit_prepares_ready_source_textures_for_animation_layer` 只按缓存键判断
/// "源纹理已就绪"，等于把"已建立 TextMotion 结构"当成"已经有动画"。本辅助直接读
/// 缓存里的 `QImage`，检查其非 null 且宽高为正，关闭这条假阳性边界。
fn line_image_has_pixels(image: &qmetaobject::QImage) -> bool {
    let ptr = image as *const qmetaobject::QImage;
    cpp!(unsafe [ptr as "const QImage*"] -> bool as "bool" {
        return ptr != nullptr && !ptr->isNull() && ptr->width() > 0 && ptr->height() > 0;
    })
}

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
/// `prepare_editor_frame` 用当前 text_revision 重新准备 frame。
///
/// Issue #901 第六轮阻塞1: emit_content_changed 不再 bump_text_revision。
/// text_revision 由编辑入口（record_transaction / IME commit / Load）通过
/// set_text_revision 绑定 Core new_revision。此测试验证 emit_content_changed
/// 仍刷新 prepared_frame（layout_generation 变化），但 text_revision 保持不变。
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
        // Issue #901 阻塞1: text_revision 不 bump（由编辑入口绑定 Core revision）
        assert_eq!(
            new_frame.layout_snapshot.text_revision, revision,
            "emit_content_changed 不 bump text_revision（由编辑入口绑定 Core revision）"
        );
        // layout_generation 仍应变化（旧 generation 释放，新 generation 分配）
        assert_ne!(
            new_frame.layout_snapshot.layout_generation, old_generation,
            "emit_content_changed 后 layout_generation 必须变化（旧 frame 失效）"
        );
        println!(
            "[BEHAVIOR_VERIFY] emit_content_changed: old frame invalidated (revision {} == {}, generation {} -> {})",
            revision,
            new_frame.layout_snapshot.text_revision,
            old_generation,
            new_frame.layout_snapshot.layout_generation
        );
    });
}

/// 连续两次 emit_content_changed 不再 bump text_revision（Issue #901 第六轮阻塞1）。
/// text_revision 由编辑入口绑定 Core new_revision，emit_content_changed 只做
/// UI 通知与 prepared frame 刷新。此测试验证连续 emit 不额外推进正文版本，
/// prepared_frame 始终为 Some（新 frame）。
#[test]
fn emit_content_changed_twice_does_not_bump_revision() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("测试文本"));

        let rev_before = item.pipeline.text_revision();
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
        assert_eq!(
            rev1, rev_before,
            "emit_content_changed 不 bump text_revision（由编辑入口绑定）"
        );
        assert_eq!(
            rev2, rev1,
            "连续 emit_content_changed 不额外推进 text_revision"
        );
        println!(
            "[BEHAVIOR_VERIFY] emit_content_changed twice: revision {} == {} == {}",
            rev_before, rev1, rev2
        );
    });
}

// =========================================================================
// 测试 2: build_render_plan_full 产出 drawn_caret_rect
// =========================================================================

/// `build_render_plan_full()` 必须产出 `drawn_caret_rect: Some((x, y, h))`，
/// 不接受 None。这是本帧真正绘制出去的 caret rect。
// TODO(issue-904): 旧 animation_coordinator 已删除；正文/光标动画由唯一的
// text_motion 计划与 cursor_ctrl 驱动。
#[cfg(any())]
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
// TODO(issue-904): 旧 animation_coordinator 已删除；正文/光标动画由唯一的
// text_motion 计划与 cursor_ctrl 驱动。
#[cfg(any())]
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
            ..CursorRenderState::default()
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
        movement_source: super::cursor_controller::CursorMoveSource::LayoutChange,
        driver_revision: None,
        driver_session: 0,
        driver_transition_id: 0,
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

        let state = item.build_cursor_render_state_for_frame(
            &crate::sujian_editor_item::text_motion::TextMotionFrame::default(),
        );
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
///
/// Issue #919：`text_motion` 协同进度已接入，`PointerClick` 不再被统一下压成
/// Snap，重新启用这条 Tween 回归。
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
///
/// Issue #919：`text_motion` 协同进度已接入，`KeyboardNavigation` 恢复独立
/// Tween，不再被统一下压成 Snap。
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
        .find("let cursor_render_state = self.build_cursor_render_state_for_frame(&motion_frame);")
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
// TODO(issue-904): 旧 animation_coordinator 已删除；帧请求由唯一的
// text_motion 计划的 needs_next_frame 与 cursor_ctrl.animation 驱动。
#[cfg(any())]
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

        let state = item.build_cursor_render_state_for_frame(
            &crate::sujian_editor_item::text_motion::TextMotionFrame::default(),
        );
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

/// Issue #919：协同模式下**整个光标接线**——正文事务的光标用 `text_motion`
/// 的 `caret_sample` 推进，真实鼠标点击不被正文动画接管，仍用 `cursor_ctrl`
/// 自己的视觉位置，并且点击不清掉进行中的正文动画计划。
///
/// 此前该测试只验证 `TextMotion` 采样几何，并用测试伪造的 `last_move_source`
/// 断言"状态选择"；这里补上真实 `click_at` 路径对计划生命周期的验证。
#[test]
fn coordinated_text_transaction_cursor_follows_motion_caret_sample() {
    run_on_qt_thread(|| {
        use super::cursor_controller::CursorMoveSource;
        use super::layout_snapshot::SourceRect;
        use super::text_motion::{TextMotion, TextMotionFrame};

        // 安装一份进行中的正文动画计划。`text_motion` 内部几何由其自身单元测试
        // 覆盖，这里只验证光标接线与真实点击后的计划生命周期。
        let install_motion = |item: &mut SujianEditorItem| {
            *item.pipeline.text_motion_mut() = Some(TextMotion {
                slices: Vec::new(),
                started_ms: 0.0,
                duration_ms: 100,
                session: item.document_session,
                target_revision: 0,
                viewport: SourceRect {
                    x: 0.0,
                    y: 0.0,
                    w: 400.0,
                    h: 600.0,
                },
                static_exclusions: Vec::new(),
                required_textures: Vec::new(),
                caret_from: Some((10.0, 5.0)),
                caret_to: Some((40.0, 20.0)),
                paragraph_moves: Vec::new(),
            });
        };

        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello World"));
        item.current_viewport_height = 600.0;
        item.set_cursor_animation_duration_ms(120);
        item.cursor_ctrl.visible = true;
        item.cursor_ctrl.visual_x = 111.0;
        item.cursor_ctrl.visual_y = 222.0;

        // 协同动画默认开启，且本帧确有唯一的正文动画计划。
        assert!(
            item.effective_animation().shared_timeline,
            "前置：协同动画默认开启"
        );
        install_motion(&mut item);

        let frame = TextMotionFrame {
            caret_sample: Some((35.0, 17.5)),
            ..TextMotionFrame::default()
        };

        // ① 正文事务（TextTransaction）：光标用正文动画的同一进度采样点。
        item.cursor_ctrl.last_move_source = CursorMoveSource::TextTransaction;
        let state = item.build_cursor_render_state_for_frame(&frame);
        assert!(
            (state.x - 35.0).abs() < 1e-9 && (state.y - 17.5).abs() < 1e-9,
            "协同正文事务光标必须读 caret_sample，实际 ({}, {})",
            state.x,
            state.y
        );
        assert!(
            item.pipeline.text_motion().is_some(),
            "采样必须基于仍存在的正文动画计划"
        );

        // ② 真实 `click_at` 路径：点击独立于正文动画，且不清掉进行中的计划。
        // 第一次点击消费初始 force-snap，第二次点击才建立独立视觉 Tween。
        item.click_at(0.0, 0.0, false);
        assert!(
            item.pipeline.text_motion().is_some(),
            "点击不得清掉进行中的正文动画计划"
        );
        item.click_at(300.0, 10.0, false);
        assert_eq!(
            item.cursor_ctrl.last_move_source,
            CursorMoveSource::PointerClick,
            "真实 click_at 必须把来源标为 PointerClick"
        );
        assert!(
            item.pipeline.text_motion().is_some(),
            "真实点击后正文动画计划仍在（点击不拥有正文动画）"
        );
        assert!(
            item.cursor_ctrl.animation.is_some(),
            "点击必须建立独立于正文动画的光标 Tween"
        );

        let state = item.build_cursor_render_state_for_frame(&frame);
        assert!(
            (state.x - item.cursor_ctrl.visual_x).abs() < 1e-9
                && (state.y - item.cursor_ctrl.visual_y).abs() < 1e-9,
            "点击源必须用 cursor_ctrl 自己的视觉位置，而非正文 caret_sample；\
             实际 state=({}, {}) visual=({}, {})",
            state.x,
            state.y,
            item.cursor_ctrl.visual_x,
            item.cursor_ctrl.visual_y
        );

        println!("[BEHAVIOR_VERIFY] #919 协同 caret_sample 与真实点击独立接线 ok");
    });
}

/// Issue #919 复核：**真实编辑入口**（而非测试安装的空计划）必须构造出含字形
/// 切片的非空 `TextMotion`，协同光标再用该计划的同一进度采样。
///
/// 上一轮 `coordinated_text_transaction_cursor_follows_motion_caret_sample` 只安装
/// 了一份 `slices` 为空的计划，复核据此指出它"不模拟从真实编辑构造非空字形动画"。
/// 本用例走 `insert_text` / `delete_backward` 生产路径，确认 `begin_or_retarget`
/// 真的由 Core `display_patches` 生成 Reveal/Conceal 覆盖层，计划的 `caret_sample`
/// 随同一 progress 推进，正文事务光标读取它。
///
/// 边界说明：仍在 Qt 布局/runtime 层验证"真实编辑 → 非空计划 → 光标采样"，
/// 不在此单元测试中真正把 QSG 场景图提交到渲染线程。
#[test]
fn real_edit_builds_non_empty_motion_and_caret_follows_it() {
    run_on_qt_thread(|| {
        use super::cursor_controller::CursorMoveSource;
        use super::text_motion::SliceMotion;

        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("甲乙"));
        item.current_viewport_height = 600.0;
        item.set_cursor_animation_duration_ms(120);
        assert!(item.effective_animation().text, "前置：输入动画默认开启");

        // 光标放到文本末尾（走真实 hit-test/click 入口）。
        item.click_at(1000.0, 0.0, false);
        assert_eq!(item.pipeline.cursor(), 6, "前置：光标落在「甲乙」末尾");

        // 第一笔真实插入：必须由编辑本身构造出含字形切片的计划。
        item.insert_text(QString::from("丙"));
        assert_eq!(
            item.cursor_ctrl.last_move_source,
            CursorMoveSource::TextTransaction,
            "真实插入必须把光标来源标为 TextTransaction"
        );
        let motion = item
            .pipeline
            .text_motion()
            .cloned()
            .expect("真实插入必须建立唯一的局部动画计划");
        assert!(
            !motion.slices.is_empty(),
            "真实编辑的字形动画不得为空（slices={}）",
            motion.slices.len()
        );
        assert!(
            motion.slices.iter().any(|s| matches!(
                s,
                SliceMotion::Reveal { byte_range, .. } if *byte_range == (6, 9)
            )),
            "新输入「丙」必须产生覆盖字节区间 (6,9) 的 Reveal，实际 {:?}",
            motion.slices
        );
        assert!(
            motion.required_textures.iter().all(|id| !id.is_none()),
            "真实编辑计划必须引用有效（非无纹理哨兵）的源行 id"
        );
        assert!(
            motion.caret_to.is_some(),
            "真实编辑计划必须带新布局 caret 终点"
        );

        // 计划的 sample() 在同一 progress 上产出真实覆盖层，中途帧确有像素。
        let frame = motion.sample(motion.started_ms + f64::from(motion.duration_ms) / 2.0);
        assert!(
            !frame.overlays.is_empty(),
            "真实编辑计划的中间帧必须产出字形覆盖层"
        );
        let sampled = frame.caret_sample.expect("真实编辑计划必须带 caret 采样");

        // 正文事务光标读该计划的 caret_sample，而不是 cursor_ctrl 的静态视觉位置。
        let state = item.build_cursor_render_state_for_frame(&frame);
        assert!(
            (state.x - sampled.0).abs() < 1e-9 && (state.y - sampled.1).abs() < 1e-9,
            "正文事务光标必须读真实编辑计划的 caret_sample {:?}，实际 ({}, {})",
            sampled,
            state.x,
            state.y
        );

        // 第二笔真实删除：从上一笔的新布局继续，生成旧字 Conceal。
        item.delete_backward();
        let motion = item
            .pipeline
            .text_motion()
            .cloned()
            .expect("真实删除必须建立局部动画计划");
        assert!(!motion.slices.is_empty(), "真实删除的字形动画不得为空");
        assert!(
            motion
                .slices
                .iter()
                .any(|s| matches!(s, SliceMotion::Conceal { .. })),
            "真实删除必须产生旧字 Conceal，实际 {:?}",
            motion.slices
        );

        println!("[BEHAVIOR_VERIFY] #919 真实编辑构造非空字形动画且协同光标跟随同一采样 ok");
    });
}

/// Issue #919 复核：真实编辑必须把本帧接管所需的**源行纹理**准备好放进
/// `texture_cache` —— 这是 Rust 侧在向 QSG 提交动画层之前的最后一步。
///
/// 上一轮 `real_edit_builds_non_empty_motion_and_caret_follows_it` 只断言
/// `required_textures` 引用的是非哨兵行 id，并未验证这些纹理真的已在缓存中就绪。
/// `scene_graph_renderer::render_frame` 的提交前提是
/// `LocalTextPlan::has_animation_resources == true`：缺任一张源纹理时
/// `prepare_text_animation_layer` 会 `continue` 掉缺纹理的 glyph，静态层画完整
/// canonical、动画层一帧也提交不出去。本用例走真实 insert/delete 路径，确认编辑
/// 结束后计划引用的旧/新源行纹理都已就绪，且中间帧确实存在具备可见面积的覆盖层。
///
/// 边界说明：仍在 Qt 布局/runtime 层验证"真实编辑 → 资源就绪 → 可提交动画层"，
/// 不真正把 QSG 场景图提交到渲染线程（那需要活的 QQuickWindow 与渲染线程帧）。
#[test]
fn real_edit_prepares_ready_source_textures_for_animation_layer() {
    run_on_qt_thread(|| {
        use super::render_plan::LocalTextPlan;

        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("甲乙"));
        item.current_viewport_height = 600.0;
        item.set_cursor_animation_duration_ms(120);
        item.click_at(1000.0, 0.0, false);

        // 检查当前计划在其采样帧上，接管所需的源纹理是否全部就绪，
        // 且中间帧确实存在可提交的可见覆盖层。
        let assert_committable = |item: &SujianEditorItem, label: &str| {
            let motion = item
                .pipeline
                .text_motion()
                .cloned()
                .unwrap_or_else(|| panic!("{label}: 必须存在局部动画计划"));
            let frame = motion.sample(motion.started_ms + f64::from(motion.duration_ms) / 2.0);
            let plan = LocalTextPlan {
                overlays: frame.overlays.clone(),
                static_holes: frame.static_holes.clone(),
                required_textures: frame.required_textures.clone(),
                paragraph_y_offsets: frame.paragraph_y_offsets.clone(),
            };
            let cache = item.pipeline.texture_cache();
            let missing = plan.missing_textures(cache);
            assert!(
                missing.is_empty(),
                "{label}: 真实编辑后动画源纹理必须已就绪，缺失 {missing:?}"
            );
            assert!(
                plan.has_animation_resources(cache),
                "{label}: has_animation_resources 必须为真，否则 QSG 动画层不会提交"
            );
            // `has_animation_resources` 只按键存在判断；这里进一步确认每个必需源
            // 纹理确实是含像素的行图，而不是空 QImage 撑起的假"已就绪"。
            for id in &plan.required_textures {
                let image = cache
                    .get_line(id)
                    .unwrap_or_else(|| panic!("{label}: 源纹理 {id:?} 必须在缓存中"));
                assert!(
                    line_image_has_pixels(image),
                    "{label}: 源纹理 {id:?} 必须是含像素的行图，不能是空 QImage"
                );
            }
            assert!(
                frame
                    .overlays
                    .iter()
                    .any(|g| g.clip_rect.w > 0.0 && g.clip_rect.h > 0.0),
                "{label}: 中间帧必须存在具备可见面积的覆盖层，实际 {:?}",
                frame.overlays
            );
        };

        // 插入：新字 Reveal 的源纹理必须就绪。
        item.insert_text(QString::from("丙"));
        assert_committable(&item, "插入");

        // 删除：从前一笔的新布局继续，旧字 Conceal 的源纹理也必须就绪。
        item.delete_backward();
        assert_committable(&item, "删除");

        println!("[BEHAVIOR_VERIFY] #919 真实编辑的动画源纹理就绪、覆盖层可提交 QSG 动画层 ok");
    });
}

/// Issue #919 复核：`has_animation_resources` / `missing_textures` 是"静态孔洞与
/// 动画层同帧原子提交"的 Rust 侧闸门 —— 缺任一源纹理时必须整体不接管，静态层画
/// 完整 canonical，绝不允许"静态挖了空但动画没画"。上一轮只在真实编辑的正向路径
/// 断言了闸门为真，这里补上反向路径：空缓存/只补一侧时闸门必须为假且逐一列出缺失的
/// `LineSnapshotId`，两侧补齐后才为真。
#[test]
fn missing_source_texture_keeps_animation_plan_not_ready() {
    use super::render_plan::LocalTextPlan;
    use super::snapshot_id::LineSnapshotId;
    use super::texture_cache::TextureCache;

    let id_old = LineSnapshotId::new(7, 1, 0);
    let id_new = LineSnapshotId::new(8, 1, 0);
    let plan = LocalTextPlan {
        required_textures: vec![id_old, id_new],
        ..LocalTextPlan::default()
    };

    let one_pixel = || {
        qmetaobject::QImage::new(
            qmetaobject::QSize {
                width: 1,
                height: 1,
            },
            qmetaobject::ImageFormat::ARGB32_Premultiplied,
        )
    };

    run_on_qt_thread(move || {
        let mut cache = TextureCache::new();

        // 空缓存：缺任一张源纹理时整体不接管，并逐一列出缺失 id。
        assert!(
            !plan.has_animation_resources(&cache),
            "缺源纹理时不得判定为可接管"
        );
        assert_eq!(plan.missing_textures(&cache), vec![id_old, id_new]);

        // 只补旧侧：仍缺新侧，闸门保持关闭。
        cache.insert_line(id_old, one_pixel());
        assert!(
            !plan.has_animation_resources(&cache),
            "只补一侧仍不得判定为可接管"
        );
        assert_eq!(plan.missing_textures(&cache), vec![id_new]);

        // 两侧补齐后才允许同帧提交静态孔洞与动画层。
        cache.insert_line(id_new, one_pixel());
        assert!(plan.has_animation_resources(&cache));
        assert!(plan.missing_textures(&cache).is_empty());

        println!("[BEHAVIOR_VERIFY] #919 缺源纹理时动画计划保持不接管并列出缺失 id ok");
    });
}

/// Issue #919：关闭正文吞吐动画后，真实编辑入口不得再建立局部动画计划，
/// 因而也不会产生静态孔洞或字形覆盖层，静态层直接画完整 canonical。
///
/// 议题明确要求「关闭文字动画时不生成 holes/动态片段，直接只画 canonical
/// 静态正文」。此前 runtime 用例只覆盖动画开启时的正向路径；本用例补上开关
/// 关闭分支，并确认开关可逆——重新开启后同一编辑入口恢复构造计划。
#[test]
fn typing_animation_disabled_edit_produces_no_motion_or_static_holes() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("甲乙"));
        item.current_viewport_height = 600.0;
        assert!(item.effective_animation().text, "前置：输入动画默认开启");

        // 协同模式是完整模式（`animation::resolve` 恒令 text=true），先退出协同，
        // 独立开关才能表达"关闭正文动画"。
        item.set_coordinated_animation_enabled(false);
        item.set_typing_animation_enabled(false);
        assert!(
            !item.effective_animation().text,
            "退出协同并关闭输入动画后，正文动画策略必须为 false"
        );
        assert!(
            item.pipeline.text_motion().is_none(),
            "关闭动画时不得残留局部动画计划"
        );

        // 真实插入仍会更新正文，但不得建立动画计划。
        item.click_at(1000.0, 0.0, false);
        let snap_before = item
            .pipeline
            .current_layout_snapshot()
            .as_ref()
            .map(|s| s.revision);
        item.insert_text(QString::from("丙"));
        assert!(
            item.pipeline.text_motion().is_none(),
            "关闭文字动画后真实插入不得建立 text_motion 计划"
        );
        // Issue #919：关闭正文动画只影响局部动画计划，不影响 canonical 提交。
        // 原先 `pipeline::tests::layout_revision_commits_even_when_text_animation_is_disabled`
        // 走已被 #919 取代的 `prepare_edit_motion` 整章路径，本用例用当前唯一入口
        // `insert_text` → `record_transaction` 验证同一语义：即使动画关闭，编辑仍安装
        // 一份新的 canonical layout snapshot（其 revision 由 `LayoutRevision::next()`
        // 推进），供后续帧/光标使用，不依赖 text_animation_enabled。
        let snap_after = item
            .pipeline
            .current_layout_snapshot()
            .as_ref()
            .expect("关闭文字动画时真实编辑仍必须安装新的 canonical layout snapshot")
            .revision;
        assert_ne!(
            Some(snap_after),
            snap_before,
            "关闭文字动画时仍必须把新的 canonical layout revision 提交到 pipeline"
        );

        // 与 `qquickitem_impl` 相同的消费路径：无计划 → 默认空帧 → 无孔洞/覆盖层。
        let motion_frame = item
            .pipeline
            .text_motion()
            .map(|motion| motion.sample(0.0))
            .unwrap_or_default();
        assert!(
            motion_frame.static_holes.is_empty(),
            "关闭文字动画时静态层不得挖孔: {:?}",
            motion_frame.static_holes
        );
        assert!(
            motion_frame.overlays.is_empty(),
            "关闭文字动画时不得有字形覆盖层: {:?}",
            motion_frame.overlays
        );

        // 重新开启后，同一编辑入口恢复构造局部动画计划（开关可逆、编辑未受损）。
        item.set_typing_animation_enabled(true);
        assert!(item.effective_animation().text);
        item.insert_text(QString::from("丁"));
        let motion = item
            .pipeline
            .text_motion()
            .cloned()
            .expect("重新开启后真实插入必须恢复建立局部动画计划");
        assert!(
            !motion.slices.is_empty(),
            "重新开启后真实插入的字形动画不得为空"
        );

        println!("[BEHAVIOR_VERIFY] #919 关闭文字动画不生成孔洞/覆盖层且开关可逆 ok");
    });
}

/// Issue #919：连续两笔真实编辑必须走同一条 canonical revision 链。
///
/// 原先 `pipeline::tests::consecutive_pipeline_edits_reuse_previous_target_revision_as_next_base_revision`
/// 在 pipeline 内部用 `prepare_edit_motion` + 整章 `current_layout_snapshot` 驱动，
/// #919 把普通编辑路径收敛到唯一的 `TextMotion` 局部计划后，该 helper 不再经过
/// `prepare_edit_motion`，用例长期 `#[ignore]`、放开即失败。本用例用当前唯一入口
/// （`insert_text` → `transaction.rs::record_transaction`）验证同一语义：
///
/// - 每一笔真实编辑都安装一份新的 canonical layout snapshot（revision 由
///   `LayoutRevision::next()` 推进），第二笔起 `text_revision` 继续严格递增；
/// - 每一笔局部 `TextMotion` 的 `target_revision` 都绑定该笔的 Core new_revision；
/// - 第二笔以上一笔安装的 canonical snapshot 为 base（`current_layout_snapshot`
///   在第二笔前已是第一笔的目标），因此第二笔的 revision 与第一笔不同。
#[test]
fn consecutive_real_edits_commit_canonical_revision_chain() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("甲乙"));
        item.current_viewport_height = 600.0;
        item.click_at(1000.0, 0.0, false);

        let r0 = item.pipeline.text_revision();

        // 第一笔：甲乙 -> 甲乙丙。
        item.insert_text(QString::from("丙"));
        let r1 = item.pipeline.text_revision();
        assert!(
            r1 >= r0,
            "第一笔真实编辑不得回退 text_revision（{r0} -> {r1}）"
        );
        let snap1 = item
            .pipeline
            .current_layout_snapshot()
            .as_ref()
            .expect("第一笔后必须安装新的 canonical layout snapshot")
            .revision;
        let motion1 = item
            .pipeline
            .text_motion()
            .cloned()
            .expect("第一笔必须建立局部动画计划");
        assert_eq!(
            motion1.target_revision, r1,
            "第一笔动画计划的目标 revision 必须绑定该笔 Core new_revision"
        );

        // 第二笔：甲乙丙 -> 甲乙丙丁。base 即第一笔安装的 canonical snapshot。
        item.insert_text(QString::from("丁"));
        let r2 = item.pipeline.text_revision();
        assert!(
            r2 > r1,
            "第二笔真实编辑必须继续推进 text_revision（{r1} -> {r2}）"
        );
        let snap2 = item
            .pipeline
            .current_layout_snapshot()
            .as_ref()
            .expect("第二笔后必须安装新的 canonical layout snapshot")
            .revision;
        assert_ne!(
            snap1, snap2,
            "第二笔必须安装一份新的 canonical layout revision，而不是沿用第一笔目标"
        );
        let motion2 = item
            .pipeline
            .text_motion()
            .cloned()
            .expect("第二笔必须建立局部动画计划");
        assert_eq!(
            motion2.target_revision, r2,
            "第二笔动画计划的目标 revision 必须绑定该笔 Core new_revision"
        );

        println!(
            "[BEHAVIOR_VERIFY] #919 连续两笔真实编辑提交同一条 canonical revision 链 ok"
        );
    });
}
