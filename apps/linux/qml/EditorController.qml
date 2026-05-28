import QtQuick 2.15
import QtQuick.Controls 2.15
import Writer 1.0

QtObject {
    id: controller

    // Target UI bindings
    property var targetTextArea: null
    property var backendRef: null
    property string projectId: ""
    property string volumeId: ""
    property string chapterId: ""
    property string saveStatus: backendRef ? backendRef.save_status : ""

    // Logical States
    property bool isLoadingChapter: false
    property string previousEditorText: ""

    // Autosave timer
    property var autoSaveTimer: Timer {
        interval: backendRef ? backendRef.setting_auto_save_delay_ms : 1500
        repeat: false
        onTriggered: {
            if (!backendRef || !chapterId || !projectId || !volumeId) return;
            if (!backendRef.setting_auto_save_enabled) return;
            backendRef.save_chapter(projectId, volumeId, chapterId, controller.getEditorPlainText());
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

    function getEditorPlainText() {
        if (!targetTextArea) return "";
        var txt = targetTextArea.getText(0, targetTextArea.length);
        return txt.replace(/\u2029/g, "\n");
    }

    // Settings applied automatically via property bindings on docHandler

    function loadChapterContentWithIds(pId, vId, cId) {
        if (!cId || !pId || !vId || !backendRef || !targetTextArea) return false;
        if (isLoadingChapter) return false;
        
        isLoadingChapter = true;
        
        var resultJson = backendRef.open_chapter_json(pId, vId, cId);
        var result = JSON.parse(resultJson);
        
        if (!result.success) {
            console.error("Failed to open chapter:", result.error);
            isLoadingChapter = false;
            return false;
        }
        
        var content = result.content;
        
        targetTextArea.textFormat = TextEdit.RichText;
        docHandler.set_plain_text(content);
        docHandler.clear_undo_stack();
        
        previousEditorText = content;
        
        isLoadingChapter = false;
        return true;
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
        // No manual action needed — changing backend settings triggers binding updates.
    }

    function formatText() {
        if (!targetTextArea) return;
        var raw = getEditorPlainText();
        if (!raw) return;
        var paragraphs = raw.split(/\r?\n|\u2029/);
        var formatted = [];
        for (var i = 0; i < paragraphs.length; i++) {
            var p = paragraphs[i].trim();
            if (p.length > 0) {
                p = p.replace(/^[ \t\u3000\u00a0]+/, "");
                formatted.push(p);
            } else {
                formatted.push("");
            }
        }
        var finalParagraphs = [];
        var lastWasEmpty = false;
        for (var j = 0; j < formatted.length; j++) {
            if (formatted[j] === "") {
                if (!lastWasEmpty) {
                    finalParagraphs.push("");
                    lastWasEmpty = true;
                }
            } else {
                finalParagraphs.push(formatted[j]);
                lastWasEmpty = false;
            }
        }
        
        var plain = finalParagraphs.join("\n");
        isLoadingChapter = true;
        var cursor = targetTextArea.cursorPosition;
        targetTextArea.textFormat = TextEdit.RichText;
        docHandler.set_plain_text(plain);
        
        targetTextArea.cursorPosition = Math.min(cursor, targetTextArea.length);
        isLoadingChapter = false;
        
        if (backendRef && chapterId && projectId && volumeId) {
            backendRef.save_chapter(projectId, volumeId, chapterId, plain);
        }
    }
}
