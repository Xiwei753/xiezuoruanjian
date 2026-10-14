//! Issue #819 中仍有效的 edit outcome 和左键手势守卫。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/source_guard.rs"]
mod source_guard;

use source_guard::{function_window, read_src};

// =========================================================================
// 编辑 outcome 与左键手势
// =========================================================================

// =========================================================================
// 问题 2：match_rebase_frames carried slices — 已修复
// =========================================================================

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

    // 3. #901 重写后 QML 用 onPressAndHold 直接处理左键长按，
    // 不再需要 is_left_button_event 在 qquickitem_impl.rs 中判断。
    assert!(
        qml_src.contains("onPressAndHold"),
        "问题 4 修复：QML 必须有 onPressAndHold 处理左键长按"
    );

    // 4. 确认 QML 调 activate_pointer_long_press。
    assert!(
        qml_src.contains("activate_pointer_long_press"),
        "问题 4 修复：QML onPressAndHold 必须调 activate_pointer_long_press"
    );
}

// =========================================================================
// 问题 5：一帧只采一次 caret — 已修复
// =========================================================================

// =========================================================================
// 问题 6：旧语义注释已清除 — 已修复
// =========================================================================
