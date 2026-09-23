//! Issue #707 评论 5724685300 — sujian_editor_item 内部状态测试。
//!
//! 本模块在 `sujian_editor_item` 模块内部（`#[cfg(test)] mod runtime_tests;`），
//! 可以直接访问 `pub(crate)` 字段和方法，不需要把生产内部 API 全暴露出去。
//!
//! 测试真实状态交接:
//! - `emit_content_changed()` 后旧 prepared_frame 失效
//! - `build_render_plan_full()` 产出 drawn_caret_rect
//! - Insert/Delete + click_at/move_cursor_* 的 epoch 变化:
//!   真正改变 caret 时 epoch 变化且旧事务不再驱动 caret;
//!   点击当前位置/已经在边界继续按方向键这种 no-op 时 epoch 必须不变。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::render_plan::{
    CursorRenderState, CursorStyle, FrameContext, PreparedEditorFrame, SelectionPreeditPlan,
    SelectionPreeditStyle,
};
use super::*;
use crate::editor::layout::{run_on_qt_thread, EditorLayout, LayoutParams};
use qmetaobject::QString;
use std::time::Instant;

/// 构造默认 LayoutParams，与 SujianEditorItem::default() 的字体设置一致。
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
        let text = item.buffer.text.clone();
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
        let frame_context = FrameContext::default();
        let cursor_style = CursorStyle::default();
        let selection_preedit_style = SelectionPreeditStyle::default();
        let frame_now = Instant::now();

        let plan = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                cursor_render_state,
                selection_preedit,
                frame_context,
                cursor_style,
                selection_preedit_style,
                frame_now,
                None, // cursor_animation
                0,    // cursor_owner_epoch
                0.0,
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
                FrameContext::default(),
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                Instant::now(),
                None,
                0,
                0.0,
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

// =========================================================================
// 测试 3: Insert/Delete + click_at/move_cursor_* 的 epoch 变化
// =========================================================================

/// `insert_text` 不应 bump cursor_owner_epoch — 正文事务应继续拥有 coordinated caret。
#[test]
fn insert_text_does_not_bump_epoch() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        item.insert_text(QString::from("Hello"));
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;
        assert_eq!(
            epoch_after, epoch_before,
            "insert_text 不应 bump cursor_owner_epoch（正文事务继续拥有 caret）"
        );
        println!(
            "[BEHAVIOR_VERIFY] insert_text: epoch unchanged ({})",
            epoch_after
        );
    });
}

/// `delete_backward` 不应 bump cursor_owner_epoch。
#[test]
fn delete_backward_does_not_bump_epoch() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.insert_text(QString::from("Hello"));
        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        item.delete_backward();
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;
        assert_eq!(
            epoch_after, epoch_before,
            "delete_backward 不应 bump cursor_owner_epoch"
        );
        println!(
            "[BEHAVIOR_VERIFY] delete_backward: epoch unchanged ({})",
            epoch_after
        );
    });
}

/// `move_cursor_horizontal` 改变 cursor 时必须 bump epoch。
#[test]
fn move_cursor_horizontal_bumps_epoch_on_change() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.insert_text(QString::from("Hello"));
        // insert_text 后 cursor 在文末
        let cursor_before = item.buffer.cursor;
        assert!(cursor_before > 0, "insert_text 后 cursor 应在文末");

        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        item.move_cursor_horizontal(false, false); // backward
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;
        let cursor_after = item.buffer.cursor;

        assert!(
            cursor_after < cursor_before,
            "move_cursor_backward 应改变 cursor: {} -> {}",
            cursor_before,
            cursor_after
        );
        assert!(
            epoch_after > epoch_before,
            "move_cursor_horizontal 改变 cursor 时必须 bump epoch: {} -> {}",
            epoch_before,
            epoch_after
        );
        println!(
            "[BEHAVIOR_VERIFY] move_cursor_horizontal: epoch bumped {} -> {} (cursor {} -> {})",
            epoch_before, epoch_after, cursor_before, cursor_after
        );
    });
}

