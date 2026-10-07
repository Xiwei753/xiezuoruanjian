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
use crate::editor::layout::{run_on_qt_thread, CaretAffinity, LayoutParams, LayoutSnapshot};
use crate::sujian_editor_item::animation::edit_frontier::EditFrontierState;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot,
    ShapingIdentity, SourceRect,
};
use qmetaobject::QString;
use std::time::{Duration, Instant};
use writer_core::editor::OffsetMap;

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

// =========================================================================
// Issue #826 评论 34: 滚动 pause / resume 时间轴 + suppressed 正文编辑收口
// =========================================================================

/// Issue #826 评论 34 的共同前置：正文 `AB`，在 `A|B` 之间插入 `X` → `AXB`
/// （X 走 Reveal、B 右移走 Reflow），前沿时长固定 160ms。
///
/// 返回前沿的 `started_at`，让测试能自己控制时间轴（pause 40ms、resume 500ms），
/// 而不是依赖墙钟。
fn build_160ms_reveal_and_reflow(item: &mut SujianEditorItem) -> Instant {
    item.set_plain_text(QString::from("AB"));
    item.current_viewport_height = 600.0;
    item.current_typing_animation_enabled = true;
    item.pipeline.set_typing_animation_duration_ms(160);
    let _ = item.pipeline.set_selection(1, 1);
    item.insert_text(QString::from("X"));

    assert_eq!(
        item.pipeline.committed_text(),
        "AXB",
        "前置：A|B 中间插 X 必须成功"
    );
    let coord = item.pipeline.animation_coordinator();
    assert!(coord.has_active_edit_frontier(), "前置：必须建出遮罩前沿");
    let reflow = coord
        .active_reflow
        .as_ref()
        .expect("前置：B 右移必须建出 Reflow");
    assert!(!reflow.spans.is_empty(), "前置：Reflow spans 不能为空");
    coord
        .active_edit_frontier
        .as_ref()
        .expect("前置：前沿存在")
        .started_at
}

/// Issue #826 评论 34 要求的测试 1。
///
/// 步骤：①建 160ms Reveal/Reflow；②40ms 处 `pause_all`；③500ms 后
/// `resume_all`；④同一恢复帧采样。
/// 断言：progress 仍约等于 pause 那一刻、state 仍 active、不是直接 finished。
/// 覆盖 Frontier + Reflow 两层（shaping 层由
/// `scroll_pause_preserves_shaping_transition_progress_without_edit` 覆盖）。
#[test]
fn scroll_pause_preserves_text_animation_progress_without_edit() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let started_at = build_160ms_reveal_and_reflow(&mut item);

        let t0 = started_at + Duration::from_millis(40);
        let coord = item.pipeline.animation_coordinator_mut();

        let sample0 = coord
            .sample_edit_frontier(t0)
            .expect("pause 前前沿必须可采样");
        assert!(
            sample0.progress > 0.0 && sample0.progress < 1.0,
            "前置：40ms 处前沿必须在途中，实际 {}",
            sample0.progress
        );
        assert!(
            !coord
                .active_edit_frontier
                .as_ref()
                .expect("前置：前沿存在")
                .is_finished(t0),
            "前置：40ms 处前沿尚未走完"
        );
        let reflow_frames0 = coord
            .active_reflow
            .as_ref()
            .expect("前置：Reflow 存在")
            .sample(t0);
        assert!(
            !reflow_frames0.is_empty(),
            "前置：Reflow 必须有正在移动的 span"
        );

        let _freed = coord.pause_all(t0);
        assert!(coord.is_paused(), "pause_all 之后必须处于 paused");

        let t1 = t0 + Duration::from_millis(500);
        coord.resume_all(t1);
        assert!(!coord.is_paused(), "resume_all 之后必须脱离 paused");

        // 同一恢复帧采样：elapsed 必须仍等于 pause 那一刻。
        let sample1 = coord
            .sample_edit_frontier(t1)
            .expect("resume 后前沿必须仍可采样");
        assert!(
            (sample1.progress - sample0.progress).abs() < 1e-12,
            "resume 后同一帧 progress 必须仍等于 pause 那一刻，实际 {} vs {}",
            sample1.progress,
            sample0.progress
        );
        assert!(
            coord.active_edit_frontier.is_some(),
            "resume 后前沿仍 active（不能被判 finished 清掉）"
        );
        assert!(
            !coord
                .active_edit_frontier
                .as_ref()
                .expect("前沿存在")
                .is_finished(t1),
            "resume 后前沿不得直接 finished"
        );

        let reflow = coord
            .active_reflow
            .as_ref()
            .expect("resume 后 Reflow 仍 active");
        let reflow_frames1 = reflow.sample(t1);
        assert_eq!(
            reflow_frames1, reflow_frames0,
            "resume 后同一帧 Reflow 几何必须与 pause 那一刻完全一致"
        );
        assert!(
            !reflow.is_finished(t1),
            "resume 后 Reflow 不得直接判 finished"
        );
        println!(
            "[BEHAVIOR_VERIFY] 评论34: pause/resume 后 progress {:.4} 保持不变",
            sample1.progress
        );
    });
}

/// Issue #826 评论 34 要求的测试 2。
///
/// 步骤：①第一笔建立 active Frontier + Reflow；②`set_is_scrolling(true)`（pause）；
/// ③Core 再应用一笔正文编辑，visual outcome = `ScrollingSuppressed`；
/// ④`emit_content_changed()` 更新最新 canonical；⑤`set_is_scrolling(false)`（resume）。
/// 断言：三层全 None、coordinator 不再 paused、render plan 里没有旧 snapshot_id
/// 的 clip / glyph。
#[test]
fn suppressed_edit_while_scrolling_drops_old_text_animation_before_new_canonical() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let _started_at = build_160ms_reveal_and_reflow(&mut item);

        let old_ids = item
            .pipeline
            .animation_coordinator()
            .collect_active_snapshot_ids();
        assert!(!old_ids.is_empty(), "前置：第一笔编辑必须登记出活动行 id");

        // ② 滚动开始 → 时间轴 pause，状态原样保留。
        item.set_is_scrolling(true);
        assert!(item.is_scrolling(), "进入滚动状态");
        assert!(
            item.pipeline.animation_coordinator().is_paused(),
            "滚动开始必须 pause 正文动画时间轴"
        );

        // ③ Core 编辑照常应用，但视觉侧被 suppress —— 完全不进 prepare_edit_motion。
        let cursor = item.pipeline.cursor();
        let before = item.pipeline.snapshot();
        let core = item
            .pipeline
            .insert_text(cursor, "Y", EditorTransactionCause::Typing);
        let pipeline::PipelineEditOutcome::Applied(result) = core else {
            panic!("前置：Core 编辑必须应用");
        };
        let after = item.pipeline.snapshot();
        let outcome = item.record_transaction(before, after, &result, false);
        assert!(
            matches!(
                outcome,
                pipeline::VisualPrepareOutcome::Skipped(
                    edit_flow::EditVisualSkipReason::ScrollingSuppressed
                )
            ),
            "滚动中的正文编辑 visual outcome 必须是 ScrollingSuppressed"
        );
        assert_eq!(
            item.pipeline.committed_text(),
            "AXYB",
            "Core 编辑必须已应用（正文真的变了）"
        );

        // ④ 模拟 emit_content_changed：canonical 换成最新正文。
        item.emit_content_changed();

        // ⑤ 滚动结束 resume。
        item.set_is_scrolling(false);

        let coord = item.pipeline.animation_coordinator();
        assert!(
            coord.active_edit_frontier.is_none(),
            "suppressed 正文编辑后旧 Frontier 必须已收成 canonical"
        );
        assert!(
            coord.active_reflow.is_none(),
            "suppressed 正文编辑后旧 Reflow 必须已收成 canonical"
        );
        assert!(
            coord.active_shaping_transition.is_none(),
            "suppressed 正文编辑后旧 Shaping 必须已收成 canonical"
        );
        assert!(!coord.is_paused(), "resume 后 coordinator 不再 paused");

        let plan = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                CursorRenderState::default(),
                SelectionPreeditPlan::default(),
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                Instant::now(),
            );
        for glyph in &plan.text_animation.glyphs {
            assert!(
                !old_ids.contains(&glyph.snapshot_id),
                "render plan 不得再画旧 snapshot_id {} 的 glyph",
                glyph.snapshot_id.layout_revision
            );
        }
        for clip in &plan.clip_rects {
            assert!(
                !old_ids.contains(&clip.snapshot_id),
                "render plan 不得再带旧 snapshot_id {} 的 clip",
                clip.snapshot_id.layout_revision
            );
        }
        println!("[BEHAVIOR_VERIFY] 评论34: suppressed 正文编辑后旧动画已让位给新 canonical");
    });
}

/// Issue #826 评论 34 要求的测试 3。
///
/// 前一步（测试 2 的链路）收口后，`collect_active_snapshot_ids()` 不得再含
/// 上一笔旧行 id；TextureCache retain 之后旧动画纹理可以被真正释放。
#[test]
fn suppressed_scroll_edit_releases_old_animation_textures() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let _started_at = build_160ms_reveal_and_reflow(&mut item);

        // 前提非真空：先把上一笔的真实行图准备进缓存；个别行没有行图时补一张
        // 占位图，保证「旧动画纹理」这个前提不依赖排版是否恰好栅格化了那一行。
        item.pipeline.prepare_frontier_textures();
        let old_ids = item
            .pipeline
            .animation_coordinator()
            .collect_active_snapshot_ids();
        assert!(!old_ids.is_empty(), "前置：必须有活动行 id");
        for id in &old_ids {
            if !item.pipeline.texture_cache().contains_line(id) {
                let image = qmetaobject::QImage::new(
                    qmetaobject::QSize {
                        width: 4,
                        height: 4,
                    },
                    qmetaobject::ImageFormat::ARGB32_Premultiplied,
                );
                item.pipeline.texture_cache_mut().insert_line(*id, image);
            }
        }
        assert!(
            old_ids
                .iter()
                .all(|id| item.pipeline.texture_cache().contains_line(id)),
            "前置：旧行纹理必须先进缓存"
        );

        item.set_is_scrolling(true);

        let cursor = item.pipeline.cursor();
        let before = item.pipeline.snapshot();
        let core = item
            .pipeline
            .insert_text(cursor, "Y", EditorTransactionCause::Typing);
        let pipeline::PipelineEditOutcome::Applied(result) = core else {
            panic!("前置：Core 编辑必须应用");
        };
        let after = item.pipeline.snapshot();
        let outcome = item.record_transaction(before, after, &result, false);
        assert!(
            matches!(
                outcome,
                pipeline::VisualPrepareOutcome::Skipped(
                    edit_flow::EditVisualSkipReason::ScrollingSuppressed
                )
            ),
            "滚动中的正文编辑 visual outcome 必须是 ScrollingSuppressed"
        );

        item.emit_content_changed();
        item.set_is_scrolling(false);

        let active_ids = item
            .pipeline
            .animation_coordinator()
            .collect_active_snapshot_ids();
        assert!(
            active_ids.is_empty(),
            "收口后不得再登记任何活动行 id，实际 {:?}",
            active_ids
                .iter()
                .map(|id| id.layout_revision)
                .collect::<Vec<_>>()
        );
        for id in &old_ids {
            assert!(
                !item.pipeline.texture_cache().contains_line(id),
                "旧动画纹理 {} 必须已可释放",
                id.layout_revision
            );
        }
        println!("[BEHAVIOR_VERIFY] 评论34: suppressed 滚动编辑释放了旧动画纹理");
    });
}

