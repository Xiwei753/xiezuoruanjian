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
use std::time::{Duration, Instant};

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
