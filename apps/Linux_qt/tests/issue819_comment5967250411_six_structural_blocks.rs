//! Issue #819 评论 5967250411 回归守卫 — 6 个结构阻塞已修复。
//!
//! 本测试是 Phase B 修复后的结构守卫：通过 WHITE_BOX 源码审查 + 纯逻辑断言
//! 确认评论指出的 6 个结构阻塞已在代码中消除。测试 PASS = 阻塞已修复。
//!
//! 6 个阻塞的修复：
//! 1. 快速删除"字先没、光标后走"：SampledSliceFrame 增加 is_finished 字段，
//!    rebase.rs 用 is_finished 做终态过滤，不再拿 visible_fraction 猜 CaretTrack。
//! 2. match_rebase_frames carried slices：RebaseVisualState 增加 carried_slices 字段，
//!    take_rebase_frames 收集未终态 slice，transaction_builder 消费。
//! 3. Applied -> Created / Skipped：pipeline edit 方法返回 PipelineEditOutcome，
//!    edit_flow 只对 Applied 进视觉流水线，写 editor.edit.applied 诊断事件。
//! 4. 左键手势单一 owner：删除 leftButtonLongPressHandler (TapHandler)，改用
//!    Timer + Rust property，qquickitem_impl mouse_event 只接左键。
//! 5. 一帧只采一次 caret：render_plan_builder 用 sample_transaction_visual_state_with_caret
//!    传入已采好的 caret，不再调 sample_transaction_visual_state 重复采 track。
//! 6. 旧语义注释清除：WritingWorkspace.qml 和 coordinator.rs 旧语义注释已更新。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

// =========================================================================
// 问题 1：快速删除"字先没、光标后走" — 已修复
// =========================================================================

#[test]
fn issue819_block1_caret_track_uses_is_finished_not_visible_fraction() {
    let frame_state_src = read_src("src/sujian_editor_item/animation/frame_state.rs");
    // 1. 确认 SampledSliceFrame 有 is_finished 字段。
    assert!(
        frame_state_src.contains("pub is_finished: bool"),
        "问题 1 修复：SampledSliceFrame 必须有 is_finished 字段"
    );

    // 2. 确认 sample.rs 设置 is_finished（CaretTrack 从 caret.progress 判断）。
    let sample_src = read_src("src/sujian_editor_item/animation/sample.rs");
    assert!(
        sample_src.contains("caret_finished") && sample_src.contains("caret.progress >= 1.0"),
        "问题 1 修复：sample.rs 必须从 caret.progress 设置 CaretTrack unit 的 is_finished"
    );

    // 3. 确认 rebase.rs take_rebase_frames 用 is_finished 做终态过滤，
    //    不再用 visible_fraction <= 1e-3 判 DeleteConceal 终态。
    let rebase_src = read_src("src/sujian_editor_item/animation/rebase.rs");
    let take_rebase = function_window(&rebase_src, "fn take_rebase_frames", 4000);
    assert!(
        take_rebase.contains("is_finished"),
        "问题 1 修复：rebase.rs take_rebase_frames 必须用 is_finished 做终态过滤"
    );
    // DeleteConceal 不再用 visible_fraction <= 1e-3 判终态。
    let still_uses_visible_fraction_for_delete =
        take_rebase.contains("DeleteConceal") && take_rebase.contains("visible_fraction <= 1e-3");
    assert!(
        !still_uses_visible_fraction_for_delete,
        "问题 1 修复：rebase.rs take_rebase_frames 不再用 visible_fraction <= 1e-3 \
         判 DeleteConceal 终态，改用 is_finished"
    );

    // 4. 确认 caret handoff 仍正常构造（光标继续走的证据保留）。
    assert!(
        take_rebase.contains("RebaseCaretHandoff") && take_rebase.contains("caret_handoff"),
        "rebase.rs take_rebase_frames 仍构造 RebaseCaretHandoff（光标 handoff 保留）"
    );
}

// =========================================================================
// 问题 2：match_rebase_frames carried slices — 已修复
// =========================================================================

