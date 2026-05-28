import QtQuick 2.15
import QtQuick.Controls 2.15
import Writer 1.0

QtObject {
    id: controller

    // Target UI bindings
    property var targetTextArea: null
    property var backendRef: null

    // Chapter state — single source of truth, updated only after successful load
    property string projectId: ""
    property string volumeId: ""
    property string chapterId: ""
    property string chapterTitle: ""
    property string saveStatus: backendRef ? backendRef.save_status : ""

    // Internal state
    property bool isLoadingChapter: false
    property string previousEditorText: ""

    // Autosave timer
    property var autoSaveTimer: Timer {
        interval: backendRef ? backendRef.setting_auto_save_delay_ms : 1500
        repeat: false
        onTriggered: {
            if (!backendRef || !controller.chapterId || !controller.projectId || !controller.volumeId) return;
            if (!backendRef.setting_auto_save_enabled) return;
            controller.saveCurrentChapter();
        }
    }

    property DocumentHandler docHandler: DocumentHandler {
        id: docHandler
        document: targetTextArea ? targetTextArea.textDocument : null
        line_spacing: backendRef ? backendRef.setting_line_spacing : 1.5
        text_indent: (backendRef && backendRef.setting_auto_indent_enabled) ? Math.round((backendRef.setting_font_size || 16) * 2) : 0
    }

    // Autosync timer
    property var autoSyncTimer: Timer {
        interval: 20000
        repeat: false
        onTriggered: {
            if (!backendRef || !backendRef.has_workspace) return;
            if (!backendRef.sync_auto_sync || !backendRef.sync_enabled) return;
            backendRef.request_auto_sync("auto_sync_after_save");
        }
    }

    // Connections to TextArea signals
    property var textConnections: Connections {
        target: targetTextArea
        function onTextChanged() {
            if (controller.isLoadingChapter) return;
            var plainText = controller.getEditorPlainText();
            controller.reportStatsIfChanged();
            if (controller.chapterId && controller.backendRef) {
                controller.backendRef.calculate_word_count(plainText);
                if (controller.backendRef.setting_auto_save_enabled) {
                    controller.autoSaveTimer.restart();
                }
            }
        }
    }

    // Safe plain text extraction via QTextDocument::toPlainText() in Rust.
    // Never returns HTML regardless of textFormat setting.
    function getEditorPlainText() {
        return docHandler.get_plain_text();
    }

    // Unified save entry — always writes plain text via backend.
    // No HTML sanitization needed: get_plain_text() already guarantees pure text.
    function saveCurrentChapter() {
        if (!backendRef || !chapterId || !projectId || !volumeId) return;
        var plainText = getEditorPlainText();
        backendRef.save_chapter(projectId, volumeId, chapterId, plainText);
    }

    // Load chapter: content comes from Rust core as plain text.
    // Returns the full result object so caller can update state.
    function loadChapterContentWithIds(pId, vId, cId) {
        if (!cId || !pId || !vId || !backendRef || !targetTextArea) return null;
        if (isLoadingChapter) return null;

        isLoadingChapter = true;

        var resultJson = backendRef.open_chapter_json(pId, vId, cId);
        var result = JSON.parse(resultJson);

        if (!result.success) {
            console.error("[WriterDebug] Failed to open chapter:", result.error);
            isLoadingChapter = false;
            return null;
        }

        var content = result.content || "";

        // Use PlainText mode: line spacing/indent are applied via QTextDocument
        // block formats in DocumentHandler, not via HTML.
        targetTextArea.textFormat = TextEdit.PlainText;
        docHandler.set_plain_text(content);
        docHandler.clear_undo_stack();

        previousEditorText = getEditorPlainText();
        isLoadingChapter = false;

        // Return full result so caller updates chapter state from authoritative source
        return result;
    }

    function computeWordCount(text) {
        if (!text) return 0;
        return text.replace(/\s/g, '').length;
    }

    function reportStatsIfChanged() {
        if (isLoadingChapter || !chapterId || !backendRef) return;
        var newText = getEditorPlainText();
        if (previousEditorText === newText) return;

        var oldLen = previousEditorText.length;
        var newLen = newText.length;
        var diff = newLen - oldLen;

        if (diff > 0) {
            var source = "human_typed";
            var inserted = diff;
            var deleted = 0;
            var pasted = 0;
            if (diff > 20) {
                source = "pasted";
                pasted = diff;
                inserted = 0;
            }
            backendRef.report_writing_event(projectId, volumeId, chapterId, source, inserted, deleted, pasted);
        } else if (diff < 0) {
            backendRef.report_writing_event(projectId, volumeId, chapterId, "deleted", 0, Math.abs(diff), 0);
        }

        previousEditorText = newText;
    }

    function applyCurrentSettings() {
        // Property bindings on docHandler already handle line_spacing and text_indent updates.
        // Font size is bound directly on the TextArea via backendRef.setting_font_size.
        // No manual action needed.
    }

    function formatText() {
        if (!targetTextArea) return;
        var raw = getEditorPlainText();
        if (!raw) return;
        var paragraphs = raw.split(/\r?\n|\u2029/);
        var formatted = [];
        for (var i = 0; i < paragraphs.length; i++) {
            var p = paragraphs[i];
            // Only strip leading full-width spaces, tabs, non-breaking spaces
            p = p.replace(/^[ \t\u3000\u00a0]+/, "");
            // Only strip trailing horizontal whitespace, preserve empty lines
            if (p.length > 0) {
                p = p.replace(/[ \t]+$/, "");
            }
            formatted.push(p);
        }

        var plain = formatted.join("\n");

        isLoadingChapter = true;
        var cursor = targetTextArea.cursorPosition;
        docHandler.set_plain_text(plain);

        targetTextArea.cursorPosition = Math.min(cursor, targetTextArea.length);
        isLoadingChapter = false;

        if (backendRef && chapterId && projectId && volumeId) {
            backendRef.save_chapter(projectId, volumeId, chapterId, plain);
        }
    }
}