/// Issue #826 评论 34 补充测试。
///
/// **未覆盖位置**：评论明确要求的测试 1 只断言 `sample_edit_frontier().progress`
/// 与 Reflow state 的 `sample()`，没有断言**真正画出去的 render plan 输出**。
///
/// 断言：pause 40ms、resume 500ms 后的同一恢复帧，`build_render_plan_full`
/// 产出的 glyph 与 clip 与 pause 那一刻完全一致。
/// 旧实现 resume 只清 `paused_at` → elapsed 直接 540ms → reveal/reflow 都到
/// 终态 → 输出不同 → FAIL。
#[test]
fn scroll_pause_keeps_render_plan_output_continuous() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let started_at = build_160ms_reveal_and_reflow(&mut item);

        let t0 = started_at + Duration::from_millis(40);
        let t1 = t0 + Duration::from_millis(500);
        let plan_before = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                CursorRenderState::default(),
                SelectionPreeditPlan::default(),
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                t0,
            );
        let glyphs_before: Vec<(f64, f64, f64, f64, f64)> = plan_before
            .text_animation
            .glyphs
            .iter()
            .map(|g| (g.x, g.y, g.w, g.h, g.opacity))
            .collect();
        let clips_before: Vec<(f64, f64, f64, f64)> = plan_before
            .clip_rects
            .iter()
            .map(|c| (c.x, c.y, c.w, c.h))
            .collect();
        assert!(
            !glyphs_before.is_empty(),
            "前置：40ms 处 render plan 必须有动画 glyph"
        );

        let coord = item.pipeline.animation_coordinator_mut();
        let _freed = coord.pause_all(t0);
        coord.resume_all(t1);

        let plan_after = item
            .pipeline
            .animation_coordinator_mut()
            .build_render_plan_full(
                CursorRenderState::default(),
                SelectionPreeditPlan::default(),
                CursorStyle::default(),
                SelectionPreeditStyle::default(),
                t1,
            );
        let glyphs_after: Vec<(f64, f64, f64, f64, f64)> = plan_after
            .text_animation
            .glyphs
            .iter()
            .map(|g| (g.x, g.y, g.w, g.h, g.opacity))
            .collect();
        let clips_after: Vec<(f64, f64, f64, f64)> = plan_after
            .clip_rects
            .iter()
            .map(|c| (c.x, c.y, c.w, c.h))
            .collect();

        assert_eq!(
            glyphs_before, glyphs_after,
            "pause/resume 后同一恢复帧的 render plan glyph 必须与 pause 那一刻一致"
        );
        assert_eq!(
            clips_before, clips_after,
            "pause/resume 后同一恢复帧的 render plan clip 必须与 pause 那一刻一致"
        );
        println!("[BEHAVIOR_VERIFY] 评论34: pause/resume 后 render plan 输出连续");
    });
}

// ── 评论 35：paused 期间重绘必须冻结在 pause 时刻 ──────────────────────────

/// Issue #826 评论 35：同一 item 上同时建出 Frontier + Reflow + Shaping 三层。
///
/// `afb` 在光标 2 处插 `i` -> `afib`：`i` 自己走 Reveal、`b` 右移给 Reflow、
/// `f` 的像素身份被 `i` 改变给 Shaping transition（评论 33 已证真实排版下
/// `af -> afi` 会产生 shaping group）。
fn build_160ms_reveal_reflow_and_shaping(item: &mut SujianEditorItem) -> Instant {
    item.set_plain_text(QString::from("afb"));
    item.current_viewport_height = 600.0;
    item.current_typing_animation_enabled = true;
    item.pipeline.set_typing_animation_duration_ms(160);
    let _ = item.pipeline.set_selection(2, 2);
    item.insert_text(QString::from("i"));

    assert_eq!(
        item.pipeline.committed_text(),
        "afib",
        "前置：afb 中间插 i 必须成功"
    );
    let coord = item.pipeline.animation_coordinator();
    assert!(coord.has_active_edit_frontier(), "前置：必须建出遮罩前沿");
    let reflow = coord
        .active_reflow
        .as_ref()
        .expect("前置：b 右移必须建出 Reflow");
    assert!(!reflow.spans.is_empty(), "前置：Reflow spans 不能为空");
    let shaping = coord
        .active_shaping_transition
        .as_ref()
        .expect("前置：f 受 i 影响必须建出 Shaping transition");
    assert!(!shaping.is_empty(), "前置：Shaping groups 不能为空");
    coord
        .active_edit_frontier
        .as_ref()
        .expect("前置：前沿存在")
        .started_at
}

/// render plan 的可比较投影：glyph `(x, y, w, h, opacity)`、clip `(x, y, w, h)`。
fn render_plan_tuples(
    item: &mut SujianEditorItem,
    frame_now: Instant,
) -> (Vec<(f64, f64, f64, f64, f64)>, Vec<(f64, f64, f64, f64)>) {
    let plan = item
        .pipeline
        .animation_coordinator_mut()
        .build_render_plan_full(
            CursorRenderState::default(),
            SelectionPreeditPlan::default(),
            CursorStyle::default(),
            SelectionPreeditStyle::default(),
            frame_now,
        );
    let glyphs = plan
        .text_animation
        .glyphs
        .iter()
        .map(|g| (g.x, g.y, g.w, g.h, g.opacity))
        .collect();
    let clips = plan
        .clip_rects
        .iter()
        .map(|c| (c.x, c.y, c.w, c.h))
        .collect();
    (glyphs, clips)
}

/// shaping old/new 两侧的几何 + opacity 投影（评论 35 至少断言
/// 「Shaping old/new opacity 不变」）。
fn shaping_side_tuples(
    item: &SujianEditorItem,
    frame_now: Instant,
) -> Vec<(f64, f64, f64, f64, f64)> {
    let mut out = Vec::new();
    for frame in item
        .pipeline
        .animation_coordinator()
        .shaping_transition_glyphs(frame_now)
    {
        for side in frame.old.iter().chain(frame.new.iter()) {
            out.push((
                side.rect.x,
                side.rect.y,
                side.rect.w,
                side.rect.h,
                side.opacity,
            ));
        }
    }
    out
}

/// Issue #826 评论 35 要求的测试 1。
///
/// 步骤：①建 160ms Frontier + Reflow + Shaping；②t0=40ms 先 build render plan
/// 记 glyph + clip + opacity；③`pause_all(t0)`；④**不调用 resume**，直接在
/// t0+100ms、t0+500ms 各 build 一次；⑤两次输出都必须与 t0 完全一致。
///
/// 旧实现 `build_render_plan_full(frame_now)` 直接按墙钟采样 → 540ms 已到终态
/// → 输出不同 → FAIL。
#[test]
fn scroll_pause_freezes_render_plan_during_repaints_before_resume() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let started_at = build_160ms_reveal_reflow_and_shaping(&mut item);

        let t0 = started_at + Duration::from_millis(40);
        let mid = t0 + Duration::from_millis(100);
        let late = t0 + Duration::from_millis(500);

        let (glyphs0, clips0) = render_plan_tuples(&mut item, t0);
        let shaping0 = shaping_side_tuples(&item, t0);
        assert!(
            !glyphs0.is_empty(),
            "前置：40ms 处 render plan 必须有动画 glyph"
        );
        assert!(
            glyphs0.iter().any(|(_, _, _, _, opacity)| *opacity < 1.0),
            "前置：40ms 处必须是途中态而不是终态"
        );
        assert!(!shaping0.is_empty(), "前置：40ms 处必须有 shaping 画面");

        {
            let coord = item.pipeline.animation_coordinator_mut();
            let _freed = coord.pause_all(t0);
            assert!(coord.is_paused(), "pause_all 之后必须处于 paused");
        }

        for (label, frame_now) in [("t0+100ms", mid), ("t0+500ms", late)] {
            let (glyphs, clips) = render_plan_tuples(&mut item, frame_now);
            assert_eq!(
                glyphs, glyphs0,
                "{label}: paused 期间重绘的 render plan glyph 必须冻结在 pause 那一刻"
            );
            assert_eq!(
                clips, clips0,
                "{label}: paused 期间重绘的 render plan clip 必须冻结在 pause 那一刻"
            );
            let shaping = shaping_side_tuples(&item, frame_now);
            assert_eq!(
                shaping, shaping0,
                "{label}: paused 期间重绘的 shaping old/new opacity 必须冻结在 pause 那一刻"
            );
        }

        println!("[BEHAVIOR_VERIFY] 评论35: paused 期间重绘冻结在 pause 时刻");
    });
}

/// Issue #826 评论 35 要求的测试 2。
///
/// pause 超过 duration 后直接 render（**仍未 resume**）：active states 仍在、
/// render 仍是 pause 时刻状态，**不得因为 frame_now 已超过 duration 就显示终态**。
#[test]
fn paused_render_plan_does_not_finish_layers_on_wall_clock_time() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let started_at = build_160ms_reveal_reflow_and_shaping(&mut item);

        let t0 = started_at + Duration::from_millis(40);
        let (glyphs0, clips0) = render_plan_tuples(&mut item, t0);
        let shaping0 = shaping_side_tuples(&item, t0);

        {
            let coord = item.pipeline.animation_coordinator_mut();
            let _freed = coord.pause_all(t0);
            assert!(coord.is_paused(), "pause_all 之后必须处于 paused");
        }

        // 墙钟远超 160ms duration，但从未 resume。
        let wall = started_at + Duration::from_millis(400);
        assert!(
            (wall - started_at).as_millis() > 160,
            "前置：墙钟必须已越过 duration"
        );

        {
            let coord = item.pipeline.animation_coordinator();
            assert!(
                coord.active_edit_frontier.is_some(),
                "越过 duration 后未 resume，active_edit_frontier 必须仍在"
            );
            assert!(
                coord.active_reflow.is_some(),
                "越过 duration 后未 resume，active_reflow 必须仍在"
            );
            assert!(
                coord
                    .active_shaping_transition
                    .as_ref()
                    .is_some_and(|shaping| !shaping.is_empty()),
                "越过 duration 后未 resume，active_shaping_transition 必须仍在"
            );
            assert!(
                coord.has_active_text_animation(wall),
                "越过 duration 后未 resume，正文动画仍被视为在跑"
            );
            let sample = coord
                .sample_edit_frontier(wall)
                .expect("越过 duration 后前沿必须可采样");
            assert!(
                sample.progress < 1.0,
                "未 resume 时进度不得按墙钟跑到终态，实际 {}",
                sample.progress
            );
        }

        let (glyphs_wall, clips_wall) = render_plan_tuples(&mut item, wall);
        assert_eq!(
            glyphs_wall, glyphs0,
            "越过 duration 后仍未 resume，render plan glyph 必须停在 pause 时刻"
        );
        assert_eq!(
            clips_wall, clips0,
            "越过 duration 后仍未 resume，render plan clip 必须停在 pause 时刻"
        );
        let shaping_wall = shaping_side_tuples(&item, wall);
        assert_eq!(
            shaping_wall, shaping0,
            "越过 duration 后仍未 resume，shaping old/new opacity 必须停在 pause 时刻"
        );

        println!("[BEHAVIOR_VERIFY] 评论35: 未 resume 不得按墙钟显示终态");
    });
}