#[test]
fn issue819_block2_rebase_visual_state_has_carried_units() {
    let rebase_src = read_src("src/sujian_editor_item/animation/rebase.rs");

    // 1. 确认 RebaseVisualState 有 carried_units 字段（携带完整 AnimatedSlice
    //    的未终态旧视觉单元）。
    assert!(
        rebase_src.contains("carried_units"),
        "问题 2 修复：RebaseVisualState 必须有 carried_units 字段"
    );

    // 2. 确认 take_rebase_frames 收集未终态 slice 到 carried_units。
    let take_rebase = function_window(&rebase_src, "fn take_rebase_frames", 4000);
    assert!(
        take_rebase.contains("carried_units"),
        "问题 2 修复：take_rebase_frames 必须收集未终态 slice 到 carried_units"
    );

    // 3. 确认 transaction_builder 消费 carried_units（Issue #824 第 5 节：只保留
    //    本帧可见几何 + 本笔 motion 时长，不带旧 timing/stage/剩余时长）。
    let builder_src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    assert!(
        builder_src.contains("carried_units"),
        "问题 2 修复：transaction_builder 必须消费 carried_units"
    );
    assert!(
        builder_src.contains("detach_caret_track_to_timed("),
        "Issue #824 评论 5971089641 第 5 节 / 评论 5972388049：carried 吞吐字必须从本帧 \
         采样几何继续（无跳变 retarget），走共用的 detach helper，不得重新排队旧 route。"
    );
}

// =========================================================================
// 问题 3：edit_flow 用 PipelineEditOutcome 判 Applied — 已修复
// =========================================================================

#[test]
fn issue819_block3_edit_flow_uses_pipeline_edit_outcome() {
    // 1. 确认 edit_flow.rs 不再用 `let applied = edit_result.is_some();`。
    let edit_flow_src = read_src("src/sujian_editor_item/edit_flow.rs");
    assert!(
        !edit_flow_src.contains("let applied = edit_result.is_some();"),
        "问题 3 修复：edit_flow.rs 不再用 `let applied = edit_result.is_some();` \
         判 Applied，改用 PipelineEditOutcome"
    );

    // 2. 确认 pipeline.rs 有 PipelineEditOutcome 枚举。
    let pipeline_src = read_src("src/sujian_editor_item/pipeline.rs");
    assert!(
        pipeline_src.contains("enum PipelineEditOutcome"),
        "问题 3 修复：pipeline.rs 必须有 PipelineEditOutcome 枚举"
    );
    assert!(
        pipeline_src.contains("PipelineEditOutcome::Applied"),
        "问题 3 修复：PipelineEditOutcome 必须有 Applied 变体"
    );
    assert!(
        pipeline_src.contains("PipelineEditOutcome::NotApplied"),
        "问题 3 修复：PipelineEditOutcome 必须有 NotApplied 变体"
    );

    // 3. 确认 edit_flow.rs 用 PipelineEditOutcome 判 Applied。
    assert!(
        edit_flow_src.contains("PipelineEditOutcome::Applied"),
        "问题 3 修复：edit_flow.rs 必须用 PipelineEditOutcome::Applied 判 Applied"
    );

    // 4. 确认 edit_flow.rs 写 editor.edit.applied 诊断事件。
    assert!(
        edit_flow_src.contains("editor.edit.applied"),
        "问题 3 修复：edit_flow.rs 必须写 editor.edit.applied 诊断事件"
    );
}

// =========================================================================
// 问题 4：左键手势单一 owner — 已修复
// =========================================================================

