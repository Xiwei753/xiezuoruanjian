//! Issue #707 评论 5724685300 — sujian_editor_item 内部状态测试。
//!
//! 本模块在 `sujian_editor_item` 模块内部（`#[cfg(test)] mod runtime_tests;`），
//! 可以直接访问 `pub(crate)` 字段和方法，不需要把生产内部 API 全暴露出去。
//!
//! 测试真实状态交接:
//! - `emit_content_changed()` 后旧 prepared_frame 失效
//! - `build_render_plan_full()` 产出 drawn_caret_rect
//!
//! Issue #826: 原先测 `cursor_owner_epoch` 变化的那批测试一并删除。
//! 光标所有权 epoch 已随遮罩前沿重写一起删掉——视觉光标 Tween 与文字动画
//! 完全解耦，不再需要"哪笔正文事务拥有这条 caret"的判定。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::animation::EditFrontierKind;
use super::render_plan::{
    CursorRenderState, CursorStyle, PreparedEditorFrame, SelectionPreeditPlan,
    SelectionPreeditStyle,
};
use super::*;
use crate::editor::layout::{run_on_qt_thread, LayoutParams};
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
                frame_now,
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
                Instant::now(),
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
// 测试 4: Issue #826 遮罩前沿生命周期
// =========================================================================

/// Issue #826: 正文编辑建出唯一遮罩前沿；`build_render_plan_full` 把当前光标
/// 位置透传成 `drawn_caret_rect`；`apply_render_plan_cursor_state` 把它回写到
/// `cursor_ctrl.visual_*`；指针点击把前沿立刻收成 canonical 终态。
#[test]
fn edit_frontier_lifecycle_with_cursor_roundtrip() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello"));
        // 设足够大的 viewport_height，使光标在视口内 → 光标移动走 Tween。
        item.current_viewport_height = 600.0;
        // 前沿只在文字动画开关打开时建立。
        item.current_typing_animation_enabled = true;

        assert!(
            !item
                .pipeline
                .animation_coordinator()
                .has_active_edit_frontier(),
            "初始没有活跃前沿"
        );

        // 一次真实输入 → 建出前沿。
        item.move_cursor_horizontal(false, false);
        item.insert_text(QString::from("A"));
        let frontier_kind = item
            .pipeline
            .animation_coordinator()
            .active_edit_frontier_kind();
        assert!(
            matches!(frontier_kind, Some(EditFrontierKind::Insert)),
            "输入后前沿种类应为 Insert，实际: {:?}",
            frontier_kind
        );

        // build_render_plan_full 用真实 cursor_render_state 产出 drawn_caret_rect。
        let cursor_render_state = CursorRenderState {
            visible: true,
            x: 37.5,
            y: 11.25,
            h: 21.0,
            opacity: 1.0,
        };
        let frame_now = Instant::now();
        let plan = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                cursor_render_state.clone(),
                SelectionPreeditPlan::default(),
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                frame_now,
            );
        let (x, y, h) = plan.drawn_caret_rect.expect("drawn_caret_rect 已设");
        assert!((x - 37.5).abs() < 0.01, "x 应透传 cursor_render_state.x");
        assert!((y - 11.25).abs() < 0.01, "y 应透传 cursor_render_state.y");
        assert!((h - 21.0).abs() < 0.01, "h 应透传 cursor_render_state.h");

        // 正式回写方法把 drawn rect 同步进 cursor_ctrl。
        item.apply_render_plan_cursor_state(&plan, frame_now, 0.0);
        assert!(
            (item.cursor_ctrl.visual_x - 37.5).abs() < 0.01,
            "visual_x 应等于 drawn_caret_rect.x，实际: {}",
            item.cursor_ctrl.visual_x
        );
        assert!(
            (item.cursor_ctrl.visual_y - 11.25).abs() < 0.01,
            "visual_y 应等于 drawn_caret_rect.y，实际: {}",
            item.cursor_ctrl.visual_y
        );

        // 指针点击把前沿收成 canonical 终态。
        item.click_at(0.0, 0.0, false);
        assert!(
            !item
                .pipeline
                .animation_coordinator()
                .has_active_edit_frontier(),
            "指针点击后前沿应立刻收成 canonical 终态"
        );

        println!(
            "[BEHAVIOR_VERIFY] edit frontier lifecycle: insert -> {:?} -> drawn_caret=({:.2},{:.2},{:.2}) -> click_at 收口",
            frontier_kind, x, y, h
        );
    });
}