/// Issue #826 评论 35 补充测试。
///
/// **未覆盖位置**：评论要求的两条测试都只断言 `build_render_plan_full` 这一个
/// **出口**，没有断言 coordinator 另外两个带 `frame_now` 的正文采样入口 ——
/// `collect_current_visuals(frame_now)`（每笔编辑的 current visual handoff 采样）
/// 和 `has_active_text_animation(frame_now)`（是否继续请求下一帧）。
/// 这两处若仍按墙钟算，即使 render plan 冻住了，交接层与请求帧逻辑照样在
/// paused 期间往前跑。
///
/// 旧实现这两处直接用 `frame_now` → 540ms 处 visuals 已变、状态被判
/// finished → FAIL。
#[test]
fn paused_text_visual_entry_points_freeze_before_resume() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let started_at = build_160ms_reveal_reflow_and_shaping(&mut item);

        let t0 = started_at + Duration::from_millis(40);
        let late = t0 + Duration::from_millis(500);

        {
            let coord = item.pipeline.animation_coordinator_mut();
            let _freed = coord.pause_all(t0);
            assert!(coord.is_paused(), "pause_all 之后必须处于 paused");
        }

        let coord = item.pipeline.animation_coordinator();
        let visuals0 = coord.collect_current_visuals(t0);
        assert!(
            !visuals0.is_empty(),
            "前置：40ms 处必须能采到 current visual clusters"
        );
        let visuals_late = coord.collect_current_visuals(late);
        assert_eq!(
            visuals0, visuals_late,
            "paused 期间 collect_current_visuals 必须冻结在 pause 时刻"
        );

        let running0 = coord.has_active_text_animation(t0);
        assert!(running0, "前置：40ms 处正文动画必须在跑");
        let running_late = coord.has_active_text_animation(late);
        assert_eq!(
            running0, running_late,
            "paused 期间 has_active_text_animation 不得因墙钟越过 duration 而翻转"
        );
        assert!(
            running_late,
            "paused 且未 resume 时正文动画必须仍被判定为在跑"
        );

        println!(
            "[BEHAVIOR_VERIFY] 评论35: collect_current_visuals / has_active_text_animation 也冻结"
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

// =========================================================================
// Issue #826 评论 38: 协同模式 = 一条 caret 运动轨迹 + 文字以 caret 当前帧为
// 吞吐边界 + Reflow 可独立
// =========================================================================

/// 把协同 motion 与前沿的时钟一起拨回 `elapsed_ms` 之前，让测试能确定性地
/// 采样 40/80/120ms 的中间帧，而不是依赖墙钟 sleep。
fn rewind_coordinated_clock_for_test(item: &mut SujianEditorItem, elapsed_ms: u64) {
    let t0 = Instant::now();
    let coord = item.pipeline.animation_coordinator_mut();
    if let Some(frontier) = coord.active_edit_frontier.as_mut() {
        frontier.started_at = t0 - Duration::from_millis(elapsed_ms);
    }
    if let Some(motion) = coord.active_coordinated_caret.as_mut() {
        motion.started_at = t0 - Duration::from_millis(elapsed_ms);
    }
}

/// 协同开 + 正文插入的前置：`A` 文末插入 `X`，前沿 160ms。
/// 返回（插入前 visual x， motion target x， duration_ms）。
fn build_coordinated_insert_for_test(item: &mut SujianEditorItem) -> (f64, f64, u64) {
    item.current_viewport_height = 600.0;
    item.current_coordinated_animation_enabled = true;
    item.current_typing_animation_enabled = true;
    item.current_smooth_cursor_enabled = true;
    item.pipeline.set_typing_animation_duration_ms(160);
    item.set_plain_text(QString::from("A"));
    let _ = item.pipeline.set_selection(1, 1);
    // 实现语义：`pipeline.set_selection` 只改 Core 真相，不搬 visual。
    // 生产里输入前 visual 本来就停在旧 caret 上（idle），这里先 Snap 对齐，
    // 否则 motion 起点取到的是陈旧 visual，与轨迹起点对不上。
    item.snap_next_cursor_update();
    let visual_before = item.cursor_ctrl.visual_x;
    item.insert_text(QString::from("X"));
    assert_eq!(item.pipeline.committed_text(), "AX");
    let coord = item.pipeline.animation_coordinator();
    assert!(
        coord.has_active_edit_frontier(),
        "前置：必须建出遮罩前沿"
    );
    let (target_x, _, duration_ms, _) = coord
        .coordinated_caret_for_test()
        .expect("前置：协同开时正文插入必须由单条 motion 接管");
    (visual_before, target_x, duration_ms)
}

/// 评论 38 要求①：`coordinated_insert_caret_and_reveal_share_one_progress`。
///
/// `A -> AX`（duration = 160ms）：40/80/120ms 三个中间帧，caret 的 x 必须
/// 等于前沿当前 Reveal 边界 x（不只终点相等）。同时断：两边 progress 相等、
/// 独立 Tween 不存在、motion 时长是 typing 时长。
#[test]
fn coordinated_insert_caret_and_reveal_share_one_progress() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let (start_x, target_x, duration_ms) =
            build_coordinated_insert_for_test(&mut item);
        assert_eq!(
            duration_ms, 160,
            "协同 motion 时长必须是 typing duration，实际 {}",
            duration_ms
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "协同接管后不得再建独立光标 Tween"
        );
        assert!(
            (target_x - start_x).abs() > 1.0,
            "前置：插入 X 后 caret 必须水平移动，实际 {} -> {}",
            start_x,
            target_x
        );

        for elapsed in [40u64, 80, 120] {
            rewind_coordinated_clock_for_test(&mut item, elapsed);
            let now = Instant::now();
            let coord = item.pipeline.animation_coordinator_mut();
            let sample = coord
                .sample_edit_frontier(now)
                .expect("前沿必须还在");
            let caret = coord
                .sample_coordinated_caret(now)
                .expect("协同 motion 必须还在");
            assert!(
                (caret.progress - sample.progress).abs() < 0.02,
                "{}ms：协同 caret 进度必须等于前沿进度，实际 {} vs {}",
                elapsed,
                caret.progress,
                sample.progress
            );
            assert!(
                (caret.x - start_x) * (caret.x - target_x) < 0.0,
                "{}ms：caret 必须在起点与终点之间，实际 x={}（{} -> {}）",
                elapsed,
                caret.x,
                start_x,
                target_x
            );
            // 生产路径断言：吐字遮罩（`hidden_new_text_rects`，吃投影距离）
            // 的左沿必须就是 caret.x —— 单个 X cluster 只产出一块 mask。
            assert!(
                sample.coordinated.is_some(),
                "{}ms：协同态 sample 必须带投影边界",
                elapsed
            );
            let frontier = coord.active_edit_frontier.as_ref().expect("前沿必须还在");
            let masks = frontier.hidden_new_text_rects(&sample);
            assert_eq!(
                masks.len(),
                1,
                "{}ms：单个插入 cluster 应只有一块吐字遮罩，实际 {} 块",
                elapsed,
                masks.len()
            );
            assert!(
                (masks[0].x - caret.x).abs() < 1.0,
                "{}ms：caret.x 必须等于 Reveal 边界 x，实际 caret={} mask_x={}",
                elapsed,
                caret.x,
                masks[0].x
            );
        }

        println!("[BEHAVIOR_VERIFY] 评论38①：协同插入 caret 与 Reveal 边界共享同一 progress");
    });
}

/// 评论 38 要求②：`coordinated_delete_caret_and_conceal_share_one_progress`。
///
/// Backspace 删 X（`AX -> A`）：caret 回退多少，Conceal 就吞到同一个视觉边界，
/// 每个中间帧都一致（keep 矩形右沿 == caret.x）。
#[test]
fn coordinated_delete_caret_and_conceal_share_one_progress() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from("AX"));
        let _ = item.pipeline.set_selection(2, 2);
        item.snap_next_cursor_update();
        let visual_before = item.cursor_ctrl.visual_x;
        item.delete_backward();
        assert_eq!(item.pipeline.committed_text(), "A");

        let (target_x, _, duration_ms, _) = item
            .pipeline
            .animation_coordinator()
            .coordinated_caret_for_test()
            .expect("协同开时删除必须由单条 motion 接管");
        assert_eq!(duration_ms, 160);
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "协同接管后不得再建独立光标 Tween"
        );
        assert!(
            visual_before > target_x + 1.0,
            "前置：Backspace 后 caret 必须回退，实际 {} -> {}",
            visual_before,
            target_x
        );

        for elapsed in [40u64, 80, 120] {
            rewind_coordinated_clock_for_test(&mut item, elapsed);
            let now = Instant::now();
            let coord = item.pipeline.animation_coordinator_mut();
            let sample = coord
                .sample_edit_frontier(now)
                .expect("前沿必须还在");
            let caret = coord
                .sample_coordinated_caret(now)
                .expect("协同 motion 必须还在");
            assert!(
                (caret.progress - sample.progress).abs() < 0.02,
                "{}ms：协同 caret 进度必须等于前沿进度",
                elapsed
            );
            let frontier = coord.active_edit_frontier.as_ref().expect("前沿必须还在");
            let glyphs = frontier.old_overlay_glyphs(&sample);
            assert!(!glyphs.is_empty(), "{}ms：吞字中途必须有旧字 overlay", elapsed);
            let keep_right = glyphs
                .iter()
                .map(|g| g.dest_rect.x + g.dest_rect.w)
                .fold(f64::MIN, f64::max);
            assert!(
                (caret.x - keep_right).abs() < 1.0,
                "{}ms：caret.x 必须等于 Conceal 保留沿，实际 caret={} keep_right={}",
                elapsed,
                caret.x,
                keep_right
            );
        }

        println!("[BEHAVIOR_VERIFY] 评论38②：协同删除 caret 与 Conceal 边界共享同一 progress");
    });
}