#[test]
fn issue819_block4_left_button_gesture_single_owner() {
    let qml_src = read_src("qml/WritingWorkspace.qml");

    // 1. 确认 WritingWorkspace.qml 不再有 leftButtonLongPressHandler (TapHandler)。
    assert!(
        !qml_src.contains("id: leftButtonLongPressHandler"),
        "问题 4 修复：WritingWorkspace.qml 不再有 leftButtonLongPressHandler (TapHandler)，\
         左键 pointer event 单一 owner 是 qquickitem_impl mouse_event"
    );

    // 2. 确认 QML 不再调 end_selection_gesture_qml。
    assert!(
        !qml_src.contains("end_selection_gesture_qml"),
        "问题 4 修复：QML 不再调 end_selection_gesture_qml，\
         release/cancel 的 selection gesture 结束只由 qquickitem_impl mouse_event 做一次"
    );

    // 3. 确认 qquickitem_impl.rs mouse_event 的 MouseButtonPress 判断左键按钮。
    let qquick_src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        qquick_src.contains("is_left_button_event"),
        "问题 4 修复：qquickitem_impl mouse_event 必须用 is_left_button_event 判断左键"
    );

    // 4. 确认 QML 有 Timer 绑定 long_press_timer_active。
    assert!(
        qml_src.contains("long_press_timer_active"),
        "问题 4 修复：QML Timer 必须绑定 long_press_timer_active property"
    );
    assert!(
        qml_src.contains("activate_pointer_long_press"),
        "问题 4 修复：QML Timer 到点必须调 activate_pointer_long_press"
    );
}

// =========================================================================
// 问题 5：一帧只采一次 caret — 已修复
// =========================================================================

#[test]
fn issue819_block5_render_path_samples_caret_once_per_frame() {
    let rpb_src = read_src("src/sujian_editor_item/animation/render_plan_builder.rs");

    // 1. 确认 render_plan_builder 用 sample_transaction_visual_state_with_caret。
    assert!(
        rpb_src.contains("sample_transaction_visual_state_with_caret"),
        "问题 5 修复：render_plan_builder 必须用 sample_transaction_visual_state_with_caret \
         传入已采好的 caret"
    );

    // 2. 确认 build_text_animation_plan_with_sample 不再调 sample_transaction_visual_state
    //    （改用 _with_caret 版本）。
    let bt_idx = rpb_src
        .find("fn build_text_animation_plan_with_sample")
        .expect("build_text_animation_plan_with_sample 必须存在");
    let build_text = &rpb_src[bt_idx..bt_idx + 5000.min(rpb_src.len() - bt_idx)];
    assert!(
        !build_text.contains("sample_transaction_visual_state("),
        "问题 5 修复：build_text_animation_plan_with_sample 不再调 \
         sample_transaction_visual_state（改用 _with_caret 版本），一帧只采一次 caret"
    );

    // 3. 确认 sample.rs 有 sample_transaction_visual_state_with_caret 函数。
    let sample_src = read_src("src/sujian_editor_item/animation/sample.rs");
    assert!(
        sample_src.contains("fn sample_transaction_visual_state_with_caret"),
        "问题 5 修复：sample.rs 必须有 sample_transaction_visual_state_with_caret 函数"
    );
}

// =========================================================================
// 问题 6：旧语义注释已清除 — 已修复
// =========================================================================

#[test]
fn issue819_block6_stale_semantics_comments_removed() {
    // 1. 确认 WritingWorkspace.qml 不再有旧语义注释。
    let qml_src = read_src("qml/WritingWorkspace.qml");
    assert!(
        !qml_src.contains("协同只表示同事务/同首帧/同 rebase，不共享 duration。"),
        "问题 6 修复：WritingWorkspace.qml 不再写着\
         '协同只表示同事务/同首帧/同 rebase，不共享 duration。'（旧语义注释已清除）"
    );

    // 2. 确认 coordinator.rs reconcile_active_transactions_with_canonical 不再有"独立时间线"。
    let coord_src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let reconcile = function_window(
        &coord_src,
        "fn reconcile_active_transactions_with_canonical",
        2500,
    );
    assert!(
        !reconcile.contains("独立时间线"),
        "问题 6 修复：reconcile_active_transactions_with_canonical 不再有'独立时间线'\
         注释（CaretTrack 没有独立时间线）"
    );
    assert!(
        !reconcile.contains("旧文字按自己当前帧做 rebase")
            && !reconcile.contains("旧文字动画按自己当前帧做 rebase"),
        "问题 6 修复：coordinator.rs 不再有'旧文字按自己当前帧做 rebase'注释"
    );
}
