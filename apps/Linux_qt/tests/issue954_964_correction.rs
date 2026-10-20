//! Issue #954 / #964 white-box QML wiring guards.
//!
//! The QML components need the full Kirigami/qrc runtime, so these tests assert
//! the source-level contract that keeps correction requests edit-driven and
//! keeps navigation/actions out of the offset-diagnostic UI.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn linux_qt_root() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

#[test]
fn edits_are_forwarded_instead_of_cursor_sentence_scans() {
    let controller = read_src("qml/EditorController.qml");
    assert!(controller.contains("apply_correction_edits("));
    assert!(controller.contains("request_correction_pending("));
    assert!(controller
        .contains("function reconcileCorrectionEdit(baseRevision, newRevision, patchRangesJson)"));
    assert!(
        !controller.contains("correctionSegmentDebounceTimer"),
        "issue #954 removed the cursor-driven debounce scan"
    );
    assert!(
        !controller.contains("correctionSentenceRange"),
        "cursor movement must never infer an edit"
    );
    assert!(
        !controller.contains("scheduleCorrectionSegment()"),
        "editing must use the transaction's patch ranges"
    );
}

#[test]
fn chapter_open_restores_cache_and_manual_check_remains() {
    let controller = read_src("qml/EditorController.qml");
    assert!(controller.contains("requestCorrectionSnapshot();"));
    assert!(controller.contains("function requestCorrectionChapter()"));
    assert!(controller.contains("function jumpToCorrection(suggestion)"));
    assert!(controller.contains("targetEditorItem.reveal_correction("));
    assert!(controller.contains("editorViewportRef.revealCurrentCaret()"));
}

#[test]
fn correction_panel_has_compact_actions_and_no_offsets() {
    let panel = read_src("qml/CorrectionPanel.qml");
    assert!(panel.contains("jumpToCorrection(modelData)"));
    assert!(panel.contains("qsTr(\"建议：%1\")"));
    assert!(panel.contains("Menu {"));
    assert!(panel.contains("忽略一次"));
    assert!(panel.contains("忽略本章"));
    assert!(panel.contains("加入作品词典"));
    assert!(panel.contains("停用此规则"));
    assert!(
        !panel.contains("UTF-16") && !panel.contains("byte ["),
        "internal offsets must not be exposed"
    );
    assert_eq!(
        panel.matches("text: qsTr(\"替换\")").count(),
        1,
        "one primary replace button only"
    );
}

#[test]
fn viewport_reveals_and_workspace_injects_it() {
    let viewport = read_src("qml/EditorViewport.qml");
    assert!(viewport.contains("function revealCurrentCaret()"));
    let workspace = read_src("qml/WritingWorkspace.qml");
    assert!(workspace.contains("editorViewportRef: editorViewport"));
}