/// 评论 38 要求④：`coordinated_mode_ignores_smooth_cursor_duration`。
///
/// typing = 160ms、smooth cursor = 20ms：协同开时正文编辑的 caret 仍跑 160ms；
/// 纯方向键移动才用 20ms。
#[test]
fn coordinated_mode_ignores_smooth_cursor_duration() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.current_cursor_animation_duration_ms = 20;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from("A"));
        let _ = item.pipeline.set_selection(1, 1);
        item.snap_next_cursor_update();
        item.insert_text(QString::from("X"));

        let (_, _, duration_ms, _) = item
            .pipeline
            .animation_coordinator()
            .coordinated_caret_for_test()
            .expect("协同开时正文插入必须由 motion 接管");
        assert_eq!(
            duration_ms, 160,
            "协同正文编辑的 caret 必须跑 typing duration（160ms），不能被 smooth duration（20ms）带偏"
        );

        // 纯方向键：独立 Tween，用 smooth duration。
        item.move_cursor_horizontal(false, false);
        assert!(
            item.pipeline
                .animation_coordinator()
                .coordinated_caret_for_test()
                .is_none(),
            "独立光标移动必须取代协同 motion"
        );
        let anim = item
            .cursor_ctrl
            .animation
            .as_ref()
            .expect("纯方向键必须建独立 Tween");
        assert_eq!(
            anim.duration_ms, 20,
            "纯光标移动用 smooth duration，实际 {}",
            anim.duration_ms
        );

        println!("[BEHAVIOR_VERIFY] 评论38④：协同态忽略 smooth 时长，纯光标移动仍用 smooth 时长");
    });
}

/// 评论 38 要求⑤：`coordinated_off_keeps_independent_timelines`。
///
/// 协同关：typing = 160ms、cursor = 80ms 时两者独立并存（前沿 160ms +
/// 独立 Tween 80ms），证明没有把所有模式硬绑在一起。
#[test]
fn coordinated_off_keeps_independent_timelines() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = false;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.current_cursor_animation_duration_ms = 80;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from("A"));
        let _ = item.pipeline.set_selection(1, 1);
        item.snap_next_cursor_update();
        item.insert_text(QString::from("X"));

        let coord = item.pipeline.animation_coordinator();
        assert!(
            coord.coordinated_caret_for_test().is_none(),
            "协同关时不得建协同 motion"
        );
        let frontier = coord
            .active_edit_frontier
            .as_ref()
            .expect("协同关时前沿仍按 typing 开关建立");
        assert_eq!(
            frontier.duration_ms, 160,
            "非协同前沿用 typing duration，实际 {}",
            frontier.duration_ms
        );
        let anim = item
            .cursor_ctrl
            .animation
            .as_ref()
            .expect("协同关时光标仍走独立 Tween");
        assert_eq!(
            anim.duration_ms, 80,
            "非协同光标用 smooth duration，实际 {}",
            anim.duration_ms
        );

        println!("[BEHAVIOR_VERIFY] 评论38⑤：协同关时两条独立 timeline 并存");
    });
}

/// 评论 38 要求⑥：`rapid_typing_retargets_single_coordinated_motion`。
///
/// 40ms 输入第二个字：不新增第二条 caret timeline（`Option` 只有一份），
/// 从当前视觉 caret 直接 retarget 到最新 caret。
#[test]
fn rapid_typing_retargets_single_coordinated_motion() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let _ = build_coordinated_insert_for_test(&mut item);

        // 第一笔播到 40ms，记当前屏幕 caret。
        rewind_coordinated_clock_for_test(&mut item, 40);
        let now = Instant::now();
        let mid_x = item
            .pipeline
            .animation_coordinator_mut()
            .sample_coordinated_caret(now)
            .expect("第一笔 motion 必须还在")
            .x;

        // 第二笔：同一 burst 内再输入一个字。
        item.insert_text(QString::from("Y"));
        assert_eq!(item.pipeline.committed_text(), "AXY");
        assert!(
            item.pipeline.animation_coordinator().has_active_edit_frontier(),
            "连续输入必须保持单一前沿"
        );
        let (target2, _, _, _) = item
            .pipeline
            .animation_coordinator()
            .coordinated_caret_for_test()
            .expect("连续输入必须保持单份协同 motion");
        assert!(
            (target2 - item.cursor_ctrl.target_x).abs() < 1e-9,
            "retarget 目标必须是最新 canonical caret"
        );
        // retarget 从当前屏幕位置继续：travelled_base 继承旧 motion 当前距离，
        // 同一时刻采样新 motion 必须≈旧采样 mid（不对回轨迹起点、不跳）。
        let now2 = Instant::now();
        let cur_x = item
            .pipeline
            .animation_coordinator_mut()
            .sample_coordinated_caret(now2)
            .expect("retarget 后 motion 必须还在")
            .x;
        assert!(
            (cur_x - mid_x).abs() < 1.0,
            "retarget 必须从当前屏幕 caret 继续，实际 cur={} mid={}",
            cur_x,
            mid_x
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "连续协同输入不得孵化独立 Tween"
        );

        println!("[BEHAVIOR_VERIFY] 评论38⑥：快速输入只 retarget 单份协同 motion");
    });
}

/// Issue #826 评论 39 BLOCKER 1 / 评论 40 BLOCKER 1：
/// `coordinated_enter_caret_moves_in_both_axes_before_finish`。
///
/// `ABCDE|FGHIJ` 按 Enter：newline 没有 Reveal glyph（Reveal path 为空），
/// FGHIJ 只做 Reflow 到下一行，caret 从旧行末走到下一行起点。motion 必须活到
/// typing duration 结束（160ms 才落 target），且 40/80/120ms 两个轴都要推进
/// （评论 40：只更新 x、最后一帧跳 y 不算跨行协同）；期间独立 Tween 始终不得出现。
#[test]
fn coordinated_enter_caret_moves_in_both_axes_before_finish() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from("ABCDEFGHIJ"));
        let _ = item.pipeline.set_selection(5, 5);
        item.snap_next_cursor_update();
        let start_x = item.cursor_ctrl.visual_x;
        let start_y = item.cursor_ctrl.visual_y;
        item.insert_text(QString::from("\n"));
        assert_eq!(
            item.pipeline.committed_text(),
            "ABCDE\nFGHIJ",
            "前置：Enter 必须把段落从中间劈开"
        );

        let coord = item.pipeline.animation_coordinator();
        let frontier = coord
            .active_edit_frontier
            .as_ref()
            .expect("前置：Enter 也必须建前沿（协同 clock 来源）");
        let reveal_total: f64 = frontier
            .reveal
            .regions
            .iter()
            .map(|r| r.path.total_length)
            .sum();
        assert!(
            reveal_total <= 1e-9,
            "前置：newline 没有可见 Reveal path，实际 total={}",
            reveal_total
        );
        assert!(
            coord.active_reflow.is_some(),
            "前置：FGHIJ 下移必须有 Reflow"
        );
        let (target_x, target_y, _, _) = coord
            .coordinated_caret_for_test()
            .expect("前置：Enter 必须建协同 motion（caret-only 段）");
        assert!(
            (target_y - start_y).abs() > 1.0,
            "前置：Enter 后 caret 必须换行，实际 y {} -> {}",
            start_y,
            target_y
        );

        let mut previous_y_err = (start_y - target_y).abs();
        for elapsed in [40u64, 80, 120] {
            rewind_coordinated_clock_for_test(&mut item, elapsed);
            let now = Instant::now();
            let coord = item.pipeline.animation_coordinator_mut();
            // 第一帧 tick 必然清掉空 path 前沿 —— motion 不得被连带。
            assert!(coord.tick(now), "motion 存活时必须继续续帧");
            assert!(
                coord.active_edit_frontier.is_none(),
                "{}ms：空 path 前沿第一帧即 finished",
                elapsed
            );
            let caret = coord
                .sample_coordinated_caret(now)
                .expect("BLOCKER1：motion 不得第一帧死亡，必须活到 typing duration 结束");
            assert!(
                caret.progress > 0.0 && caret.progress < 1.0 && !caret.finished,
                "{}ms：motion 必须在途中，实际 progress={}",
                elapsed,
                caret.progress
            );
            // 评论 40 BLOCKER 1：两个轴都要动，不能只动 x 最后一帧跳 y。
            assert!(
                (caret.x - start_x).abs() > 1e-6 || (start_x - target_x).abs() <= 1e-6,
                "{}ms：x 轴必须推进，实际 x={}（{} -> {}）",
                elapsed,
                caret.x,
                start_x,
                target_x
            );
            assert!(
                caret.y > start_y.min(target_y) + 1e-6
                    && caret.y < start_y.max(target_y) - 1e-6,
                "{}ms：y 必须在两行之间（不能只动 x），实际 y={}（{} -> {}）",
                elapsed,
                caret.y,
                start_y,
                target_y
            );
            let y_err = (caret.y - target_y).abs();
            assert!(
                y_err < previous_y_err,
                "{}ms：|y-target_y| 必须单调递减，实际 {}（上一帧 {}）",
                elapsed,
                y_err,
                previous_y_err
            );
            previous_y_err = y_err;
        }
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "全程不得孵化独立 Tween"
        );

        // 到点结束并精确落 target。
        rewind_coordinated_clock_for_test(&mut item, 200);
        let now = Instant::now();
        let end = item
            .pipeline
            .animation_coordinator_mut()
            .sample_coordinated_caret(now)
            .expect("终点采样必须还有一帧（finished 那一帧）");
        assert!(end.finished, "200ms 必须结束");
        assert!(
            (end.x - target_x).abs() < 1e-9 && (end.y - target_y).abs() < 1e-9,
            "终点必须精确落 canonical caret，实际 ({}, {}) vs ({}, {})",
            end.x,
            end.y,
            target_x,
            target_y
        );
        assert!(
            !item
                .pipeline
                .animation_coordinator()
                .has_active_coordinated_caret(),
            "结束后 motion 必须清掉"
        );

        println!("[BEHAVIOR_VERIFY] 评论40①：Enter caret 两轴同时推进到 typing 结束");
    });
}

