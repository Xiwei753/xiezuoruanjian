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

        let plan = item.pipeline.animation_coordinator_mut().build_render_plan_full(
            cursor_render_state,
            selection_preedit,
            frame_context,
            cursor_style,
            selection_preedit_style,
            frame_now,
            false, // coordinated_enabled
            None,  // cursor_animation
            0,     // cursor_owner_epoch
        );

        assert!(
            plan.drawn_caret_rect.is_some(),
            "build_render_plan_full 必须产出 drawn_caret_rect (Some)"
        );
        let (x, y, h) = plan.drawn_caret_rect.expect("drawn_caret_rect 已设");
        assert!(
            x.is_finite(),
            "drawn_caret_rect.x 必须有限，实际: {}",
            x
        );
        assert!(
            y.is_finite(),
            "drawn_caret_rect.y 必须有限，实际: {}",
            y
        );
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
        let plan = item.pipeline.animation_coordinator_mut().build_render_plan_full(
            cursor_render_state,
            SelectionPreeditPlan::default(),
            FrameContext::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            Instant::now(),
            false,
            None,
            0,
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
        println!("[BEHAVIOR_VERIFY] insert_text: epoch unchanged ({})", epoch_after);
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
        println!("[BEHAVIOR_VERIFY] delete_backward: epoch unchanged ({})", epoch_after);
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
            epoch_before,
            epoch_after
        );
        println!("[BEHAVIOR_VERIFY] move_cursor_horizontal no-op: epoch unchanged ({})", epoch_after);
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

        assert_eq!(
            item.buffer.cursor, 0,
            "行首 backward 应是 no-op"
        );
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

        assert_eq!(
            item.buffer.cursor, 0,
            "再次 click_at(0,0) cursor 仍为 0"
        );
        assert_eq!(
            epoch_after, epoch_before,
            "click_at 同一位置不应 bump epoch: {} -> {}",
            epoch_before,
            epoch_after
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
        println!("[BEHAVIOR_VERIFY] delete_forward: epoch unchanged ({})", epoch_after);
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
        assert!(e4 > e3, "第二次 move backward 应 bump epoch: {} -> {}", e3, e4);

        println!(
            "[BEHAVIOR_VERIFY] insert-move-insert epoch sequence: {} -> {} -> {} -> {} -> {}",
            e0, e1, e2, e3, e4
        );
    });
}