/// `move_cursor_horizontal` no-op（已在边界）时不 bump epoch。
#[test]
fn move_cursor_horizontal_noop_does_not_bump_epoch() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.insert_text(QString::from("Hello"));
        // cursor 在文末，继续 forward 是 no-op
        let cursor_before = item.buffer.cursor;

        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        item.move_cursor_horizontal(true, false); // forward — no-op at end
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;
        let cursor_after = item.buffer.cursor;

        assert_eq!(
            cursor_after, cursor_before,
            "文末 forward 应是 no-op，cursor 不变"
        );
        assert_eq!(
            epoch_after, epoch_before,
            "no-op move 不应 bump epoch: {} -> {}",
            epoch_before, epoch_after
        );
        println!(
            "[BEHAVIOR_VERIFY] move_cursor_horizontal no-op: epoch unchanged ({})",
            epoch_after
        );
    });
}

/// `move_cursor_horizontal` 在行首 backward 是 no-op，不 bump epoch。
#[test]
fn move_cursor_backward_at_start_does_not_bump_epoch() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.insert_text(QString::from("Hello"));
        // 移到行首
        for _ in 0..10 {
            item.move_cursor_horizontal(false, false);
        }
        assert_eq!(item.buffer.cursor, 0, "应移到行首");

        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        item.move_cursor_horizontal(false, false); // backward — no-op at start
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;

        assert_eq!(item.buffer.cursor, 0, "行首 backward 应是 no-op");
        assert_eq!(
            epoch_after, epoch_before,
            "行首 no-op backward 不应 bump epoch"
        );
        println!(
            "[BEHAVIOR_VERIFY] move_cursor_backward at start: epoch unchanged ({})",
            epoch_after
        );
    });
}

/// `click_at` 到不同位置时必须 bump epoch。
#[test]
fn click_at_different_position_bumps_epoch() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.insert_text(QString::from("Hello"));
        // cursor 在文末（5）
        let cursor_before = item.buffer.cursor;
        assert!(cursor_before > 0, "insert_text 后 cursor 应在文末");

        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        // 点击文档起点 (0, 0) — 应改变 cursor
        item.click_at(0.0, 0.0, false);
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;
        let cursor_after = item.buffer.cursor;

        assert!(
            cursor_after < cursor_before,
            "click_at(0,0) 应改变 cursor: {} -> {}",
            cursor_before,
            cursor_after
        );
        assert!(
            epoch_after > epoch_before,
            "click_at 不同位置必须 bump epoch: {} -> {}",
            epoch_before,
            epoch_after
        );
        println!(
            "[BEHAVIOR_VERIFY] click_at different: epoch bumped {} -> {} (cursor {} -> {})",
            epoch_before, epoch_after, cursor_before, cursor_after
        );
    });
}

/// `click_at` 到当前位置（同一位置）时不 bump epoch。
#[test]
fn click_at_same_position_does_not_bump_epoch() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.insert_text(QString::from("Hello"));
        // 先点击文档起点
        item.click_at(0.0, 0.0, false);
        assert_eq!(item.buffer.cursor, 0, "click_at(0,0) 后 cursor 应在 0");

        // 再点击同一位置 — 不应 bump epoch
        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        item.click_at(0.0, 0.0, false);
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;

        assert_eq!(item.buffer.cursor, 0, "再次 click_at(0,0) cursor 仍为 0");
        assert_eq!(
            epoch_after, epoch_before,
            "click_at 同一位置不应 bump epoch: {} -> {}",
            epoch_before, epoch_after
        );
        println!(
            "[BEHAVIOR_VERIFY] click_at same position: epoch unchanged ({})",
            epoch_after
        );
    });
}

/// `delete_forward` 不应 bump cursor_owner_epoch。
#[test]
fn delete_forward_does_not_bump_epoch() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.insert_text(QString::from("Hello"));
        // 移到行首，使 delete_forward 有字符可删
        for _ in 0..10 {
            item.move_cursor_horizontal(false, false);
        }
        assert_eq!(item.buffer.cursor, 0, "应移到行首");

        let epoch_before = item.cursor_ctrl.cursor_owner_epoch;
        item.delete_forward();
        let epoch_after = item.cursor_ctrl.cursor_owner_epoch;

        assert_eq!(
            epoch_after, epoch_before,
            "delete_forward 不应 bump cursor_owner_epoch"
        );
        println!(
            "[BEHAVIOR_VERIFY] delete_forward: epoch unchanged ({})",
            epoch_after
        );
    });
}

