// =============================================================================
// EditorController.qml — 编辑器逻辑控制器
// =============================================================================
//
// 层级：Desktop UI 层（QML 逻辑控制器）
// 职责：章节打开/保存、自动保存、格式应用、保存守卫（防误触）
// 约束：
//   - 不包含 UI 渲染，只管理编辑器状态
//   - 通过 backendRef 调用 AppBackend (Rust QObject)
//   - 自研编辑器主流程：SujianEditorItem + QTextLayout + EditorAnimationOverlay
//   - TextArea / DocumentHandler 仅作为 fallback 兼容路径，不参与自研编辑器
//
// 关键流程（自研编辑器主路径）：
//   openChapter() → read_chapter → SujianEditorItem setText
//   saveCurrentChapter() → get_plain_text → sanitize → save_chapter
//
// 防死循环机制：
//   isLoadingChapter 标记防止 chapter_path_changed 信号重入
// =============================================================================

import QtQuick
import QtQuick.Controls
import Sujian 1.0

QtObject {
    id: controller

    signal emptySaveBlocked(string message)

    // Target UI bindings
    property var targetEditorItem: null
    property var targetTextArea: null
    property bool useSelfRenderedEditor: false
    property var backendRef: null
    property var dt: null

    // Chapter state — single source of truth, updated only after successful load
    property string projectId: ""
    property string volumeId: ""
    property string chapterId: ""
    property string chapterTitle: ""
    property string saveStatus: backendRef ? backendRef.save_status : ""

    // Internal state
    property bool isLoadingChapter: false
    property bool isApplyingFormat: false
    property bool isApplyingSettings: false
    property bool pendingAutoSaveAfterGuard: false
    property bool explicitEmptySavePending: false
    property double lastPotentialExplicitClearAtMs: 0
    property string previousEditorText: ""
    property string lastSavedEditorText: ""

    // Autosave timer
    property var autoSaveTimer: Timer {
        interval: settingsBackend ? settingsBackend.setting_auto_save_delay_ms : 1500
        repeat: false
        onTriggered: {
            if (!backendRef || !controller.chapterId || !controller.projectId || !controller.volumeId) return;
            if (!settingsBackend || !settingsBackend.setting_auto_save_enabled) return;
            if (controller.saveGuardActive()) {
                controller.pendingAutoSaveAfterGuard = true;
                return;
            }
            controller.saveCurrentChapter();
        }
    }

    property var settingsGuardTimer: Timer {
        interval: 300
        repeat: false
        onTriggered: {
            controller.isApplyingSettings = false;
            if (!controller.pendingAutoSaveAfterGuard) return;
            controller.pendingAutoSaveAfterGuard = false;
            if (!settingsBackend || !settingsBackend.setting_auto_save_enabled) return;
            if (!controller.chapterId || !controller.projectId || !controller.volumeId) return;

            var read = controller.readEditorPlainText();
            if (read.suspiciousEmpty) {
                controller.blockUnsafeEmptySave("settings_guard_autosave", read);
                return;
            }
            if (read.text === controller.lastSavedEditorText) return;
            controller.autoSaveTimer.restart();
        }
    }

    function colorToHex(colorValue, fallback) {
        if (colorValue === undefined || colorValue === null) return fallback;
        if (typeof colorValue === "string") return colorValue;
        var r = Math.round(Math.max(0, Math.min(1, colorValue.r)) * 255);
        var g = Math.round(Math.max(0, Math.min(1, colorValue.g)) * 255);
        var b = Math.round(Math.max(0, Math.min(1, colorValue.b)) * 255);
        function componentHex(value) {
            var hex = value.toString(16).toUpperCase();
            return hex.length === 1 ? "0" + hex : hex;
        }
        return "#" + componentHex(r) + componentHex(g) + componentHex(b);
    }

    function logRenderColorProbe(reason) {
        if (!backendRef || !backendRef.log_qml) return;

        var themeMode = "<unset>";
        try {
            themeMode = appState && appState.settings ? appState.settings.themeMode : "<unset>";
        } catch (e) {
            themeMode = "<unavailable>";
        }

        var isDark = dt ? dt.isDark : "<no-dt>";
        var editorText = dt ? String(dt.editorText) : "<no-dt>";
        var convertedEditorText = dt ? controller.colorToHex(dt.editorText, "#E2E2E5") : "<no-dt>";
        var handlerColor = docHandler ? docHandler.text_color : "<no-docHandler>";
        backendRef.log_qml("info", "editor", "theme_color_probe",
                           "reason=" + reason
                           + " themeMode=" + themeMode
                           + " designTokens.isDark=" + isDark
                           + " designTokens.editorText=" + editorText
                           + " colorToHex(editorText)=" + convertedEditorText
                           + " docHandler.text_color=" + handlerColor);
    }

    property DocumentHandler docHandler: DocumentHandler {
        id: docHandler
        document: (!useSelfRenderedEditor && targetTextArea) ? targetTextArea.textDocument : null
        line_spacing: settingsBackend ? settingsBackend.setting_line_spacing : 1.5
        text_indent: (settingsBackend && settingsBackend.setting_auto_indent_enabled) ? Math.max(Math.round((settingsBackend.setting_font_size || 16) * 2), 28) : 0
        text_color: dt ? controller.colorToHex(dt.editorText, "#E2E2E5") : "#E2E2E5"
    }

    // Stats + word count debounce timer — batches per-keystroke FFI calls.
    // process_writing_event_from_text does text diff + stats recording (expensive),
    // calculate_word_count scans full text. Both are imperceptible at 300ms延迟.
    property var statsTimer: Timer {
        interval: 300
        repeat: false
        onTriggered: {
            if (!controller.chapterId || !controller.backendRef) return;
            var plainText = controller.getEditorPlainText();
            controller.reportStatsIfChanged(plainText);
            controller.backendRef.calculate_word_count(plainText);
        }
    }

    // NOTE: sync is NOT triggered by save. Auto-sync runs on workspace open
    // and foreground return only (see main.qml workspaceOpenAutoSyncTimer,
    // foregroundAutoSyncTimer). This is intentional: save ≠ sync.

    // Connections to TextArea signals
    property var textConnections: Connections {
        target: (!controller.useSelfRenderedEditor && targetTextArea) ? targetTextArea : null
        function onTextChanged() {
            controller.handlePlainTextChanged("text_changed");
        }
    }

    property var editorItemConnections: Connections {
        target: (controller.useSelfRenderedEditor && targetEditorItem) ? targetEditorItem : null
        function onText_changed() {
            controller.handlePlainTextChanged("sujian_editor_text_changed");
            Qt.callLater(targetEditorItem.flush_content_height);
        }
    }

    // Safe plain text extraction via QTextDocument::toPlainText() in Rust.
    // Never returns HTML regardless of textFormat setting.
    function getEditorPlainText() {
        return readEditorPlainText().text;
    }

    function normalizePlainText(text) {
        if (text === undefined || text === null) return "";
        return String(text).replace(/\u2029/g, "\n").replace(/\r\n/g, "\n").replace(/\r/g, "\n");
    }

    function readTextAreaPlainText() {
        if (!targetTextArea) return "";

        var text = "";
        try {
            var len = targetTextArea.length || 0;
            if (len > 0 && targetTextArea.getText) {
                text = targetTextArea.getText(0, len);
            }
        } catch (e) {
            logWriterWarning("text_area_get_text_failed", "error=" + e);
        }

        if ((!text || text.length === 0) && targetTextArea.text && targetTextArea.text.length > 0) {
            text = targetTextArea.text;
        }

        return normalizePlainText(text);
    }

    function readEditorItemPlainText() {
        if (!targetEditorItem) return "";
        try {
            if (targetEditorItem.get_plain_text) {
                return normalizePlainText(targetEditorItem.get_plain_text());
            }
            if (targetEditorItem.plain_text !== undefined) {
                return normalizePlainText(targetEditorItem.plain_text);
            }
        } catch (e) {
            logWriterWarning("editor_item_get_plain_text_failed", "error=" + e);
        }
        return "";
    }

    function readEditorPlainText() {
        if (useSelfRenderedEditor && targetEditorItem) {
            var editorItemText = readEditorItemPlainText();
            // The guard protects persisted content. Transient text that was never
            // saved must not make undo-back-to-empty look like a destructive save.
            var hadKnownEditorItemContent = lastSavedEditorText.length > 0;
            if (editorItemText.length === 0 && hadKnownEditorItemContent && hasRecentExplicitClearCandidate()) {
                explicitEmptySavePending = true;
            }
            return {
                "text": editorItemText,
                "docLength": -1,
                "textAreaLength": -1,
                "editorItemLength": editorItemText.length,
                "usedFallback": false,
                "suspiciousEmpty": editorItemText.length === 0 && hadKnownEditorItemContent && !explicitEmptySavePending
            };
        }

        var docText = "";
        try {
            docText = normalizePlainText(docHandler.get_plain_text());
        } catch (e) {
            logWriterWarning("doc_get_plain_text_failed", "error=" + e);
        }

        var textAreaText = readTextAreaPlainText();
        var text = docText;
        var usedFallback = false;
        if (docText.length === 0 && textAreaText.length > 0) {
            text = textAreaText;
            usedFallback = true;
            logWriterWarning("doc_empty_textarea_fallback", "textAreaLen=" + textAreaText.length);
        }

        // Only persisted non-empty content is dangerous to overwrite with an
        // unexpected empty read. Unsaved transient text can legitimately undo to empty.
        var hadKnownContent = lastSavedEditorText.length > 0;
        if (text.length === 0 && hadKnownContent && hasRecentExplicitClearCandidate()) {
            explicitEmptySavePending = true;
        }

        var suspiciousEmpty = text.length === 0 && hadKnownContent && !explicitEmptySavePending;
        return {
            "text": text,
            "docLength": docText.length,
            "textAreaLength": textAreaText.length,
            "editorItemLength": -1,
            "usedFallback": usedFallback,
            "suspiciousEmpty": suspiciousEmpty
        };
    }

    function saveGuardActive() {
        return isLoadingChapter || isApplyingFormat || isApplyingSettings || (!useSelfRenderedEditor && docHandler && docHandler.visual_format_mutating);
    }

    function hasRecentExplicitClearCandidate() {
        if (useSelfRenderedEditor && targetEditorItem) {
            return readEditorItemPlainText().length === 0 && (Date.now() - lastPotentialExplicitClearAtMs) < 2000;
        }
        if (!targetTextArea || !targetTextArea.activeFocus || (targetTextArea.length || 0) !== 0) return false;
        return (Date.now() - lastPotentialExplicitClearAtMs) < 2000;
    }

    function markPotentialExplicitClear() {
        if (useSelfRenderedEditor && targetEditorItem) {
            if (saveGuardActive()) return;
            var editorTextLen = readEditorItemPlainText().length;
            var hasSelection = targetEditorItem.has_selection === true;
            if (editorTextLen <= 1 || hasSelection) {
                lastPotentialExplicitClearAtMs = Date.now();
            }
            return;
        }
        if (!targetTextArea || !targetTextArea.activeFocus || saveGuardActive()) return;
        var currentLen = targetTextArea.length || 0;
        var selectedLen = targetTextArea.selectedText ? targetTextArea.selectedText.length : 0;
        if (currentLen <= 1 || (selectedLen > 0 && selectedLen >= currentLen)) {
            lastPotentialExplicitClearAtMs = Date.now();
        }
    }

    function logWriterWarning(event, message) {
        var msg = message || "";
        console.warn("[SujianDebug][WARN][qml][module=editor][event=" + event + "] " + msg);
        if (backendRef && backendRef.log_qml) {
            backendRef.log_qml("warn", "editor", event, msg);
        }
    }

    function blockUnsafeEmptySave(reason, read) {
        var details = "reason=" + reason
                + ", previousLen=" + previousEditorText.length
                + ", lastSavedLen=" + lastSavedEditorText.length
                + ", docLen=" + (read ? read.docLength : -1)
                + ", textAreaLen=" + (read ? read.textAreaLength : -1)
                + ", editorItemLen=" + (read ? read.editorItemLength : -1);
        logWriterWarning("empty_save_blocked", details);
        if (backendRef) {
            backendRef.save_status = qsTr("已阻止空内容保存");
        }
        autoSaveTimer.stop();
        pendingAutoSaveAfterGuard = false;
        return false;
    }

    function handlePlainTextChanged(reason) {
        if (controller.saveGuardActive()) return;

        var read = controller.readEditorPlainText();
        if (read.suspiciousEmpty) {
            controller.blockUnsafeEmptySave(reason, read);
            return;
        }

        var plainText = read.text;
        if (plainText.length > 0) {
            controller.explicitEmptySavePending = false;
        }
        // Debounce stats + word count — batch into 300ms timer instead of per-keystroke FFI
        controller.statsTimer.restart();
        if (controller.chapterId && controller.backendRef) {
            if (settingsBackend && settingsBackend.setting_auto_save_enabled) {
                controller.autoSaveTimer.restart();
            }
        }
    }

    // Unified save entry — always writes normalized plain text via backend.
    function saveCurrentChapter() {
        if (!backendRef || !chapterId || !projectId || !volumeId) return;
        if (saveGuardActive()) {
            pendingAutoSaveAfterGuard = true;
            return false;
        }

        var read = readEditorPlainText();
        if (read.suspiciousEmpty) {
            return blockUnsafeEmptySave("save_current_chapter", read);
        }

        var plainText = read.text;
        if (plainText === lastSavedEditorText) return true;
        var allowEmptyOverwrite = plainText.length === 0 && explicitEmptySavePending;

        var result = backendRef.save_chapter(projectId, volumeId, chapterId, plainText, allowEmptyOverwrite);
        if (result && result.success) {
            lastSavedEditorText = plainText;
            previousEditorText = plainText;
            if (plainText.length === 0) {
                explicitEmptySavePending = false;
                lastPotentialExplicitClearAtMs = 0;
            }
            return true;
        } else {
            if (result && result.errorCode === "EMPTY_OVERWRITE_BLOCKED") {
                logWriterWarning("empty_save_blocked", "blocked by core: " + (result.errorCode || ""));
                controller.emptySaveBlocked(qsTr("检测到异常空内容覆盖，已阻止保存。"));
            }
            return false;
        }
    }

    // Load chapter: content comes from Rust core as plain text.
    // Returns the full result object so caller can update state.
    function loadChapterContentWithIds(pId, vId, cId) {
        if (!cId || !pId || !vId || !backendRef) return null;
        if (useSelfRenderedEditor && !targetEditorItem) return null;
        if (!useSelfRenderedEditor && !targetTextArea) return null;
        if (isLoadingChapter) return null;

        isLoadingChapter = true;

        var result = backendRef.open_chapter(pId, vId, cId);

        if (!result.success) {
            console.error("[SujianDebug] Failed to open chapter:", result.errorCode || result.rawError);
            isLoadingChapter = false;
            return null;
        }

        var content = normalizePlainText(result.data ? result.data.content || "" : "");

        if (useSelfRenderedEditor && targetEditorItem) {
            var isReload = (cId === controller.chapterId && pId === controller.projectId);
            if (isReload) {
                targetEditorItem.reload_plain_text(content);
            } else {
                targetEditorItem.set_plain_text(content);
                targetEditorItem.clear_undo_stack();
            }
        } else {
            // TextArea fallback owns plain text; DocumentHandler owns display format only.
            targetTextArea.textFormat = TextEdit.PlainText;
            targetTextArea.text = content;
            logRenderColorProbe("open_chapter_before_apply_format");
            docHandler.apply_format();
            docHandler.clear_undo_stack();
        }

        previousEditorText = content;
        lastSavedEditorText = content;
        explicitEmptySavePending = false;
        lastPotentialExplicitClearAtMs = 0;
        isLoadingChapter = false;

        // Return full result so caller updates chapter state from authoritative source
        return result;
    }

    function reportStatsIfChanged(currentText) {
        if (isLoadingChapter || !chapterId || !backendRef) return;
        var newText = currentText === undefined ? getEditorPlainText() : currentText;
        if (previousEditorText === newText) return;

        backendRef.process_writing_event_from_text(projectId, volumeId, chapterId, previousEditorText, newText);

        previousEditorText = newText;
    }

    function applyCurrentSettings() {
        isApplyingSettings = true;
        if (autoSaveTimer.running) {
            pendingAutoSaveAfterGuard = true;
            autoSaveTimer.stop();
        }
        if (!useSelfRenderedEditor && docHandler) {
            logRenderColorProbe("apply_settings_before_apply_format");
            docHandler.apply_format();
        }
        settingsGuardTimer.restart();
    }

    function formatText() {
        if (useSelfRenderedEditor && !targetEditorItem) return;
        if (!useSelfRenderedEditor && !targetTextArea) return;
        var read = readEditorPlainText();
        if (read.suspiciousEmpty) {
            blockUnsafeEmptySave("format_text_read", read);
            return;
        }

        var raw = read.text;
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

        isApplyingFormat = true;
        autoSaveTimer.stop();
        try {
            if (useSelfRenderedEditor && targetEditorItem) {
                targetEditorItem.reload_plain_text(plain);
                targetEditorItem.clear_undo_stack();
            } else {
                var cursor = targetTextArea.cursorPosition;
                targetTextArea.textFormat = TextEdit.PlainText;
                targetTextArea.text = plain;
                logRenderColorProbe("format_text_before_apply_format");
                docHandler.apply_format();
                targetTextArea.cursorPosition = Math.min(cursor, targetTextArea.length);
            }
        } finally {
            isApplyingFormat = false;
        }

        saveCurrentChapter();
    }
}