/// Issue #826 评论 39 BLOCKER 1：`coordinated_shaping_only_edit_keeps_caret_until_typing_duration_finishes`。
///
/// synthetic：changed range 全部被 shaping-owned 挖空（scalar Reveal/Conceal
/// path 全空，只有 motion 在跑）。断言 caret 不会第一帧死亡、按 typing
/// duration 走完并精确落 target。
#[test]
fn coordinated_shaping_only_edit_keeps_caret_until_typing_duration_finishes() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let t0 = Instant::now();
        // 空 inserted ranges ⇒ Reveal regions 为空（等价于“全被 shaping 接管挖空”）。
        let empty_snapshot = EditorLayoutSnapshot::new(
            LayoutSnapshot::empty_for_tests(),
            Vec::new(),
            None,
            None,
            CaretAffinity::Downstream,
        );
        let frontier = EditFrontierState::begin_insert(
            String::new(),
            empty_snapshot,
            String::new(),
            Vec::new(),
            OffsetMap::from_single_edit(0, (0, 0), 0),
            Vec::new(),
            t0,
            160,
        );
        assert!(
            frontier.reveal.regions.is_empty(),
            "前置：synthetic 前沿 Reveal 必须为空"
        );
        let coord = item.pipeline.animation_coordinator_mut();
        coord.active_edit_frontier = Some(frontier);
        assert!(
            coord.begin_or_retarget_coordinated_caret(10.0, 20.0, 50.0, 40.0, t0),
            "有前沿（即使空 path）就必须能建 motion"
        );

        // 第一帧 tick：空 path 前沿 finished 被清，motion 必须存活。
        let t1 = t0 + Duration::from_millis(10);
        assert!(coord.tick(t1), "motion 存活时 tick 必须返回 true（仍需续帧）");
        assert!(
            coord.active_edit_frontier.is_none(),
            "空 path 前沿第一帧即 finished"
        );
        assert!(
            coord.has_active_coordinated_caret(),
            "BLOCKER1：scalar 全空时 motion 不得第一帧死亡"
        );

        let mid = coord
            .sample_coordinated_caret(t0 + Duration::from_millis(80))
            .expect("80ms motion 必须还在");
        assert!(!mid.finished && mid.progress > 0.0 && mid.progress < 1.0);
        assert!(
            (mid.x - 10.0) * (mid.x - 50.0) < 0.0,
            "caret 必须向 target 前进，实际 x={}",
            mid.x
        );
        // 评论 40 BLOCKER 1：CaretOnly 段 y 也必须推进（start=(10,20) target=(50,40)）。
        assert!(
            mid.y > 20.0 && mid.y < 40.0,
            "CaretOnly 段 80ms y 必须在 (20,40)，实际 {}",
            mid.y
        );

        let end = coord
            .sample_coordinated_caret(t0 + Duration::from_millis(200))
            .expect("终点采样必须还有一帧");
        assert!(end.finished, "200ms 必须结束");
        assert!(
            (end.x - 50.0).abs() < 1e-9 && (end.y - 40.0).abs() < 1e-9,
            "终点必须精确落 target"
        );
        assert!(
            !coord.has_active_coordinated_caret(),
            "结束后 motion 必须清掉"
        );

        println!("[BEHAVIOR_VERIFY] 评论39①：shaping 全接管时 caret 按 typing duration 走完");
    });
}

/// Issue #826 评论 40 BLOCKER 2：
/// `coordinated_forward_delete_conceals_progressively_while_caret_stays_at_logical_target`。
///
/// 正文 `AX`，caret 在 `A|X`，Forward Delete：逻辑 caret 删除前后同点
/// （start==target），但被删 X 必须沿 Forward 路径被同一份 clock 逐步吞掉，
/// 不能整个 duration 不吞、最后一帧突然消失（旧 #722 问题3 同一个 bug）。
#[test]
fn coordinated_forward_delete_conceals_progressively_while_caret_stays_at_logical_target() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from("AX"));
        let _ = item.pipeline.set_selection(1, 1);
        item.snap_next_cursor_update();
        let start_x = item.cursor_ctrl.visual_x;
        item.delete_forward();
        assert_eq!(item.pipeline.committed_text(), "A", "前置：Forward Delete 立即删 X");

        let coord = item.pipeline.animation_coordinator();
        let motion = coord
            .coordinated_caret_for_test()
            .expect("Forward Delete 也必须由协同 motion 接管");
        assert_eq!(motion.2, 160, "motion 时长必须是 typing duration");
        assert!(
            (motion.0 - start_x).abs() < 1.0,
            "Forward Delete 的 caret target 必须与 start 同点，实际 {} vs {}",
            motion.0,
            start_x
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "协同接管后不得再建独立 Tween"
        );

        let mut previous_width = f64::MAX;
        for elapsed in [40u64, 80, 120] {
            rewind_coordinated_clock_for_test(&mut item, elapsed);
            let now = Instant::now();
            let coord = item.pipeline.animation_coordinator_mut();
            let sample = coord.sample_edit_frontier(now).expect("前沿必须还在");
            let glyphs = coord.old_overlay_glyphs_for(&sample);
            let width: f64 = glyphs.iter().map(|g| g.dest_rect.w).sum();
            assert!(
                width > 0.0,
                "{}ms：X 必须还没吞完（>0），实际 {}",
                elapsed,
                width
            );
            assert!(
                width < previous_width,
                "{}ms：overlay 宽度必须严格递减，实际 {}（上一帧 {}）",
                elapsed,
                width,
                previous_width
            );
            previous_width = width;
            let caret = coord
                .sample_coordinated_caret(now)
                .expect("Forward Delete 的 motion 必须活到 typing 结束");
            assert!(
                (caret.x - start_x).abs() < 1.0,
                "{}ms：drawn caret 可以原地，实际 {} vs {}",
                elapsed,
                caret.x,
                start_x
            );
        }

        // 160ms：X 完全消失。
        rewind_coordinated_clock_for_test(&mut item, 200);
        let now = Instant::now();
        let coord = item.pipeline.animation_coordinator_mut();
        let sample = coord.sample_edit_frontier(now).expect("frontier 收口帧");
        let width: f64 = coord
            .old_overlay_glyphs_for(&sample)
            .iter()
            .map(|g| g.dest_rect.w)
            .sum();
        assert!(width <= 1e-9, "160ms 后 overlay 必须完全消失，实际 {}", width);

        println!("[BEHAVIOR_VERIFY] 评论40②：Forward Delete 逐步吞字、caret 原地");
    });
}

/// Issue #826 评论 40 BLOCKER 2（续）：
/// `coordinated_forward_delete_does_not_collapse_motion_clock_when_start_equals_target`。
///
/// start==target 不能等价于「正文动画已经完成」：motion 仍按 typing duration
/// 走完，期间的 conceal 进度必须真的推进。
#[test]
fn coordinated_forward_delete_does_not_collapse_motion_clock_when_start_equals_target() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from("AX"));
        let _ = item.pipeline.set_selection(1, 1);
        item.snap_next_cursor_update();
        item.delete_forward();

        let coord = item.pipeline.animation_coordinator();
        let motion = coord
            .coordinated_caret_for_test()
            .expect("Forward Delete 必须有 motion");
        assert!(
            (motion.0 - item.cursor_ctrl.visual_x).abs() < 1.0
                // start==target（target 字段 ==0）
                && motion.3 < 1.0,
            "前置：start==target 时 motion 自身轨迹长度可以为 0（caret 原地），\
             但 clock 必须仍然存在并走满 typing duration，实际 total={}",
            motion.3
        );

        // conceal 进度必须真的随 time progress 推进（走的是 conceal.advanced）。
        rewind_coordinated_clock_for_test(&mut item, 40);
        let now = Instant::now();
        let coord = item.pipeline.animation_coordinator_mut();
        let sample = coord.sample_edit_frontier(now).expect("前沿必须还在");
        let boundary = sample.coordinated.expect("协同态必须带边界");
        let conceal_total: f64 = coord
            .active_edit_frontier
            .as_ref()
            .expect("前沿必须还在")
            .conceal
            .regions
            .iter()
            .map(|r| r.path.total_length)
            .sum();
        assert!(
            boundary.conceal_distance > 0.0 && boundary.conceal_distance < conceal_total,
            "40ms conceal 必须在中途（不能因 start==target 就等价完成），\
             实际 {} / {}",
            boundary.conceal_distance,
            conceal_total
        );

        println!("[BEHAVIOR_VERIFY] 评论40②：start==target 不塌缩 motion clock");
    });
}

// =========================================================================
// Issue #826 评论 41: 单字软换行时文字段不能被钉成跨行斜线
// =========================================================================

fn test_shaping_identity() -> ShapingIdentity {
    ShapingIdentity {
        text_content_hash: 1,
        raw_font_fingerprint: String::from("test-font"),
        glyph_indexes_hash: 1,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 1,
    }
}

/// Issue #826 评论 41：`coordinated_frontier_segments_never_change_visual_line_when_pinned`。
///
/// synthetic：单个 Frontier text segment（y=40, x=10..30），传
/// `start=(100,10)`、`target=(30,40)`。生成 motion 后，原 text segment 必须
/// 仍是 `y_from == y_to == 40`（不被首尾 pin 改行）；start 到它的跨行差异必须
/// 由额外 connector 承担。
#[test]
fn coordinated_frontier_segments_never_change_visual_line_when_pinned() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let t0 = Instant::now();
        let snapshot = EditorLayoutSnapshot::new(
            LayoutSnapshot::empty_for_tests(),
            vec![PreparedLineSnapshot::stub_for_tests(
                0,
                40.0,
                0,
                vec![LineClusterSnapshot {
                    byte_start: 0,
                    byte_end: 1,
                    source_rect: SourceRect {
                        x: 10.0,
                        y: 0.0,
                        w: 20.0,
                        h: 20.0,
                    },
                    shaping_identity: test_shaping_identity(),
                }],
            )],
            None,
            None,
            CaretAffinity::Downstream,
        );
        let frontier = EditFrontierState::begin_insert(
            String::new(),
            snapshot,
            String::from("x"),
            vec![(0, 1)],
            OffsetMap::from_single_edit(0, (0, 0), 0),
            Vec::new(),
            t0,
            160,
        );
        let coord = item.pipeline.animation_coordinator_mut();
        coord.active_edit_frontier = Some(frontier);
        assert!(
            coord.begin_or_retarget_coordinated_caret(100.0, 10.0, 30.0, 40.0, t0),
            "必须建出 motion"
        );
        let boundaries = coord.coordinated_boundary_segments_for_test();
        assert_eq!(boundaries.len(), 1, "只有一条文字段，实际 {}", boundaries.len());
        let b = boundaries[0];
        assert!(
            (b.2 - 40.0).abs() < 1e-9 && (b.3 - 40.0).abs() < 1e-9,
            "文字段必须保持 y_from == y_to == 40（不被 pin 成斜线），实际 ({}, {})",
            b.2,
            b.3
        );
        assert!(
            coord.coordinated_has_connector_for_test(),
            "start 与文字段入口的跨行差异必须由 connector 承担"
        );
        println!("[BEHAVIOR_VERIFY] 评论41：pin 不改文字段视觉行");
    });
}

/// 100 个 CJK 的第一视觉行 byte_end（= 该行容量，字节数）。
fn first_visual_line_byte_end(item: &SujianEditorItem) -> usize {
    item.editor_layout
        .cache()
        .expect("必须有排版缓存")
        .lines[0]
        .byte_end
}