/// 连续 insert + move + insert: insert 不 bump, move bump, insert 不 bump。
/// 验证 epoch 只在非正文事务的 cursor 移动时增加。
#[test]
fn insert_move_insert_epoch_sequence() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();

        // insert "Hello" — epoch 不变
        let e0 = item.cursor_ctrl.cursor_owner_epoch;
        item.insert_text(QString::from("Hello"));
        let e1 = item.cursor_ctrl.cursor_owner_epoch;
        assert_eq!(e1, e0, "第一次 insert 不 bump epoch");

        // move backward — epoch bump
        item.move_cursor_horizontal(false, false);
        let e2 = item.cursor_ctrl.cursor_owner_epoch;
        assert!(e2 > e1, "move backward 应 bump epoch: {} -> {}", e1, e2);

        // insert "X" — epoch 不变（相对于 e2）
        item.insert_text(QString::from("X"));
        let e3 = item.cursor_ctrl.cursor_owner_epoch;
        assert_eq!(e3, e2, "第二次 insert 不 bump epoch: {} -> {}", e2, e3);

        // move backward — epoch bump
        item.move_cursor_horizontal(false, false);
        let e4 = item.cursor_ctrl.cursor_owner_epoch;
        assert!(
            e4 > e3,
            "第二次 move backward 应 bump epoch: {} -> {}",
            e3,
            e4
        );

        println!(
            "[BEHAVIOR_VERIFY] insert-move-insert epoch sequence: {} -> {} -> {} -> {} -> {}",
            e0, e1, e2, e3, e4
        );
    });
}

// =========================================================================
// 测试 4: 完整帧/事务/光标所有权交接生命周期
// =========================================================================

