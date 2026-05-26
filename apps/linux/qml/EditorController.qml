import QtQuick 2.15
import QtQuick.Controls 2.15

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
            if (!backendRef || !chapterId) return;
            if (!backendRef.setting_auto_save_enabled) return;
            backendRef.save_current_chapter(controller.getEditorPlainText());
        }
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
        if (targetTextArea.textFormat === TextEdit.RichText) {
            return targetTextArea.getText(0, targetTextArea.length);
        }
        return targetTextArea.text;
    }

    function loadChapterContent() {
        if (!chapterId || !backendRef || !targetTextArea) return;
        isLoadingChapter = true;
        var content = backendRef.get_chapter_content(projectId, volumeId, chapterId);
        
        if (backendRef.setting_auto_indent_enabled) {
            var paragraphs = content.split("\n");
            var html = "";
            for (var i = 0; i < paragraphs.length; i++) {
                var p = paragraphs[i];
                p = p.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
                html += "<p style='text-indent: 2em; margin-top: 0; margin-bottom: 8px;'>" + p + "</p>";
            }
            targetTextArea.textFormat = TextEdit.RichText;
            targetTextArea.text = html;
        } else {
            targetTextArea.textFormat = TextEdit.PlainText;
            targetTextArea.text = content;
        }
        
        previousEditorText = content;
        isLoadingChapter = false;
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

    function formatText() {
        if (!targetTextArea) return;
        var raw = getEditorPlainText();
        if (!raw) return;
        var paragraphs = raw.split(/\n/);
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
        if (backendRef && backendRef.setting_auto_indent_enabled) {
            var html = "";
            for (var k = 0; k < finalParagraphs.length; k++) {
                var line = finalParagraphs[k];
                line = line.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
                html += "<p style='text-indent: 2em; margin-top: 0; margin-bottom: 8px;'>" + line + "</p>";
            }
            targetTextArea.textFormat = TextEdit.RichText;
            targetTextArea.text = html;
        } else {
            targetTextArea.textFormat = TextEdit.PlainText;
            targetTextArea.text = plain;
        }
        
        if (backendRef && chapterId) {
            backendRef.save_current_chapter(plain);
        }
    }
}