/// Issue #826 评论 42：某视觉行真正的 Qt caret top（`cursor_rect_for_line`）。
///
/// `visual_line_top` 只是 QTextLine 行顶；drawn caret 的 y 是
/// `cursor_rect_for_line(...).0`，两者默认差 `top_padding`。
fn expected_caret_top_for_line_top(item: &SujianEditorItem, line_top: f64) -> f64 {
    let cache = item.editor_layout.cache().expect("必须有排版缓存");
    let vl = cache
        .lines
        .iter()
        .find(|l| (l.y - line_top).abs() <= 0.5)
        .expect("必须能按 visual_line_top 找到对应 VisualLine");
    crate::editor::layout::cursor_rect_for_line(vl, f64::from(cache.font_size), &cache.font_family).0
}

/// Issue #826 评论 41：`coordinated_single_glyph_soft_wrap_does_not_turn_frontier_segment_diagonal`。
///
/// 真实 Qt layout：把正文设为刚好写满一行，caret 在行末，插入**一个**可见 CJK
/// 让它落到第二行。此时 Reveal 只有一条 visible segment（第二行），motion 的
/// 文字段必须保持 `y_from == y_to == 第二行 y`，不允许出现 `y_from=第一行,
/// y_to=第二行` 的单一文字段；跨行差异必须由 connector 承担。
#[test]
fn coordinated_single_glyph_soft_wrap_does_not_turn_frontier_segment_diagonal() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        // 先用长串找出第一行容量。
        item.set_plain_text(QString::from(
            "界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界",
        ));
        let capacity_bytes = first_visual_line_byte_end(&item);
        let capacity_chars = capacity_bytes / 3; // CJK 3 byte
        assert!(capacity_chars >= 2, "前置：行容量太小 {}", capacity_chars);
        // 正文设为刚好写满一行（不含第二行）。
        item.set_plain_text(QString::from("界".repeat(capacity_chars)));
        assert_eq!(
            item.editor_layout.cache().expect("cache").lines.len(),
            1,
            "前置：capacity_chars 个字必须刚好一行"
        );
        let _ = item.pipeline.set_selection(capacity_chars * 3, capacity_chars * 3);
        item.snap_next_cursor_update();
        let start_y = item.cursor_ctrl.visual_y;
        // 插入一个可见 CJK → 软换行到第二行。
        item.insert_text(QString::from("界"));
        assert_eq!(item.pipeline.committed_text().chars().count(), capacity_chars + 1);

        let coord = item.pipeline.animation_coordinator();
        let frontier = coord
            .active_edit_frontier
            .as_ref()
            .expect("前置：必须建前沿");
        let reveal_segments: Vec<(f64, f64)> = frontier
            .reveal
            .regions
            .iter()
            .flat_map(|r| r.path.segments.iter().map(|s| (s.y, s.x_left)))
            .collect();
        assert_eq!(
            reveal_segments.len(),
            1,
            "前置：单字软换行的 Reveal 必须只有一条 visible segment，实际 {}",
            reveal_segments.len()
        );
        let line2_y = reveal_segments[0].0;
        let line2_caret_top = expected_caret_top_for_line_top(&item, line2_y);
        assert!(
            (line2_y - start_y).abs() > 1.0,
            "前置：插入的字必须在第二行，实际 reveal y={} start_y={}",
            line2_y,
            start_y
        );
        assert!(
            (line2_caret_top - line2_y).abs() > 0.1,
            "前置：默认 font=22px / line_spacing=1.5 下 caret_top 与 visual_line_top \
             必须不同（top_padding != 0），实际 caret_top={} line_top={}",
            line2_caret_top,
            line2_y
        );
        let boundaries = coord.coordinated_boundary_segments_for_test();
        assert_eq!(boundaries.len(), 1, "文字段只有一条");
        let b = boundaries[0];
        assert!(
            (b.2 - line2_caret_top).abs() < 1e-6 && (b.3 - line2_caret_top).abs() < 1e-6,
            "唯一文字段必须保持 y_from == y_to == 第二行**真实 caret top**\
             （评论 42：不能用 visual_line_top，也不能是跨行斜线），\
             实际 y_from={} y_to={}，第二行 caret_top={} line_top={}",
            b.2,
            b.3,
            line2_caret_top,
            line2_y
        );
        assert!(
            (b.2 - line2_y).abs() > 0.1,
            "文字段 y 不得等于 visual_line_top（评论 42 的 bug）"
        );
        assert!(
            coord.coordinated_has_connector_for_test(),
            "旧 caret 在第一行、文字段在第二行，必须由 connector 承担跨行位移"
        );
        println!("[BEHAVIOR_VERIFY] 评论41/42：单字软换行文字段 y 用真实 caret top");
    });
}

/// Issue #826 评论 41：`coordinated_single_glyph_wrap_reveal_waits_during_connector_then_matches_caret_boundary`。
///
/// 同一个单字软换行场景：connector 中间帧 caret 正在换行、Reveal 距离冻结在
/// 0（X 不能提前吐）；进入第二行文字段后 caret.y == 第二行 y 且 Reveal 边界
/// x == caret.x。
#[test]
fn coordinated_single_glyph_wrap_reveal_waits_during_connector_then_matches_caret_boundary() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from(
            "界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界",
        ));
        let capacity_bytes = first_visual_line_byte_end(&item);
        let capacity_chars = capacity_bytes / 3;
        item.set_plain_text(QString::from("界".repeat(capacity_chars)));
        let _ = item.pipeline.set_selection(capacity_chars * 3, capacity_chars * 3);
        item.snap_next_cursor_update();
        let start_y = item.cursor_ctrl.visual_y;
        item.insert_text(QString::from("界"));

        let line2_y = item
            .pipeline
            .animation_coordinator()
            .active_edit_frontier
            .as_ref()
            .expect("前沿")
            .reveal
            .regions[0]
            .path
            .segments[0]
            .y;
        assert!((line2_y - start_y).abs() > 1.0);
        let line2_caret_top = expected_caret_top_for_line_top(&item, line2_y);

        let mut saw_connector = false;
        let mut saw_boundary = false;
        for elapsed in (0..=160).step_by(10) {
            rewind_coordinated_clock_for_test(&mut item, elapsed);
            let now = Instant::now();
            let coord = item.pipeline.animation_coordinator_mut();
            let Some(sample) = coord.sample_edit_frontier(now) else {
                continue;
            };
            let Some((_, distance)) = coord.coordinated_distance_for_test(now) else {
                continue;
            };
            let Some(boundary) = sample.coordinated else {
                continue;
            };
            let Some((is_boundary, caret_x, caret_y, _frozen)) =
                coord.coordinated_segment_at_test(distance)
            else {
                continue;
            };
            if !is_boundary {
                saw_connector = true;
                assert!(
                    boundary.reveal_distance <= 1e-6,
                    "{}ms：connector 期间 Reveal 距离必须冻结在 0（X 不能提前吐），\
                     实际 {}",
                    elapsed,
                    boundary.reveal_distance
                );
                continue;
            }
            // 进入第二行文字段后：caret.y == 第二行**真实 caret top**，
            // 边界 x == caret.x（评论 42：不再是 visual_line_top）。
            assert!(
                (caret_y - line2_caret_top).abs() < 1e-6,
                "{}ms：进入文字段后 caret.y 必须等于第二行真实 caret top，实际 {} vs {}",
                elapsed,
                caret_y,
                line2_caret_top
            );
            let frontier = coord.active_edit_frontier.as_ref().expect("前沿");
            let masks = frontier.hidden_new_text_rects(&sample);
            if let Some(mask) = masks.first() {
                assert!(
                    (mask.x - caret_x).abs() < 1.0,
                    "{}ms：文字段内 Reveal 边界 x 必须等于 caret.x，实际 {} vs {}",
                    elapsed,
                    mask.x,
                    caret_x
                );
                saw_boundary = true;
            }
        }
        assert!(saw_connector, "必须观察到 connector 帧");
        assert!(saw_boundary, "必须观察到进入文字段后的帧");
        println!("[BEHAVIOR_VERIFY] 评论41：connector 期间不吐、进入文字段后边界==caret");
    });
}
/// Issue #826 评论 42 的共同前置：真实 Qt layout 下单字软换行。
///
/// 返回（item, 第一行 caret top / start_y, 第二行 `visual_line_top`,
/// 第二行真实 `caret_top`）。`capacity` 通过长串实测，正文设为刚好一行再插一个
/// 可见 CJK → 落到第二行，Reveal 只有一条 visible segment。
fn build_single_glyph_wrap_for_test() -> (SujianEditorItem, f64, f64, f64) {
    let mut item = SujianEditorItem::default();
    item.current_viewport_height = 600.0;
    item.current_coordinated_animation_enabled = true;
    item.current_typing_animation_enabled = true;
    item.current_smooth_cursor_enabled = true;
    item.pipeline.set_typing_animation_duration_ms(160);
    item.set_plain_text(QString::from(
        "界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界",
    ));
    let capacity_chars = first_visual_line_byte_end(&item) / 3;
    assert!(capacity_chars >= 2, "前置：行容量太小 {}", capacity_chars);
    item.set_plain_text(QString::from("界".repeat(capacity_chars)));
    assert_eq!(
        item.editor_layout.cache().expect("cache").lines.len(),
        1,
        "前置：capacity_chars 个字必须刚好一行"
    );
    let _ = item.pipeline.set_selection(capacity_chars * 3, capacity_chars * 3);
    item.snap_next_cursor_update();
    let start_y = item.cursor_ctrl.visual_y;
    item.insert_text(QString::from("界"));
    let line2_y = item
        .pipeline
        .animation_coordinator()
        .active_edit_frontier
        .as_ref()
        .expect("前置：必须建前沿")
        .reveal
        .regions[0]
        .path
        .segments[0]
        .y;
    let line2_caret_top = expected_caret_top_for_line_top(&item, line2_y);
    (item, start_y, line2_y, line2_caret_top)
}

/// Issue #826 评论 42：`coordinated_boundary_uses_qt_caret_top_not_visual_line_top`。
///
/// 跳行 Boundary 的 drawn caret y 必须是该视觉行真实的
/// `cursor_rect_for_line(...).0`，而不是 `FrontierSegment.y`（= visual_line_top）。
#[test]
fn coordinated_boundary_uses_qt_caret_top_not_visual_line_top() {
    run_on_qt_thread(|| {
        let (item, _start_y, line2_y, line2_caret_top) = build_single_glyph_wrap_for_test();
        assert!(
            (line2_caret_top - line2_y).abs() > 0.1,
            "前置：默认字体下 caret_top({}) 必须与 visual_line_top({}) 不同",
            line2_caret_top,
            line2_y
        );
        let boundaries = item
            .pipeline
            .animation_coordinator()
            .coordinated_boundary_segments_for_test();
        assert_eq!(boundaries.len(), 1, "单字软换行只有一条文字段");
        let b = boundaries[0];
        assert!(
            (b.2 - line2_caret_top).abs() < 1e-6 && (b.3 - line2_caret_top).abs() < 1e-6,
            "Boundary 的 y_from/y_to 必须是 Qt 真实 caret top={}，实际 ({}, {})",
            line2_caret_top,
            b.2,
            b.3
        );
        assert!(
            (b.2 - line2_y).abs() > 0.1,
            "Boundary y 不得等于 visual_line_top={}（评论 42 的 bug）",
            line2_y
        );
        println!("[BEHAVIOR_VERIFY] 评论42①：跳行 Boundary 用 Qt 真实 caret top");
    });
}