/// Issue #826: 连续输入只更新同一个前沿 —— 不生成第二个历史动画对象。
///
/// 语义：连续 Insert 的 `extend_insert` 把当前采样当新起点，只更新最新 target；
/// `active_edit_frontier` 始终只有一个。
#[test]
fn consecutive_typing_keeps_single_edit_frontier() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_typing_animation_enabled = true;
        item.set_plain_text(QString::from("ab"));
        item.move_cursor_horizontal(false, false);

        item.insert_text(QString::from("c"));
        item.insert_text(QString::from("d"));
        item.insert_text(QString::from("e"));

        let coord = item.pipeline.animation_coordinator();
        assert!(
            coord.has_active_edit_frontier(),
            "连续输入后应仍有唯一活跃前沿"
        );
        assert!(
            matches!(
                coord.active_edit_frontier_kind(),
                Some(EditFrontierKind::Insert)
            ),
            "连续输入的前沿种类保持 Insert"
        );
        println!("[BEHAVIOR_VERIFY] consecutive typing: 单一遮罩前沿，无交棒");
    });
}

// =========================================================================
// 测试: Issue #724 评论 5752572618 — auto-follow anchor 时序
// =========================================================================
// Issue #727 评论 5755858583 问题1: CaretViewportAnchor 已删除，
// auto-follow anchor 相关测试一并删除。cursor layer 现在和正文层统一用
// QSGTransformNode 做 scroll_y 变换，不再需要 viewport anchor 机制。

// =========================================================================
// 测试: Issue #745 评论 5805323459 — 正文状态单一平台投影 + Core grapheme 边界
// =========================================================================

/// 单个 grapheme cluster 的家庭 emoji: 👨 ZWJ 👩 ZWJ 👧。
/// 18 byte / 5 个 Unicode scalar —— Qt 端若还按 scalar 自己算编辑边界，
/// 一次删除只会去掉 4 或 3 个 byte，留下残缺的半个簇。
const FAMILY_EMOJI: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";

/// 退格的编辑边界必须来自 Core `EditorKernel::previous_grapheme_boundary`：
/// 一次 `delete_backward` 整簇删除。
#[test]
fn delete_backward_removes_whole_grapheme_cluster_via_kernel_boundary() {
    assert_eq!(FAMILY_EMOJI.len(), 18);
    assert_eq!(FAMILY_EMOJI.chars().count(), 5);

    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("ab"));
        // set_plain_text 把光标放在 0，先移到文末再插入，退格才有目标簇。
        let _ = item.pipeline.set_selection(2, 2);
        item.insert_text(QString::from(FAMILY_EMOJI));
        assert_eq!(
            item.pipeline.committed_text(),
            format!("ab{FAMILY_EMOJI}"),
            "插入后 committed 投影必须包含完整簇"
        );
        assert_eq!(item.pipeline.cursor(), 2 + FAMILY_EMOJI.len());

        item.delete_backward();

        assert_eq!(
            item.pipeline.committed_text(),
            "ab",
            "一次退格必须删掉整个 grapheme cluster，不能残留半个簇"
        );
        assert_eq!(item.pipeline.cursor(), 2, "退格后 cursor 落在簇起点");
        assert!(!item.pipeline.has_selection());
    });
}

/// 前删的编辑边界必须来自 Core `EditorKernel::next_grapheme_boundary`。
#[test]
fn delete_forward_removes_whole_grapheme_cluster_via_kernel_boundary() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from(format!("{FAMILY_EMOJI}cd")));
        // set_plain_text 把 cursor 放在 0，簇正好在光标之后。
        assert_eq!(item.pipeline.cursor(), 0);

        item.delete_forward();

        assert_eq!(
            item.pipeline.committed_text(),
            "cd",
            "一次前删必须删掉整个 grapheme cluster，不能残留半个簇"
        );
        assert_eq!(item.pipeline.cursor(), 0);
    });
}

/// 选区/文本读取全部走 pipeline 只读投影：镜像的 cursor/anchor 与
/// `selection_range`/`selected_text`/`snapshot` 必须自洽，不存在第二份可比对的正文。
#[test]
fn selection_projection_is_self_consistent_with_mirror() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.set_plain_text(QString::from("Hello世界"));
        assert_eq!(item.pipeline.committed_text(), "Hello世界");
        assert!(!item.pipeline.has_selection());
        assert!(item.pipeline.selected_text().is_empty());

        // "ell" = byte [1, 4)
        let _ = item.pipeline.set_selection(1, 4);
        assert!(item.pipeline.has_selection());
        assert_eq!(item.pipeline.selection_range(), (1, 4));
        assert_eq!(item.pipeline.selected_text(), "ell");
        // IME 路径与 QML 属性路径读的是同一个投影，不再各自持有副本。
        assert_eq!(item.ime_query_selected_text(), "ell");
        assert_eq!(item.selected_text(), QString::from("ell"));

        // anchor > head 时 range 仍归一化为 [min, max)
        let _ = item.pipeline.set_selection(11, 8);
        assert_eq!(item.pipeline.selection_range(), (8, 11));
        assert_eq!(item.pipeline.selected_text(), "界");

        let snap = item.pipeline.snapshot();
        assert_eq!(snap.text, item.pipeline.committed_text());
        assert_eq!(snap.cursor, item.pipeline.cursor());
        assert_eq!(snap.selection_anchor, item.pipeline.selection_anchor());
    });
}