/// 完整生命周期：正文 revision → 旧 prepared_frame 失效 → 当前 render snapshot
/// 算目标 → 实际 RenderPlan 产出 drawn_caret_rect → 下一笔从 drawn rect rebase
/// → 手动移动使旧正文事务失去 caret ownership → no-op 不失去 ownership。
///
/// Issue #707 评论 5724685300: 这条测试覆盖 comment 要求的完整交接链，
/// 不只单独测试每个环节，而是把它们串起来验证一致性。
/// Issue #707 评论 5725190370: 重写本测试填补最后一个缺口 ——
/// 1. 用 `build_cursor_render_state_for_frame` 构造真实 `CursorRenderState`
///    （和 `update_paint_node` 完全一致），不再用 `CursorRenderState::default()`
///    冒充光标；
/// 2. `mark_prepared` 把 `insert_text` 创建的事务推进到 `Prepared`，
///    `build_render_plan_full` 内部再推进到 `Rendering`，
///    `compute_coordinated_cursor_position` 返回 `Some` 产生 `Coordinated`；
/// 3. 调正式回写方法 `apply_render_plan_cursor_state` 并断言
///    `cursor_ctrl.visual_* == drawn_caret_rect`；
/// 4. `move_cursor_horizontal` bump epoch 但不取消正文事务，
///    同一活动事务 `build_render_plan_full` 不再产生 `Coordinated`
///    （`cursor_owner_epoch != current_cursor_epoch` 不匹配）；
/// 5. no-op（行首 backward / 同位置 click）不 bump epoch。
/// Issue #707 评论 5725462471: 补上最后两个行为断言 ——
/// - 阶段 3.5: 正文事务仍拥有 caret 时，no-op（文末 forward）不 bump epoch、
///   不切断 ownership，下一帧 build 仍 `Coordinated`。之前阶段 5 的 no-op
///   发生在 epoch 已 bump、旧事务已失去 ownership 之后，只能证明"no-op 不
///   继续 bump"，不能证明"no-op 不切断"。
/// - 阶段 4: 真实 move 后 `cursor_ctrl.animation` 的 `start_x/start_y` 必须等于
///   上一帧 `drawn_caret_rect` 回写的 `visual_x/visual_y`，证明 drawn caret
///   回写真的被下一笔动画消费，且 target 已变化（非 no-op）。
#[test]
fn full_lifecycle_frame_invalidation_render_plan_epoch_handoff() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello"));
        // Issue #707 评论 5725462471: 设足够大的 viewport_height，使 cursor 在
        // viewport 内（in_viewport=true），这样 move 后 build_cursor_plan 走 Tween
        // 而非 Snap（!should_be_visible），animation 才会被创建，才能断言
        // start_x/start_y == visual_after_write。默认 current_viewport_height=0
        // 会导致 should_be_visible=false → Snap → animation=None。
        item.current_viewport_height = 600.0;
        // Issue #707 评论 5725462471: 先移到文末，使 insert_text 在文末插入，
        // cursor 落在新文末边界。这样阶段 3.5 可以用 forward no-op 验证
        // "正文事务拥有 caret 时 no-op 不切断 ownership"（forward no-op 直接
        // return，不调 update_cursor_visual_position，visual 不变，最稳妥）。
        // 此时还没有正文事务，bump epoch 无副作用。
        item.move_to_line_edge(true, false);

        // ── 阶段 1: 初始 prepared_frame ──
        assert!(
            item.prepared_frame.is_some(),
            "set_plain_text 后 prepared_frame 应为 Some"
        );
        let initial_rev = item.pipeline.text_revision();

        // ── 阶段 2: insert_text 创建活动正文事务（不 bump epoch）──
        let epoch_before_insert = item.cursor_ctrl.cursor_owner_epoch;
        item.insert_text(QString::from("World"));
        assert_eq!(
            item.cursor_ctrl.cursor_owner_epoch, epoch_before_insert,
            "insert_text 不应 bump epoch（正文事务拥有 caret）"
        );

        // 断言确实存在活动正文事务
        let tx_key = item
            .pipeline
            .animation_coordinator_mut()
            .active_text_transaction_key()
            .expect("insert_text 后应有活动正文事务");
        assert!(
            item.pipeline
                .animation_coordinator_mut()
                .has_active_insert(),
            "insert_text 后应有 active insert 事务"
        );

        // emit_content_changed 使旧 prepared_frame 失效
        item.emit_content_changed();
        let new_rev = item.pipeline.text_revision();
        assert!(
            new_rev > initial_rev,
            "emit_content_changed 后 text_revision 必须增加: {} -> {}",
            initial_rev,
            new_rev
        );
        assert!(
            item.prepared_frame.is_some(),
            "emit_content_changed 后 prepared_frame 应为 Some"
        );

        // ── 阶段 3: mark_prepared + build_render_plan_full 得到 Coordinated ──
        assert!(
            item.pipeline
                .animation_coordinator_mut()
                .prepared_queue
                .mark_prepared(tx_key),
            "mark_prepared 应成功推进事务到 Prepared"
        );

        // 用真实 CursorRenderState（和 update_paint_node 一样），不用 default()
        let cursor_render_state = item.build_cursor_render_state_for_frame();
        let selection_preedit = item
            .prepared_frame
            .as_ref()
            .map(|f| f.selection_preedit.clone())
            .unwrap_or_default();
        let frame_now = Instant::now();
        let frame_basis_rev = item.pipeline.layout_revision();

        let plan = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                cursor_render_state,
                selection_preedit,
                FrameContext {
                    active_transaction_keys: Vec::new(),
                    keys_to_complete: Vec::new(),
                    keys_to_cancel: Vec::new(),
                    layout_basis_revision: frame_basis_rev,
                },
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                frame_now,
                item.cursor_ctrl.animation.as_ref(),
                item.cursor_ctrl.cursor_owner_epoch,
                0.0,
            );

        // 断言得到 Coordinated（证明正文事务此时确实拥有 caret）
        let drawn = plan
            .drawn_caret_rect
            .expect("build_render_plan_full 必须产出 drawn_caret_rect");
        match plan.cursor_sample_outcome {
            super::render_plan::CursorSampleOutcome::Coordinated { x, y, h } => {
                assert!(
                    x.is_finite() && y.is_finite() && h.is_finite(),
                    "Coordinated 位置必须有限: ({}, {}, {})",
                    x,
                    y,
                    h
                );
                println!(
                    "[BEHAVIOR_VERIFY] 阶段3 Coordinated: ({:.4}, {:.4}, {:.4})",
                    x, y, h
                );
            }
            other => panic!(
                "epoch 未变时活动正文事务应产生 Coordinated，实际: {:?}",
                other
            ),
        }

        // 调正式回写方法，断言 cursor_ctrl.visual_* == drawn_caret_rect
        let (dx, dy, dh) = drawn;
        item.apply_render_plan_cursor_state(&plan, frame_now, 0.0);
        assert!(
            (item.cursor_ctrl.visual_x - dx).abs() < 0.5,
            "回写后 visual_x 应等于 drawn_caret_rect.x: 实际 {} vs {}",
            item.cursor_ctrl.visual_x,
            dx
        );
        assert!(
            (item.cursor_ctrl.visual_y - dy).abs() < 0.5,
            "回写后 visual_y 应等于 drawn_caret_rect.y: 实际 {} vs {}",
            item.cursor_ctrl.visual_y,
            dy
        );
        if dh > 0.0 {
            assert!(
                (item.cursor_ctrl.visual_h - dh).abs() < 0.5,
                "回写后 visual_h 应等于 drawn_caret_rect.h: 实际 {} vs {}",
                item.cursor_ctrl.visual_h,
                dh
            );
        }
        let visual_after_write = (item.cursor_ctrl.visual_x, item.cursor_ctrl.visual_y);

        // ── 阶段 3.5: no-op 不切断正文事务 caret 所有权 ──
        // Issue #707 评论 5725462471: 在阶段4 真正 move 之前验证 ——
        // 正文事务还拥有 caret 时，no-op 不应把 ownership 切断，下一帧仍应 Coordinated。
        // 之前阶段5 的 no-op 测试发生在 epoch 已经 bump、旧事务已失去 ownership 之后，
        // 只能证明"no-op 不继续 bump epoch"，不能证明"no-op 不切断 ownership"。
        // Issue #707 评论 5725765860: plan_noop 这一帧必须按正式路径回写（apply），
        // 否则后续 move 仍拿第一次 plan 的 visual 当起点，与生产 update_paint_node
        // 路径（build 后必 apply）不一致。visual_after_noop_frame 在块外声明，供阶段4 使用。
        let visual_after_noop_frame;
        {
            let epoch_before_noop = item.cursor_ctrl.cursor_owner_epoch;
            let cursor_before_noop = item.buffer.cursor;
            // cursor 当前在文末（"HelloWorld" 位置 10），forward 是确定的 no-op。
            // move_cursor_horizontal 内部先算 next，确认 next == cursor 后直接 return，
            // 不 bump epoch、不调 update_cursor_visual_position，visual 不变。
            assert_eq!(
                item.buffer.cursor,
                item.buffer.text.len(),
                "no-op 前 cursor 应在文末（setup 已移到文末再 insert）"
            );
            item.move_cursor_horizontal(true, false); // forward no-op at end of text
            assert_eq!(
                item.buffer.cursor, cursor_before_noop,
                "文末 forward 应是 no-op，cursor 不变"
            );
            assert_eq!(
                item.cursor_ctrl.cursor_owner_epoch, epoch_before_noop,
                "no-op 不应 bump epoch（正文事务仍拥有 caret）"
            );
            // 确认同一个正文事务仍 active
            assert!(
                item.pipeline
                    .animation_coordinator_mut()
                    .active_text_transaction_key()
                    .is_some(),
                "no-op 后正文事务应仍 active"
            );
            // 再用同样的 build_render_plan_full build 一帧，断言仍然是 Coordinated
            let cursor_render_state_noop = item.build_cursor_render_state_for_frame();
            let selection_preedit_noop = item
                .prepared_frame
                .as_ref()
                .map(|f| f.selection_preedit.clone())
                .unwrap_or_default();
            let frame_now_noop = Instant::now();
            let frame_basis_rev_noop = item.pipeline.layout_revision();
            let plan_noop = item
                .pipeline
                .animation_coordinator_mut()
                .build_render_plan_full(
                    cursor_render_state_noop,
                    selection_preedit_noop,
                    FrameContext {
                        active_transaction_keys: Vec::new(),
                        keys_to_complete: Vec::new(),
                        keys_to_cancel: Vec::new(),
                        layout_basis_revision: frame_basis_rev_noop,
                    },
                    CursorStyle::default(),
                    SelectionPreeditStyle::default(),
                    frame_now_noop,
                    item.cursor_ctrl.animation.as_ref(),
                    item.cursor_ctrl.cursor_owner_epoch,
                    0.0,
                );
            match plan_noop.cursor_sample_outcome {
                super::render_plan::CursorSampleOutcome::Coordinated { x, y, h } => {
                    assert!(
                        x.is_finite() && y.is_finite() && h.is_finite(),
                        "no-op 后正文事务仍应产生 Coordinated: ({}, {}, {})",
                        x,
                        y,
                        h
                    );
                    println!(
                        "[BEHAVIOR_VERIFY] 阶段3.5 no-op 后仍 Coordinated: ({:.4}, {:.4}, {:.4})",
                        x, y, h
                    );
                }
                other => panic!(
                    "no-op 不应切断正文事务 caret 所有权，下一帧仍应 Coordinated，实际: {:?}",
                    other
                ),
            }
            // Issue #707 评论 5725765860: 把 plan_noop 这一帧按正式路径回写。
            // 生产 update_paint_node() 每次 build_render_plan_full() 后都会
            // apply_render_plan_cursor_state，测试必须同样回写，后续 move 才会
            // 拿"上一帧真正画出来的位置"当 Tween 起点，而非第一次 plan 留下的 visual。
            let drawn_noop = plan_noop
                .drawn_caret_rect
                .expect("plan_noop 必须产出 drawn_caret_rect");
            item.apply_render_plan_cursor_state(&plan_noop, frame_now_noop, 0.0);
            visual_after_noop_frame = (item.cursor_ctrl.visual_x, item.cursor_ctrl.visual_y);
            let (dnx, dny, _) = drawn_noop;
            assert!(
                (visual_after_noop_frame.0 - dnx).abs() < 0.5,
                "plan_noop 回写后 visual_x 应等于 drawn_caret_rect.x: 实际 {} vs {}",
                visual_after_noop_frame.0,
                dnx
            );
            assert!(
                (visual_after_noop_frame.1 - dny).abs() < 0.5,
                "plan_noop 回写后 visual_y 应等于 drawn_caret_rect.y: 实际 {} vs {}",
                visual_after_noop_frame.1,
                dny
            );
            println!(
                "[BEHAVIOR_VERIFY] 阶段3.5 plan_noop 帧回写后 visual=({:.4}, {:.4}) == drawn_caret_rect",
                visual_after_noop_frame.0,
                visual_after_noop_frame.1
            );
        }

        // ── 阶段 4: 真实手动移动 bump epoch，旧事务失去 caret 所有权 ──
        let epoch_before_move = item.cursor_ctrl.cursor_owner_epoch;
        item.move_cursor_horizontal(false, false); // backward
                                                   // Issue #707 评论 5725462471/5725765860: 证明下一笔光标动画从上一帧
                                                   // drawn_caret_rect 起步。上一帧 = plan_noop 帧，其 drawn_caret_rect 已按正式
                                                   // 路径回写为 visual_after_noop_frame。生产 CursorController::apply_plan 的关键
                                                   // 保证：有可信 visual position 时，新 Tween 的 start_x/start_y 必须取当前
                                                   // visual_x/visual_y，不能退回 old_rect。move 后 cursor_ctrl.animation 必须是
                                                   // Some（Tween），且 start == visual_after_noop_frame。
        let anim_after_move = item
            .cursor_ctrl
            .animation
            .as_ref()
            .expect("真实 move 后应创建 Tween 动画（visual != target，smooth cursor 开启）");
        // Issue #707 评论 5725765860: Tween 起点必须取 plan_noop 帧回写的 visual，
        // 即上一帧真正画出来的位置（plan_noop.drawn_caret_rect），而非第一次 plan 的 visual。
        assert!(
            (anim_after_move.start_x - visual_after_noop_frame.0).abs() < 0.5,
            "新 Tween start_x 必须从 plan_noop 帧回写的 visual 起步: \
             实际 start_x={:.4} vs visual_after_noop_frame.x={:.4}",
            anim_after_move.start_x,
            visual_after_noop_frame.0
        );
        assert!(
            (anim_after_move.start_y - visual_after_noop_frame.1).abs() < 0.5,
            "新 Tween start_y 必须从 plan_noop 帧回写的 visual 起步: \
             实际 start_y={:.4} vs visual_after_noop_frame.y={:.4}",
            anim_after_move.start_y,
            visual_after_noop_frame.1
        );
        // 断言 target 已经变化，避免 no-op / 同位置误通过
        assert!(
            (anim_after_move.target_x - anim_after_move.start_x).abs() > 0.01
                || (anim_after_move.target_y - anim_after_move.start_y).abs() > 0.01,
            "Tween target 必须与 start 不同（真实移动）：\
             target=({:.4}, {:.4}), start=({:.4}, {:.4})",
            anim_after_move.target_x,
            anim_after_move.target_y,
            anim_after_move.start_x,
            anim_after_move.start_y
        );
        println!(
            "[BEHAVIOR_VERIFY] 阶段4: move 后 Tween start=({:.4}, {:.4}) == visual_after_noop_frame, target=({:.4}, {:.4})",
            anim_after_move.start_x,
            anim_after_move.start_y,
            anim_after_move.target_x,
            anim_after_move.target_y
        );
        let epoch_after_move = item.cursor_ctrl.cursor_owner_epoch;
        assert!(
            epoch_after_move > epoch_before_move,
            "move_cursor_backward 必须 bump epoch: {} -> {}",
            epoch_before_move,
            epoch_after_move
        );

        // 同一个正文事务应仍在队列中（move 不取消事务），但因 epoch 不一致
        // 已被 `find_cursor_transaction_for_target` 触发收口（caret_motion_retired = true）。
        // Issue #735 评论 5773604666 问题3: 新行为——epoch 不一致时 CaretDriven units
        // 立即落到终态，事务 retired，active_text_transaction_key() 不再返回它。
        let tx_still_in_queue = item
            .pipeline
            .animation_coordinator_mut()
            .prepared_queue
            .active_transactions()
            .iter()
            .any(|t| t.key == tx_key);
        assert!(
            tx_still_in_queue,
            "move 后正文事务应仍在队列（move 不取消事务，只是 retired）"
        );
        // 验证新行为：事务已被 retired
        let tx_ref = item
            .pipeline
            .animation_coordinator_mut()
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == tx_key)
            .expect("事务应仍在队列中");
        assert!(
            tx_ref.caret_motion_retired,
            "Issue #735 评论 5773604666 问题3: epoch 不一致后事务应被 retired\
             （CaretDriven units 已落到终态）"
        );
        // 验证新行为：CaretDriven units 的 start_fraction 已设为 target_fraction（终态）
        for unit in &tx_ref.units {
            use super::animated_slice::AnimatedSliceKind;
            use super::text_visual_transaction::VisualUnitTiming;
            if matches!(
                unit.slice.kind,
                AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
            ) {
                if let VisualUnitTiming::CaretDriven {
                    start_fraction,
                    target_fraction,
                } = &unit.timing
                {
                    assert!(
                        (start_fraction - target_fraction).abs() < 1e-9,
                        "Issue #735 评论 5773604666 问题3: CaretDriven unit 的 \
                         start_fraction 应已设为 target_fraction（终态）: \
                         start={:.4}, target={:.4}",
                        start_fraction,
                        target_fraction
                    );
                }
            }
        }
        println!(
            "[BEHAVIOR_VERIFY] 阶段4: 事务 {:?} 已 retired，CaretDriven units 已落到终态",
            tx_key
        );
        // 验证 has_active_timed_units 语义：收口后事务是否还有活跃 Timed unit
        // （ReflowMove/ReflowCrossFade）。Insert 事务通常没有 Timed unit，应返回 false。
        let has_timed = tx_ref.has_active_timed_units(Instant::now());
        println!(
            "[BEHAVIOR_VERIFY] 阶段4: 事务 {:?} has_active_timed_units={}",
            tx_key, has_timed
        );
        // active_text_transaction_key() 不再返回 retired 事务
        let active_key_after_retire = item
            .pipeline
            .animation_coordinator_mut()
            .active_text_transaction_key();
        assert!(
            active_key_after_retire.is_none() || active_key_after_retire != Some(tx_key),
            "retired 事务不应被 active_text_transaction_key() 返回"
        );

        let cursor_render_state_2 = item.build_cursor_render_state_for_frame();
        let selection_preedit_2 = item
            .prepared_frame
            .as_ref()
            .map(|f| f.selection_preedit.clone())
            .unwrap_or_default();
        let frame_now_2 = Instant::now();
        let frame_basis_rev_2 = item.pipeline.layout_revision();
        let plan2 = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                cursor_render_state_2,
                selection_preedit_2,
                FrameContext {
                    active_transaction_keys: Vec::new(),
                    keys_to_complete: Vec::new(),
                    keys_to_cancel: Vec::new(),
                    layout_basis_revision: frame_basis_rev_2,
                },
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                frame_now_2,
                item.cursor_ctrl.animation.as_ref(),
                item.cursor_ctrl.cursor_owner_epoch,
                0.0,
            );

        // 断言这次不能再得到由旧事务产生的 Coordinated
        match plan2.cursor_sample_outcome {
            super::render_plan::CursorSampleOutcome::Coordinated { .. } => {
                panic!("epoch bump 后旧正文事务不应再产生 Coordinated（旧事务失去 caret 所有权）");
            }
            other => {
                println!(
                    "[BEHAVIOR_VERIFY] 阶段4: epoch bump 后旧事务不再 Coordinated (outcome={:?})",
                    other
                );
            }
        }
        let drawn2 = plan2.drawn_caret_rect.expect("plan2 drawn_caret_rect");
        item.apply_render_plan_cursor_state(&plan2, frame_now_2, 0.0);
        println!(
            "[BEHAVIOR_VERIFY] 阶段4: visual after move=({}, {}), drawn2=({:?})",
            item.cursor_ctrl.visual_x, item.cursor_ctrl.visual_y, drawn2
        );

        // ── 阶段 5: no-op 移动不 bump epoch ──
        for _ in 0..20 {
            item.move_cursor_horizontal(false, false);
        }
        assert_eq!(item.buffer.cursor, 0, "应移到行首");
        let epoch_at_start = item.cursor_ctrl.cursor_owner_epoch;
        item.move_cursor_horizontal(false, false);
        assert_eq!(
            item.cursor_ctrl.cursor_owner_epoch, epoch_at_start,
            "行首 backward no-op 不应 bump epoch"
        );

        item.click_at(0.0, 0.0, false);
        assert_eq!(item.buffer.cursor, 0, "click_at(0,0) 后 cursor 应在 0");
        let epoch_before_same_click = item.cursor_ctrl.cursor_owner_epoch;
        item.click_at(0.0, 0.0, false);
        assert_eq!(
            item.cursor_ctrl.cursor_owner_epoch, epoch_before_same_click,
            "click_at 同一位置不应 bump epoch"
        );

        println!(
            "[BEHAVIOR_VERIFY] full lifecycle: rev {} -> {}, epoch {} -> {} (move) -> {} (no-op), visual_after_write=({:.1},{:.1})",
            initial_rev,
            new_rev,
            epoch_before_move,
            epoch_after_move,
            item.cursor_ctrl.cursor_owner_epoch,
            visual_after_write.0,
            visual_after_write.1
        );
    });
}

// =========================================================================
// 测试: Issue #724 评论 5752572618 — auto-follow anchor 时序
// =========================================================================
// Issue #727 评论 5755858583 问题1: CaretViewportAnchor 已删除，
// auto-follow anchor 相关测试一并删除。cursor layer 现在和正文层统一用
// QSGTransformNode 做 scroll_y 变换，不再需要 viewport anchor 机制。