/// Issue #826 评论 42：`coordinated_single_glyph_wrap_keeps_caret_vertically_centered_on_boundary`。
///
/// 单字软换行进入 Boundary 后，drawn caret.y 必须停在该视觉行真实 caret top
/// （与 canonical 光标一致）；Reveal 边界 x 仍等于 caret.x；且看到边界后 y 不再
/// 抖动（connector 收尾与 Boundary 首帧连续）。
#[test]
fn coordinated_single_glyph_wrap_keeps_caret_vertically_centered_on_boundary() {
    run_on_qt_thread(|| {
        let (mut item, _start_y, _line2_y, line2_caret_top) =
            build_single_glyph_wrap_for_test();
        let mut saw_boundary = false;
        let mut previous_boundary_y: Option<f64> = None;
        for elapsed in (0..=160).step_by(10) {
            rewind_coordinated_clock_for_test(&mut item, elapsed);
            let now = Instant::now();
            let coord = item.pipeline.animation_coordinator_mut();
            let Some(sample) = coord.sample_edit_frontier(now) else {
                continue;
            };
            let Some((_, distance)) = coord.coordinated_distance_for_test(now) else {
                continue;
            };
            let Some((is_boundary, caret_x, caret_y, _frozen)) =
                coord.coordinated_segment_at_test(distance)
            else {
                continue;
            };
            if !is_boundary {
                continue;
            }
            saw_boundary = true;
            assert!(
                (caret_y - line2_caret_top).abs() < 1e-6,
                "{}ms：Boundary 期间 caret.y 必须是第二行真实 caret top {}，实际 {}",
                elapsed,
                line2_caret_top,
                caret_y
            );
            if let Some(prev) = previous_boundary_y {
                assert!(
                    (caret_y - prev).abs() < 1e-6,
                    "{}ms：进入 Boundary 后 y 不得再抖动（前后 {} vs {}）",
                    elapsed,
                    prev,
                    caret_y
                );
            }
            previous_boundary_y = Some(caret_y);
            let frontier = coord.active_edit_frontier.as_ref().expect("前沿");
            let masks = frontier.hidden_new_text_rects(&sample);
            if let Some(mask) = masks.first() {
                assert!(
                    (mask.x - caret_x).abs() < 1.0,
                    "{}ms：Boundary 内 Reveal 边界 x 必须等于 caret.x，实际 {} vs {}",
                    elapsed,
                    mask.x,
                    caret_x
                );
            }
        }
        assert!(saw_boundary, "必须观察到 Boundary 帧");
        println!("[BEHAVIOR_VERIFY] 评论42②：单字软换行 caret 垂直居中于真实 caret top");
    });
}

/// Issue #826 评论 42：`coordinated_cross_line_boundary_does_not_add_tail_vertical_correction`。
///
/// target 就在最后一个 Boundary 出口时，末段 Boundary 的 y 已是 target caret top，
/// 不应再追加「visual_line_top -> target_y」的竖向尾 connector。断言 motion 的
/// 最后一个分段是 Boundary，且其 y_to == target_y。
#[test]
fn coordinated_cross_line_boundary_does_not_add_tail_vertical_correction() {
    run_on_qt_thread(|| {
        let (item, _start_y, _line2_y, line2_caret_top) = build_single_glyph_wrap_for_test();
        let coord = item.pipeline.animation_coordinator();
        let (_target_x, target_y, _duration, _total) = coord
            .coordinated_caret_for_test()
            .expect("协同 motion 必须存在");
        assert!(
            (target_y - line2_caret_top).abs() < 1e-6,
            "前置：target_y 必须是第二行真实 caret top {}，实际 {}",
            line2_caret_top,
            target_y
        );
        let kinds = coord.coordinated_segment_kinds_for_test();
        let (is_boundary, _y_from, y_to) = *kinds.last().expect("必须有分段");
        assert!(
            is_boundary,
            "末段必须是 Boundary（不得追加只为 caret_top 校正的竖向尾 connector），\
             实际 kinds={:?}",
            kinds
        );
        assert!(
            (y_to - target_y).abs() < 1e-6,
            "末段 Boundary 的 y_to 必须已是 target caret top {}，实际 {}",
            target_y,
            y_to
        );
        println!("[BEHAVIOR_VERIFY] 评论42③：跨行 Boundary 不追加竖向尾校正");
    });
}

/// Issue #826 评论 43 辅助：构造一条 synthetic 视觉行。
///
/// `h` 用 `visual_line_bottom - visual_line_top` 控制（评论 43 的 bug 需要
/// `h` ≈ 行距，才能让相邻两行的 y 命中带重叠）；`caret_top` 独立于
/// `visual_line_top`。
fn test_line(
    id: LineSnapshotId,
    top: f64,
    h: f64,
    caret_top: f64,
    byte_start: usize,
    byte_end: usize,
    x: f64,
    w: f64,
) -> PreparedLineSnapshot {
    PreparedLineSnapshot {
        id,
        image: None,
        clusters: vec![LineClusterSnapshot {
            byte_start,
            byte_end,
            source_rect: SourceRect {
                x,
                y: 0.0,
                w,
                h: 20.0,
            },
            shaping_identity: test_shaping_identity(),
        }],
        document_origin_y: top,
        dpr: 1.0,
        byte_start,
        byte_end,
        visual_x: x,
        visual_line_top: top,
        visual_line_bottom: top + h,
        caret_top,
        caret_height: 20.0,
    }
}

fn two_line_snapshot(lines: Vec<PreparedLineSnapshot>) -> EditorLayoutSnapshot {
    EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        lines,
        None,
        None,
        CaretAffinity::Downstream,
    )
}

/// Issue #826 评论 43：`coordinated_retarget_uses_frontier_travelled_not_caret_y_band`。
///
/// synthetic 两行：line1 caret_top=5/h=30，line2 caret_top=35/h=30，两行 x span
/// 都 10..100。旧算法用 `[caret_top, caret_top+h]` 命中，start_y=35 会同时落在
/// line1 的 `[5,35]`（+1 容差）里 → 误认成 line1。新算法按
/// `frontier.reveal.travelled`（已越过 line1）精确裁掉 line1。
#[test]
fn coordinated_retarget_uses_frontier_travelled_not_caret_y_band() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let t0 = Instant::now();
        let snapshot = two_line_snapshot(vec![
            test_line(LineSnapshotId::new(0, 0, 0), 5.0, 30.0, 5.0, 0, 1, 10.0, 90.0),
            test_line(LineSnapshotId::new(0, 0, 1), 35.0, 30.0, 35.0, 1, 2, 10.0, 90.0),
        ]);
        let mut frontier = EditFrontierState::begin_insert(
            String::new(),
            snapshot,
            String::from("xy"),
            vec![(0, 2)],
            OffsetMap::from_single_edit(0, (0, 0), 0),
            Vec::new(),
            t0,
            160,
        );
        // 旧前沿已经越过 line1（travelled = line1 段长 90）。
        assert_eq!(frontier.reveal.regions.len(), 1, "两条线合成一条 region（同 range）");
        frontier.reveal.travelled = 90.0;
        let coord = item.pipeline.animation_coordinator_mut();
        coord.active_edit_frontier = Some(frontier);
        // 第一笔建 motion（fresh，不裁）。
        assert!(coord.begin_or_retarget_coordinated_caret(0.0, 5.0, 50.0, 5.0, t0));
        // 第二笔 retarget：start_y=35 与 line1 命中带重叠的经典情形。
        let t1 = t0 + Duration::from_millis(20);
        assert!(coord.begin_or_retarget_coordinated_caret(50.0, 35.0, 60.0, 35.0, t1));
        let boundaries = coord.coordinated_boundary_segments_for_test();
        assert!(
            !boundaries.is_empty(),
            "retarget 后必须有文字段，实际 {:?}",
            boundaries
        );
        for b in &boundaries {
            assert!(
                (b.2 - 35.0).abs() < 1e-6 && (b.3 - 35.0).abs() < 1e-6,
                "retarget 必须从 line2(caret_top=35) 开始，不得命中 line1，实际段 y=({}, {})",
                b.2,
                b.3
            );
        }
        println!("[BEHAVIOR_VERIFY] 评论43②：retarget 按 frontier.travelled 裁，而非 caret y-band");
    });
}

/// Issue #826 评论 43：`reveal_boundary_caret_top_prefers_exact_line_id`。
///
/// 两份 snapshot 行 y 相同/接近时，Reveal 段必须按 `segment.line_id` 精确找
/// target line 的 caret_top，不能靠 first-y-match。
#[test]
fn reveal_boundary_caret_top_prefers_exact_line_id() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let t0 = Instant::now();
        // 两行 y 相同（视觉上不可能，但正好压测 first-y-match 的歧义），
        // caret_top 不同：line_id 精确匹配才能各取各的。
        let snapshot = two_line_snapshot(vec![
            test_line(LineSnapshotId::new(0, 0, 0), 5.0, 20.0, 5.0, 0, 1, 10.0, 40.0),
            test_line(LineSnapshotId::new(0, 0, 1), 5.0, 20.0, 9.0, 1, 2, 50.0, 40.0),
        ]);
        let frontier = EditFrontierState::begin_insert(
            String::new(),
            snapshot,
            String::from("xy"),
            vec![(0, 2)],
            OffsetMap::from_single_edit(0, (0, 0), 0),
            Vec::new(),
            t0,
            160,
        );
        let coord = item.pipeline.animation_coordinator_mut();
        coord.active_edit_frontier = Some(frontier);
        assert!(coord.begin_or_retarget_coordinated_caret(10.0, 5.0, 60.0, 5.0, t0));
        let boundaries = coord.coordinated_boundary_segments_for_test();
        let ys: Vec<f64> = boundaries.iter().map(|b| b.2).collect();
        assert!(
            ys.iter().any(|y| (y - 5.0).abs() < 1e-6)
                && ys.iter().any(|y| (y - 9.0).abs() < 1e-6),
            "两段必须按 line_id 各取自己的 caret_top（5 与 9），first-y-match 会让两段都得 5，实际 {:?}",
            ys
        );
        println!("[BEHAVIOR_VERIFY] 评论43③：Reveal 段按 line_id 精确取 caret_top");
    });
}

/// Issue #826 评论 43：`rapid_wrap_retarget_does_not_match_previous_line_when_caret_tops_are_one_line_height_apart`。
///
/// 真实 Qt layout：第一笔多字插入跨软换行（Reveal 有 line1 + line2 两条段），
/// 让 motion 进入第二行；紧接着再输入一个字 retarget。断言 retarget 后不再有
/// line1 的 Boundary（不得把 caret 带回上一行），且第一帧 (x,y) 与旧 motion
/// 连续。
#[test]
fn rapid_wrap_retarget_does_not_match_previous_line_when_caret_tops_are_one_line_height_apart() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from(
            "界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界界",
        ));
        let line1_end = first_visual_line_byte_end(&item);
        let insert_at = line1_end - 15;
        let _ = item.pipeline.set_selection(insert_at, insert_at);
        item.snap_next_cursor_update();
        // 12 个新字把 line1 尾 5 字 + 自己挤到 line2 → Reveal 跨两行两条段。
        item.insert_text(QString::from("界界界界界界界界界界界界"));
        {
            let boundaries = item
                .pipeline
                .animation_coordinator()
                .coordinated_boundary_segments_for_test();
            assert!(
                boundaries.len() >= 2,
                "前置：跨软换行插入必须有 >=2 条文字段，实际 {}",
                boundaries.len()
            );
        }
        let line1_caret_top = item
            .pipeline
            .animation_coordinator()
            .coordinated_boundary_segments_for_test()
            .first()
            .map(|b| b.2)
            .expect("line1 boundary");
        // 播到 120ms（已进入第二行 Boundary）。
        rewind_coordinated_clock_for_test(&mut item, 120);
        let retarget_ready = {
            let coord = item.pipeline.animation_coordinator();
            coord.active_coordinated_caret.is_some()
                && coord
                    .coordinated_boundary_segments_for_test()
                    .len()
                    >= 2
        };
        assert!(retarget_ready, "前置：旧 motion 必须还在");

        // 第二笔：同一 burst 再输入一个字 → retarget。
        let retarget_at = {
            item.insert_text(QString::from("界"));
            item.pipeline
                .animation_coordinator()
                .active_coordinated_caret
                .as_ref()
                .expect("retarget 后 motion 必须还在")
                .started_at
        };
        let coord = item.pipeline.animation_coordinator();
        let boundaries = coord.coordinated_boundary_segments_for_test();
        assert!(!boundaries.is_empty(), "retarget 后必须有文字段");
        // 根因断言：第一条 Boundary 不能是 line1（不得把 caret 带回上一行）。
        assert!(
            (boundaries[0].2 - line1_caret_top).abs() > 1.0,
            "retarget 后首个 Boundary 必须是当前行（非 line1 caret_top={}），实际 {}",
            line1_caret_top,
            boundaries[0].2
        );
        // 首帧连续性：新 motion 在 retarget 时刻的 caret 必须还在第二行。
        let sample = coord
            .active_coordinated_caret
            .as_ref()
            .expect("motion 必须还在")
            .sample_at_distance(
                coord
                    .active_coordinated_caret
                    .as_ref()
                    .unwrap()
                    .distance_at_progress(
                        coord
                            .active_coordinated_caret
                            .as_ref()
                            .unwrap()
                            .sample_progress(retarget_at),
                    ),
            );
        assert!(
            (sample.1 - line1_caret_top).abs() > 1.0,
            "retarget 首帧 y 不得跳回 line1 caret_top={}，实际 {}",
            line1_caret_top,
            sample.1
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "协同 retarget 不得孵化独立 Tween"
        );
        println!("[BEHAVIOR_VERIFY] 评论43①：快速换行 retarget 不回上一行");
    });
}

/// Issue #826 评论 44 BLOCKER 1：
/// `coordinated_partial_trim_preserves_global_frontier_distance_on_first_frame`。
///
/// 按 `travelled` 从 Boundary 中间裁段后，段的 `frontier_distance_from` 必须同步
/// 推进到 `start_frontier_distance`，否则第二笔首帧 `sample_at_distance(0)` 返回的
/// 前沿距离仍是旧 from（0），Reveal mask 把已吐出的字瞬间吞回（快速同线连打回弹）。
#[test]
fn coordinated_partial_trim_preserves_global_frontier_distance_on_first_frame() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let t0 = Instant::now();
        // 单行一段：cluster x=10..30（visual_length 20），frontier_distance_from=0。
        let line = test_line(LineSnapshotId::new(0, 0, 0), 5.0, 20.0, 5.0, 0, 1, 10.0, 20.0);
        let frontier = EditFrontierState::begin_insert(
            String::new(),
            two_line_snapshot(vec![line]),
            String::from("x"),
            vec![(0, 1)],
            OffsetMap::from_single_edit(0, (0, 0), 0),
            Vec::new(),
            t0,
            160,
        );
        let coord = item.pipeline.animation_coordinator_mut();
        coord.active_edit_frontier = Some(frontier);
        // 先建一份 motion，再把它 retarget 到 travelled=8 的当前前沿。
        assert!(coord.begin_or_retarget_coordinated_caret(0.0, 5.0, 40.0, 5.0, t0));
        coord.active_edit_frontier.as_mut().unwrap().reveal.travelled = 8.0;
        assert!(coord.begin_or_retarget_coordinated_caret(
            18.0,
            5.0,
            45.0,
            5.0,
            t0 + Duration::from_millis(10)
        ));

        let boundaries = coord.coordinated_boundary_segments_for_test();
        assert_eq!(boundaries.len(), 1, "只有一条 Boundary");
        let b = boundaries[0];
        assert!(
            b.0 > 10.0 + 0.5 && b.0 < 30.0 - 0.5,
            "第一段 x_from 必须从原段中间裁起（10..30 内非端点），实际 {}",
            b.0
        );
        assert!(
            (b.5 - 8.0).abs() < 1e-6,
            "裁段后 frontier_distance_from 必须是 start_frontier_distance=8，实际 {}",
            b.5
        );
        let at0 = coord
            .active_coordinated_caret
            .as_ref()
            .unwrap()
            .sample_at_distance(0.0);
        assert_eq!(
            at0.2,
            Some(8.0),
            "第二笔首帧 sample_at_distance(0).frontier_distance 必须是 8，绝不能退回 0"
        );
        println!("[BEHAVIOR_VERIFY] 评论44①：partial trim 同步推进 frontier_distance_from");
    });
}

/// Issue #826 评论 44 BLOCKER 2：
/// `coordinated_target_one_line_below_is_not_same_row_even_when_delta_equals_line_height`。
///
/// 末段 target 的 same_row 判定不能再拿整行 height 当容差：下一行 caret_top 与末段
/// 相差约一行高度，会被误判成同行，导致整段留在上一行、最后一帧才 Snap 跳行。
#[test]
fn coordinated_target_one_line_below_is_not_same_row_even_when_delta_equals_line_height() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        let t0 = Instant::now();
        // 单行 Boundary：caret_top=10，h=30（行高整块）。target_y=40 = 正好下一行。
        let line = test_line(LineSnapshotId::new(0, 0, 0), 10.0, 30.0, 10.0, 0, 1, 10.0, 20.0);
        let frontier = EditFrontierState::begin_insert(
            String::new(),
            two_line_snapshot(vec![line]),
            String::from("x"),
            vec![(0, 1)],
            OffsetMap::from_single_edit(0, (0, 0), 0),
            Vec::new(),
            t0,
            160,
        );
        let coord = item.pipeline.animation_coordinator_mut();
        coord.active_edit_frontier = Some(frontier);
        // target_y=40（= 末段 caret_top 10 + 行高 30），target_x 在末段 x span 内。
        assert!(coord.begin_or_retarget_coordinated_caret(20.0, 10.0, 20.0, 40.0, t0));

        let kinds = coord.coordinated_segment_kinds_for_test();
        let (is_boundary, _y_from, y_to) = *kinds.last().expect("必须有分段");
        assert!(
            !is_boundary,
            "target 在下一行时必须追加 Connector（不得把末段 Boundary 直接改写成 target_x），\
             实际 kinds={:?}",
            kinds
        );
        assert!(
            (y_to - 40.0).abs() < 1e-6,
            "尾 Connector 必须走到 target_y=40，实际 {}",
            y_to
        );
        // progress<1 时 y 已经开始向下一行移动（不是靠终点 Snap）。
        let m = coord.active_coordinated_caret.as_ref().unwrap();
        let mid = m.sample_at_distance(m.total_length * 0.9);
        assert!(
            mid.1 > 10.5,
            "progress<1 时 caret.y 必须已在向下一行推进（不是终点才跳），实际 {}",
            mid.1
        );
        println!("[BEHAVIOR_VERIFY] 评论44②：下一行不被误判为同行，追加跨行 Connector");
    });
}

/// Issue #826 评论 44：`rapid_same_line_typing_does_not_rehide_partially_revealed_previous_glyph`。
///
/// 同一行快速连打：第一字吐出一部分后立刻输入第二字，第二笔第一帧的 Reveal 边界
/// 距离不得小于第一笔当前可见边界（不得把已吐出的部分吞回去）。
#[test]
fn rapid_same_line_typing_does_not_rehide_partially_revealed_previous_glyph() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.current_viewport_height = 600.0;
        item.current_coordinated_animation_enabled = true;
        item.current_typing_animation_enabled = true;
        item.current_smooth_cursor_enabled = true;
        item.pipeline.set_typing_animation_duration_ms(160);
        item.set_plain_text(QString::from("A"));
        let _ = item.pipeline.set_selection(1, 1);
        item.snap_next_cursor_update();
        item.insert_text(QString::from("X"));

        // 播到 50ms，记第一笔当前可见前沿距离。
        rewind_coordinated_clock_for_test(&mut item, 50);
        let now_old = Instant::now();
        let old_distance = item
            .pipeline
            .animation_coordinator_mut()
            .sample_edit_frontier(now_old)
            .expect("第一笔前沿必须还在")
            .coordinated
            .expect("协同必须带边界")
            .reveal_distance;
        assert!(old_distance > 0.0, "前置：第一笔必须已吐出一点，实际 {}", old_distance);

        // 第二笔马上输入（同 burst，extend + retarget）。
        item.insert_text(QString::from("Y"));
        assert_eq!(item.pipeline.committed_text(), "AXY");
        let coord = item.pipeline.animation_coordinator_mut();
        // 在 retarget 时刻采第二笔首帧边界。
        let retarget_at = coord
            .active_coordinated_caret
            .as_ref()
            .expect("第二笔 motion 必须还在")
            .started_at;
        let new_distance = coord
            .sample_edit_frontier(retarget_at)
            .expect("前沿必须还在")
            .coordinated
            .expect("协同必须带边界")
            .reveal_distance;
        assert!(
            new_distance >= old_distance - 1e-6,
            "第二笔首帧 Reveal 边界不得回退：旧可见 {}，新 {}",
            old_distance,
            new_distance
        );
        assert!(
            item.cursor_ctrl.animation.is_none(),
            "协同连打不得孵化独立 Tween"
        );
        println!("[BEHAVIOR_VERIFY] 评论44：同行快速连打不回吞已吐出的字");
    });
}
