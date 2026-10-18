// =============================================================================
// EditorController.qml — 编辑器逻辑控制器
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 逻辑控制器）
// 职责：章节打开/保存、自动保存、格式应用、保存守卫（防误触）
// 约束：
//   - 不包含 UI 渲染，只管理编辑器状态
//   - 通过 editorBackendRef 调用 AppBackend (Rust QObject)
//   - 唯一编辑器路径：SujianEditorItem + QTextLayout + Rust Coordinator → Scene Graph
//
// 关键流程：
//   openChapter() → read_chapter → SujianEditorItem setText
//   saveCurrentChapter() → get_plain_text → sanitize → save_chapter
//
// 防死循环机制：
//   isLoadingChapter 标记防止 chapter_path_changed 信号重入
// =============================================================================

import QtQuick
import QtQuick.Controls

QtObject {
    id: controller

    signal emptySaveBlocked(string message)

    // Target UI bindings
    property var targetEditorItem: null
    property var editorBackendRef: null
    required property var dt
    // Issue #709 评论 issue-body-709: 主题诊断需要读取 ThemeController runtime state
    //（appearance_mode/is_dark/color_source），由 WritingWorkspace 注入。
    property var themeControllerRef: null

    // Issue #933：设置后端由 WritingWorkspace 显式注入，不再依赖全局 context property。
    property var settingsBackendRef: null

    // Chapter state — single source of truth, updated only after successful load
    property string projectId: ""
    property string volumeId: ""
    property string chapterId: ""
    property string chapterTitle: ""
    property string saveStatus: editorBackendRef ? editorBackendRef.save_status : ""

    // Internal state
    property bool isLoadingChapter: false
    property bool isApplyingFormat: false
    // Issue #721: 改名为 settingsSaveGuardActive，收窄语义为纯保存 guard。
    // 不再传成 SujianEditorItem.is_applying_settings 的编辑器视觉抑制状态。
    property bool settingsSaveGuardActive: false
    property bool pendingAutoSaveAfterGuard: false
    property bool explicitEmptySavePending: false
    property double lastPotentialExplicitClearAtMs: 0
    property string previousEditorText: ""
    property string lastSavedEditorText: ""

    // Issue #943：proofreading results are revision-scoped annotations only.
    // The editor item remains the sole owner of editable text and transactions.
    property var correctionSuggestions: []
    property string correctionError: ""
    property string correctionStatus: ""
    property string correctionDictionaryError: ""
    property var correctionDictionaryResources: []
    property bool correctionCheckPending: false
    property int correctionRequestSerial: 0
    property int pendingCorrectionRequestId: 0
    property double pendingCorrectionRevision: -1
    property string pendingCorrectionProjectId: ""
    property string pendingCorrectionChapterId: ""
    property string pendingCorrectionText: ""

    // Autosave timer
    property var autoSaveTimer: Timer {
        interval: controller.settingsBackendRef ? controller.settingsBackendRef.setting_auto_save_delay_ms : 1500
        repeat: false
        onTriggered: {
            if (!editorBackendRef || !controller.chapterId || !controller.projectId || !controller.volumeId) return;
            if (!controller.settingsBackendRef || !controller.settingsBackendRef.setting_auto_save_enabled) return;
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
            controller.settingsSaveGuardActive = false;
            if (!controller.pendingAutoSaveAfterGuard) return;
            controller.pendingAutoSaveAfterGuard = false;
            if (!controller.settingsBackendRef || !controller.settingsBackendRef.setting_auto_save_enabled) return;
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
        // Issue #709 评论 issue-body-709: 不再读旧 appState.settings.themeMode。
        // 只读 ThemeController runtime state 和 designTokens。同一条诊断写出
        // appearance_mode/is_dark/color_source/primary/surface/on_surface/
        // on_surface_variant，便于定位深色模式文字仍为黑色的问题。
        if (!editorBackendRef || !editorBackendRef.log_qml) return;
        var tc = themeControllerRef ? themeControllerRef : null;
        var appearanceMode = tc ? tc.appearance_mode : "<null>";
        var isDark = dt ? dt.isDark : "<no-dt>";
        var colorSource = tc ? tc.color_source : "<null>";
        var editorText = dt ? String(dt.editorText) : "<no-dt>";
        var convertedEditorText = dt ? controller.colorToHex(dt.editorText, dt.textPrimary) : "<no-dt>";
        var primary = dt ? String(dt.primary) : "<no-dt>";
        var surface = dt ? String(dt.surface) : "<no-dt>";
        var onSurface = dt ? String(dt.onSurface) : "<no-dt>";
        var onSurfaceVariant = dt ? String(dt.onSurfaceVariant) : "<no-dt>";
        editorBackendRef.log_qml("info", "editor", "theme_color_probe",
                           "reason=" + reason
                           + " appearance_mode=" + appearanceMode
                           + " isDark=" + isDark
                           + " color_source=" + colorSource
                           + " primary=" + primary
                           + " surface=" + surface
                           + " on_surface=" + onSurface
                           + " on_surface_variant=" + onSurfaceVariant
                           + " designTokens.editorText=" + editorText
                           + " colorToHex(editorText)=" + convertedEditorText);
    }

    // Stats + word count debounce timer — batches per-keystroke FFI calls.
    // Issue #843: statsTimer 只负责 calculate_word_count（全文扫描）。
    // 编辑事务统计（cause + inserted/deleted）由 editor_change_applied signal
    // 直接从 Rust EditorEditResult 发出，不再走 300ms 文本 diff。
    property var statsTimer: Timer {
        interval: 300
        repeat: false
        onTriggered: {
            if (!controller.chapterId || !controller.editorBackendRef) return;
            var plainText = controller.getEditorPlainText();
            controller.editorBackendRef.calculate_word_count(plainText);
        }
    }

    // Issue #943：输入停顿后仅检查光标所在句子，Core 会为语言模型补充邻近上下文。
    property var correctionSegmentDebounceTimer: Timer {
        interval: 500
        repeat: false
        onTriggered: controller.requestCorrectionSegment()
    }

    // NOTE: sync is NOT triggered by save. Auto-sync has been removed
    // (Issue #729). This is intentional: save ≠ sync.

    property var editorItemConnections: Connections {
        target: targetEditorItem ? targetEditorItem : null
        function onText_changed() {
            controller.invalidateCorrectionResults();
            controller.scheduleCorrectionSegment();
            controller.handlePlainTextChanged("sujian_editor_text_changed");
            Qt.callLater(targetEditorItem.flush_content_height);
        }
        // Issue #843: 编辑事务事实从 Rust EditorEditResult 直接传来，
        // cause + inserted/deleted 不再在 QML 中用文本长度 diff 猜。
        function onEditor_change_applied(cause, insertedChars, deletedChars) {
            if (!controller.chapterId || !controller.editorBackendRef) return;
            controller.editorBackendRef.record_editor_change_stats(
                controller.projectId, controller.volumeId, controller.chapterId,
                cause, insertedChars, deletedChars);
        }
    }

    property var correctionBackendConnections: Connections {
        target: editorBackendRef ? editorBackendRef : null
        function onCorrection_check_ready(requestId, textRevision, suggestionsJson) {
            controller.receiveCorrectionSuggestions(requestId, textRevision, suggestionsJson);
        }
        function onCorrection_check_failed(requestId, textRevision, message) {
            controller.receiveCorrectionFailure(requestId, textRevision, message);
        }
    }

    // Safe plain text extraction via SujianEditorItem in Rust.
    // Never returns HTML regardless of textFormat setting.
    function getEditorPlainText() {
        return readEditorPlainText().text;
    }

    function normalizePlainText(text) {
        if (text === undefined || text === null) return "";
        return String(text).replace(/\u2029/g, "\n").replace(/\r\n/g, "\n").replace(/\r/g, "\n");
    }

    function invalidateCorrectionResults() {
        correctionSegmentDebounceTimer.stop();
        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        pendingCorrectionRevision = -1;
        pendingCorrectionProjectId = "";
        pendingCorrectionChapterId = "";
        pendingCorrectionText = "";
        correctionSuggestions = [];
        correctionCheckPending = false;
        correctionError = "";
        correctionStatus = "";
    }

    function requestCorrectionChapter() {
        if (!editorBackendRef || !targetEditorItem || !projectId || !chapterId) {
            correctionError = qsTr("请先打开一个章节。");
            return;
        }
        if (!editorBackendRef.request_correction_chapter) {
            correctionError = qsTr("当前版本不支持校对。");
            return;
        }

        correctionSegmentDebounceTimer.stop();

        var snapshotText = getEditorPlainText();
        var revision = Number(targetEditorItem.text_revision);
        if (!isFinite(revision) || revision < 0) {
            correctionError = qsTr("无法读取正文版本。");
            return;
        }
        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        pendingCorrectionRevision = revision;
        pendingCorrectionProjectId = projectId;
        pendingCorrectionChapterId = chapterId;
        pendingCorrectionText = snapshotText;
        correctionSuggestions = [];
        correctionError = "";
        correctionStatus = qsTr("正在校对本章…");
        correctionCheckPending = true;
        editorBackendRef.request_correction_chapter(
            pendingCorrectionRequestId,
            projectId,
            chapterId,
            revision,
            snapshotText);
    }

    // SujianEditorItem exposes its logical caret as a Unicode scalar count;
    // QML string indices are UTF-16 code units. Convert through the same text
    // snapshot before deriving any Core byte offsets.
    function correctionUtf16OffsetForCharIndex(text, characterIndex) {
        var target = Number(characterIndex);
        if (!isFinite(target) || Math.floor(target) !== target || target < 0) {
            throw new Error("无法读取当前光标位置");
        }
        var textIndex = 0;
        var count = 0;
        while (count < target && textIndex < text.length) {
            var first = text.charCodeAt(textIndex);
            if (first >= 0xD800 && first <= 0xDBFF) {
                if (textIndex + 1 >= text.length) throw new Error("正文包含无效 UTF-16 代理项");
                var second = text.charCodeAt(textIndex + 1);
                if (second < 0xDC00 || second > 0xDFFF) throw new Error("正文包含无效 UTF-16 代理项");
                textIndex += 2;
            } else {
                if (first >= 0xDC00 && first <= 0xDFFF) throw new Error("正文包含无效 UTF-16 代理项");
                textIndex += 1;
            }
            count += 1;
        }
        if (count !== target) throw new Error("光标位置超出正文");
        return textIndex;
    }

    function correctionUtf8ByteOffset(text, utf16Offset) {
        var target = Number(utf16Offset);
        if (!isFinite(target) || Math.floor(target) !== target || target < 0 || target > text.length) {
            throw new Error("校对句子位置无效");
        }
        var textIndex = 0;
        var byteOffset = 0;
        while (textIndex < target) {
            var first = text.charCodeAt(textIndex);
            var codePoint = first;
            var units = 1;
            if (first >= 0xD800 && first <= 0xDBFF) {
                if (textIndex + 1 >= text.length) throw new Error("正文包含无效 UTF-16 代理项");
                var second = text.charCodeAt(textIndex + 1);
                if (second < 0xDC00 || second > 0xDFFF) throw new Error("正文包含无效 UTF-16 代理项");
                codePoint = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                units = 2;
            } else if (first >= 0xDC00 && first <= 0xDFFF) {
                throw new Error("正文包含无效 UTF-16 代理项");
            }
            if (textIndex + units > target) throw new Error("校对位置落在 UTF-16 代理项中间");
            byteOffset += codePoint <= 0x7F ? 1
                        : codePoint <= 0x7FF ? 2
                        : codePoint <= 0xFFFF ? 3 : 4;
            textIndex += units;
        }
        if (textIndex !== target) throw new Error("校对位置不是字符边界");
        return byteOffset;
    }

    function correctionSentenceRange(text, cursorUtf16) {
        var separators = ["。", "！", "？", "!", "?", ".", ";", "；", "…", "\n", "\r", "\u2028", "\u2029"];
        var closingMarks = ["”", "’", "」", "』", "》", "】", ")", "）", "\"", "'"];
        var rangeCursor = cursorUtf16;
        var finishedSentenceEnd = -1;
        var trailingIndex = cursorUtf16;
        while (trailingIndex > 0 && closingMarks.indexOf(text.charAt(trailingIndex - 1)) >= 0) {
            trailingIndex -= 1;
        }
        if (trailingIndex > 0 && separators.indexOf(text.charAt(trailingIndex - 1)) >= 0) {
            rangeCursor = trailingIndex - 1;
            finishedSentenceEnd = cursorUtf16;
        }

        var previousBoundary = -1;
        var nextBoundary = text.length;
        if (rangeCursor > 0) {
            for (var leftIndex = 0; leftIndex < separators.length; leftIndex += 1) {
                previousBoundary = Math.max(
                    previousBoundary,
                    text.lastIndexOf(separators[leftIndex], rangeCursor - 1));
            }
        }
        if (finishedSentenceEnd >= 0) {
            nextBoundary = rangeCursor;
        } else {
            for (var rightIndex = 0; rightIndex < separators.length; rightIndex += 1) {
                var found = text.indexOf(separators[rightIndex], cursorUtf16);
                if (found >= 0) nextBoundary = Math.min(nextBoundary, found);
            }
        }
        return {
            startUtf16: previousBoundary + 1,
            endUtf16: finishedSentenceEnd >= 0
                    ? finishedSentenceEnd
                    : nextBoundary < text.length ? nextBoundary + 1 : text.length
        };
    }

    function scheduleCorrectionSegment() {
        if (!editorBackendRef || !targetEditorItem || !projectId || !chapterId) return;
        if (isLoadingChapter || isApplyingFormat || saveGuardActive()) return;
        correctionSegmentDebounceTimer.restart();
    }

    function requestCorrectionSegment() {
        if (!editorBackendRef || !targetEditorItem || !projectId || !chapterId) return;
        if (!editorBackendRef.request_correction_segment) {
            correctionError = qsTr("当前版本不支持实时校对。");
            return;
        }
        if (isLoadingChapter || isApplyingFormat || saveGuardActive()) return;

        var snapshotText = getEditorPlainText();
        if (snapshotText.length === 0) return;
        var revision = Number(targetEditorItem.text_revision);
        if (!isFinite(revision) || revision < 0) {
            correctionError = qsTr("无法读取正文版本。");
            return;
        }

        try {
            var cursorUtf16 = correctionUtf16OffsetForCharIndex(
                snapshotText, targetEditorItem.cursor_position);
            var sentence = correctionSentenceRange(snapshotText, cursorUtf16);
            var startByte = correctionUtf8ByteOffset(snapshotText, sentence.startUtf16);
            var endByte = correctionUtf8ByteOffset(snapshotText, sentence.endUtf16);
            if (startByte >= endByte) return;

            correctionRequestSerial += 1;
            if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
            pendingCorrectionRequestId = correctionRequestSerial;
            pendingCorrectionRevision = revision;
            pendingCorrectionProjectId = projectId;
            pendingCorrectionChapterId = chapterId;
            pendingCorrectionText = snapshotText;
            correctionSuggestions = [];
            correctionError = "";
            correctionStatus = qsTr("正在检查当前句子…");
            correctionCheckPending = true;
            editorBackendRef.request_correction_segment(
                pendingCorrectionRequestId,
                projectId,
                chapterId,
                revision,
                snapshotText,
                startByte,
                endByte);
        } catch (error) {
            correctionCheckPending = false;
            correctionSuggestions = [];
            correctionError = String(error);
            correctionStatus = "";
        }
    }

    function correctionResponseIsCurrent(requestId, textRevision) {
        if (requestId !== pendingCorrectionRequestId) return false;
        if (textRevision !== pendingCorrectionRevision) return false;
        if (projectId !== pendingCorrectionProjectId || chapterId !== pendingCorrectionChapterId) return false;
        if (!targetEditorItem || Number(targetEditorItem.text_revision) !== textRevision) return false;
        if (getEditorPlainText() !== pendingCorrectionText) return false;
        return true;
    }

    // QML QString indices count UTF-16 code units. Convert only the small set
    // of returned byte-range endpoints while walking the immutable snapshot
    // once; do not build a character map or copy a transformed chapter.
    function annotateCorrectionPositions(text, suggestions) {
        var endpoints = [];
        for (var index = 0; index < suggestions.length; index += 1) {
            var startByte = Number(suggestions[index].startByte);
            var endByte = Number(suggestions[index].endByte);
            if (!isFinite(startByte) || !isFinite(endByte)
                    || Math.floor(startByte) !== startByte
                    || Math.floor(endByte) !== endByte
                    || startByte < 0 || endByte < startByte) {
                throw new Error("校对 byte range 无效");
            }
            endpoints.push({ byte: startByte, index: index, edge: "start" });
            endpoints.push({ byte: endByte, index: index, edge: "end" });
        }
        endpoints.sort(function(a, b) { return a.byte - b.byte; });

        var textIndex = 0;
        var byteOffset = 0;
        var utf16Offset = 0;
        var charOffset = 0;
        for (var endpointIndex = 0; endpointIndex < endpoints.length; endpointIndex += 1) {
            var endpoint = endpoints[endpointIndex];
            while (byteOffset < endpoint.byte) {
                if (textIndex >= text.length) throw new Error("校对 byte range 超出正文");
                var first = text.charCodeAt(textIndex);
                var codePoint = first;
                var units = 1;
                if (first >= 0xD800 && first <= 0xDBFF) {
                    if (textIndex + 1 >= text.length) throw new Error("正文包含无效 UTF-16 代理项");
                    var second = text.charCodeAt(textIndex + 1);
                    if (second < 0xDC00 || second > 0xDFFF) throw new Error("正文包含无效 UTF-16 代理项");
                    codePoint = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                    units = 2;
                } else if (first >= 0xDC00 && first <= 0xDFFF) {
                    throw new Error("正文包含无效 UTF-16 代理项");
                }
                var utf8Bytes = codePoint <= 0x7F ? 1
                              : codePoint <= 0x7FF ? 2
                              : codePoint <= 0xFFFF ? 3 : 4;
                byteOffset += utf8Bytes;
                utf16Offset += units;
                charOffset += 1;
                textIndex += units;
            }
            if (byteOffset !== endpoint.byte) throw new Error("校对位置不在 UTF-8 字符边界");
            var suggestion = suggestions[endpoint.index];
            if (endpoint.edge === "start") {
                suggestion.startUtf16 = utf16Offset;
                suggestion.startChar = charOffset;
            } else {
                suggestion.endUtf16 = utf16Offset;
                suggestion.endChar = charOffset;
            }
        }

        for (var suggestionIndex = 0; suggestionIndex < suggestions.length; suggestionIndex += 1) {
            var item = suggestions[suggestionIndex];
            if (text.slice(item.startUtf16, item.endUtf16) !== item.original) {
                throw new Error("校对原文与 byte range 不匹配");
            }
        }
        return suggestions;
    }

    function receiveCorrectionSuggestions(requestId, textRevision, suggestionsJson) {
        if (requestId !== pendingCorrectionRequestId) return;
        correctionCheckPending = false;
        if (!correctionResponseIsCurrent(requestId, textRevision)) {
            invalidateCorrectionResults();
            correctionStatus = qsTr("正文已变化，请重新校对。");
            return;
        }
        try {
            var parsed = JSON.parse(String(suggestionsJson));
            if (!Array.isArray(parsed)) throw new Error("校对结果格式无效");
            for (var i = 0; i < parsed.length; i += 1) {
                var suggestion = parsed[i];
                if (Number(suggestion.textRevision) !== textRevision) throw new Error("校对正文版本无效");
            }
            parsed.sort(function(a, b) { return a.startByte - b.startByte; });
            annotateCorrectionPositions(pendingCorrectionText, parsed);
            correctionSuggestions = parsed;
            correctionStatus = parsed.length > 0
                    ? qsTr("发现 %1 条建议").arg(parsed.length)
                    : qsTr("未发现建议。");
        } catch (error) {
            correctionSuggestions = [];
            correctionError = String(error);
            correctionStatus = "";
        }
    }

    function receiveCorrectionFailure(requestId, textRevision, message) {
        if (requestId !== pendingCorrectionRequestId) return;
        correctionCheckPending = false;
        if (!correctionResponseIsCurrent(requestId, textRevision)) {
            invalidateCorrectionResults();
            correctionStatus = qsTr("正文已变化，请重新校对。");
            return;
        }
        correctionSuggestions = [];
        correctionStatus = "";
        correctionError = String(message || qsTr("校对失败。"));
    }

    function suggestionIsCurrent(suggestion) {
        if (!(targetEditorItem
                && Number(suggestion.textRevision) === Number(targetEditorItem.text_revision)
                && projectId === pendingCorrectionProjectId
                && chapterId === pendingCorrectionChapterId)) return false;
        var currentText = getEditorPlainText();
        return currentText.slice(suggestion.startUtf16, suggestion.endUtf16) === suggestion.original;
    }

    function correctionReason(suggestion) {
        var reasons = {
            "confusion.custom": qsTr("命中作品中的自定义易混词替换。"),
            "proper.similarity": qsTr("与作品中已有专名相似，请核对人名或地名。"),
            "detector.exact": qsTr("词典检测给出了明确替换。"),
            "detector.oov": qsTr("该词未收录在作品词典中，替换后的句子语言模型评分更高。"),
            "detector.char_mad": qsTr("字符组合偏离语言模型习惯，替换后的句子评分更高。"),
            "detector.candidate": qsTr("替换后的句子语言模型评分更高。"),
            "punctuation.repeated": qsTr("发现连续重复标点；这也可能是有意的表达。"),
            "character.repeated": qsTr("发现连续重复汉字；请结合语境确认。")
        };
        var sourceNames = {
            "confusion_word": qsTr("易混词"),
            "proper_name": qsTr("专名相似度"),
            "oov": qsTr("词典与语言模型"),
            "character_ngram": qsTr("字符语言模型"),
            "rule": qsTr("文本规则")
        };
        var reason = reasons[suggestion.ruleId]
                || qsTr("命中规则 %1。需结合上下文判断。 ").arg(suggestion.ruleId);
        var sourceName = sourceNames[suggestion.source] || suggestion.source;
        return qsTr("%1（来源：%2）").arg(reason).arg(sourceName);
    }

    function correctionSeverityLabel(suggestion) {
        var severity = String(suggestion && suggestion.severity || "").toLowerCase();
        if (severity === "error") return qsTr("错误");
        if (severity === "warning") return qsTr("警告");
        return qsTr("提示");
    }

    function correctionSeverityColor(suggestion) {
        var severity = String(suggestion && suggestion.severity || "").toLowerCase();
        if (severity === "error") return "#C52222";
        if (severity === "warning") return "#A65D00";
        return "#777777";
    }

    function acceptCorrection(suggestion) {
        if (!suggestionIsCurrent(suggestion)) {
            invalidateCorrectionResults();
            correctionStatus = qsTr("正文已变化，请重新校对。");
            return false;
        }
        var applied = targetEditorItem.accept_correction(
            suggestion.startByte,
            suggestion.endByte,
            suggestion.original,
            suggestion.replacement,
            suggestion.textRevision);
        if (!applied) {
            invalidateCorrectionResults();
            correctionStatus = qsTr("建议已过期或原文不匹配，请重新校对。");
            return false;
        }
        correctionStatus = qsTr("已应用建议。");
        return true;
    }

    function ignoreCorrection(suggestion, scope) {
        if (!suggestionIsCurrent(suggestion)) {
            invalidateCorrectionResults();
            correctionStatus = qsTr("正文已变化，请重新校对。");
            return false;
        }
        if (!editorBackendRef || !editorBackendRef.ignore_correction) return false;
        var result = editorBackendRef.ignore_correction(
            projectId,
            chapterId,
            suggestion.ruleId,
            suggestion.original,
            scope,
            suggestion.textRevision,
            suggestion.startByte,
            suggestion.endByte);
        if (!result || !result.success) {
            correctionError = result && result.rawError ? result.rawError : qsTr("忽略建议失败。");
            return false;
        }
        var remaining = [];
        for (var i = 0; i < correctionSuggestions.length; i += 1) {
            var item = correctionSuggestions[i];
            var remove = scope === "once"
                    ? item.startByte === suggestion.startByte && item.endByte === suggestion.endByte
                    : scope === "chapter"
                      ? item.ruleId === suggestion.ruleId && item.original === suggestion.original
                      : item.ruleId === suggestion.ruleId;
            if (!remove) {
                remaining.push(item);
            }
        }
        correctionSuggestions = remaining;
        correctionStatus = scope === "once" ? qsTr("已忽略这一次。")
                : scope === "chapter" ? qsTr("已在本章忽略此类建议。")
                : qsTr("已停用此规则。") ;
        return true;
    }

    function addCorrectionWord(suggestion) {
        if (!suggestionIsCurrent(suggestion)) {
            invalidateCorrectionResults();
            correctionStatus = qsTr("正文已变化，请重新校对。");
            return false;
        }
        if (!editorBackendRef || !editorBackendRef.add_correction_dictionary_entry) return false;
        var result = editorBackendRef.add_correction_dictionary_entry(projectId, suggestion.original);
        if (!result || !result.success) {
            correctionError = result && result.rawError ? result.rawError : qsTr("加入作品词典失败。");
            return false;
        }
        var remaining = [];
        for (var i = 0; i < correctionSuggestions.length; i += 1) {
            if (correctionSuggestions[i].original !== suggestion.original) remaining.push(correctionSuggestions[i]);
        }
        correctionSuggestions = remaining;
        correctionError = "";
        correctionStatus = qsTr("已加入作品词典；下次检查会使用该词条。");
        return true;
    }

    function setCorrectionModelPath(path) {
        if (!editorBackendRef || !editorBackendRef.set_correction_model_path) {
            correctionError = qsTr("当前版本不支持配置校对模型。");
            return false;
        }
        var result = editorBackendRef.set_correction_model_path(path);
        if (!result || !result.success) {
            correctionError = result && result.rawError ? result.rawError : qsTr("模型路径设置失败。");
            return false;
        }
        correctionError = "";
        correctionStatus = qsTr("模型路径已保存。");
        return true;
    }

    function chooseCorrectionModelPath() {
        if (!editorBackendRef || !editorBackendRef.pick_correction_model_path) {
            correctionError = qsTr("当前版本不支持选择校对模型。");
            return "";
        }
        var result = editorBackendRef.pick_correction_model_path();
        if (!result || result.cancelled) return "";
        if (!result.success) {
            correctionError = result.rawError || qsTr("读取所选模型路径失败。");
            return "";
        }
        var path = String(result.path || "");
        if (!path) {
            correctionError = qsTr("所选模型路径为空。");
            return "";
        }
        return setCorrectionModelPath(path) ? path : "";
    }

    function refreshCorrectionDictionaryStatus() {
        if (!editorBackendRef || !editorBackendRef.correction_dictionary_status) {
            correctionDictionaryError = qsTr("当前版本不支持读取词库状态。");
            return false;
        }
        var result = editorBackendRef.correction_dictionary_status();
        if (!result || !result.success) {
            correctionDictionaryError = result && result.rawError
                    ? result.rawError : qsTr("读取词库状态失败。");
            return false;
        }
        correctionDictionaryResources = result.resources || [];
        correctionDictionaryError = "";
        return true;
    }

    function importCorrectionDictionary() {
        if (!editorBackendRef || !editorBackendRef.import_correction_dictionary) {
            correctionDictionaryError = qsTr("当前版本不支持导入词库。");
            return false;
        }
        var result = editorBackendRef.import_correction_dictionary();
        if (!result || result.cancelled) return false;
        if (!result.success) {
            correctionDictionaryError = result.rawError || qsTr("导入词库失败。");
            return false;
        }
        correctionDictionaryResources = result.resources || [];
        correctionDictionaryError = "";
        correctionStatus = qsTr("已导入词库：%1").arg(result.fileName || "");
        return true;
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
        var editorItemText = readEditorItemPlainText();
        // The guard protects persisted content. Transient text that was never
        // saved must not make undo-back-to-empty look like a destructive save.
        var hadKnownEditorItemContent = lastSavedEditorText.length > 0;
        if (editorItemText.length === 0 && hadKnownEditorItemContent && hasRecentExplicitClearCandidate()) {
            explicitEmptySavePending = true;
        }
        return {
            "text": editorItemText,
            "editorItemLength": editorItemText.length,
            "suspiciousEmpty": editorItemText.length === 0 && hadKnownEditorItemContent && !explicitEmptySavePending
        };
    }

    function saveGuardActive() {
        return isLoadingChapter || isApplyingFormat || settingsSaveGuardActive;
    }

    function hasRecentExplicitClearCandidate() {
        if (!targetEditorItem) return false;
        return readEditorItemPlainText().length === 0 && (Date.now() - lastPotentialExplicitClearAtMs) < 2000;
    }

    function markPotentialExplicitClear() {
        if (!targetEditorItem || saveGuardActive()) return;
        var editorTextLen = readEditorItemPlainText().length;
        var hasSelection = targetEditorItem.has_selection === true;
        if (editorTextLen <= 1 || hasSelection) {
            lastPotentialExplicitClearAtMs = Date.now();
        }
    }

    function logWriterWarning(event, message) {
        var msg = message || "";
        if (editorBackendRef && editorBackendRef.log_qml) {
            editorBackendRef.log_qml("warn", "editor", event, msg);
        }
    }

    function blockUnsafeEmptySave(reason, read) {
        var details = "reason=" + reason
                + ", previousLen=" + previousEditorText.length
                + ", lastSavedLen=" + lastSavedEditorText.length
                + ", editorItemLen=" + (read ? read.editorItemLength : -1);
        logWriterWarning("empty_save_blocked", details);
        if (editorBackendRef) {
            editorBackendRef.save_status = qsTr("已阻止空内容保存");
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
        if (controller.chapterId && controller.editorBackendRef) {
            if (controller.settingsBackendRef && controller.settingsBackendRef.setting_auto_save_enabled) {
                controller.autoSaveTimer.restart();
            }
        }
    }

    function flushActiveEditorBeforeSync() {
        if (!editorBackendRef || !chapterId || !projectId || !volumeId) return true;
        if (saveGuardActive()) {
            pendingAutoSaveAfterGuard = false;
            return true;
        }
        var read = readEditorPlainText();
        if (read.suspiciousEmpty) {
            logWriterWarning("flush_before_sync_blocked_empty", "blocked suspicious empty save before sync");
            return false;
        }
        var plainText = read.text;
        if (plainText === lastSavedEditorText) return true;
        var allowEmptyOverwrite = plainText.length === 0 && explicitEmptySavePending;
        var result = editorBackendRef.save_chapter(projectId, volumeId, chapterId, plainText, allowEmptyOverwrite);
        if (result && result.success) {
            lastSavedEditorText = plainText;
            previousEditorText = plainText;
            if (plainText.length === 0) {
                explicitEmptySavePending = false;
                lastPotentialExplicitClearAtMs = 0;
            }
            return true;
        }
        logWriterWarning("flush_before_sync_failed", "save failed before sync, blocking sync");
        return false;
    }

    // Unified save entry — always writes normalized plain text via backend.
    function saveCurrentChapter() {
        if (!editorBackendRef || !chapterId || !projectId || !volumeId) return;
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

        var result = editorBackendRef.save_chapter(projectId, volumeId, chapterId, plainText, allowEmptyOverwrite);
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
        if (!cId || !pId || !vId || !editorBackendRef) return null;
        if (!targetEditorItem) return null;
        if (isLoadingChapter) return null;

        isLoadingChapter = true;

        var result = editorBackendRef.open_chapter(pId, vId, cId);

        if (!result.success) {
            console.error("[SujianDebug] Failed to open chapter:", result.errorCode || result.rawError);
            isLoadingChapter = false;
            return null;
        }

        var content = normalizePlainText(result.data ? result.data.content || "" : "");

        // Document identity is the complete project/volume/chapter tuple. A
        // different chapter can have the same canonical text (including empty).
        var isReload = (pId === controller.projectId
                && vId === controller.volumeId
                && cId === controller.chapterId);
        if (isReload) {
            targetEditorItem.reload_plain_text(content);
        } else {
            targetEditorItem.load_document_text(content);
            targetEditorItem.clear_undo_stack();
        }

        previousEditorText = content;
        lastSavedEditorText = content;
        explicitEmptySavePending = false;
        lastPotentialExplicitClearAtMs = 0;
        isLoadingChapter = false;

        // Return full result so caller updates chapter state from authoritative source
        return result;
    }

    // Issue #754 评论 5815901258: 清空当前章节编辑器状态。
    // 用 isLoadingChapter 包住清理，避免清空正文被当成用户编辑触发保存。
    // 同步删除当前章节/分卷/作品后调用，使编辑器退回"请选择或新建章节"。
    function clearActiveChapter() {
        isLoadingChapter = true;
        try {
            if (targetEditorItem) {
                targetEditorItem.load_document_text("");
                targetEditorItem.clear_undo_stack();
            }
            projectId = "";
            volumeId = "";
            chapterId = "";
            chapterTitle = "";
            previousEditorText = "";
            lastSavedEditorText = "";
            explicitEmptySavePending = false;
            lastPotentialExplicitClearAtMs = 0;
        } finally {
            isLoadingChapter = false;
        }
    }

    function applyCurrentSettings() {
        // Issue #721: applyCurrentSettings() 只启动/刷新保存 guard，
        // 不再影响编辑器渲染。主题切换走 text_color/selection_color 等
        // 绑定自动下发，不进入此函数。
        settingsSaveGuardActive = true;
        if (autoSaveTimer.running) {
            pendingAutoSaveAfterGuard = true;
            autoSaveTimer.stop();
        }
        settingsGuardTimer.restart();
    }

    function formatText() {
        if (!targetEditorItem) return;
        var read = readEditorPlainText();
        if (read.suspiciousEmpty) {
            blockUnsafeEmptySave("format_text_read", read);
            return;
        }

        var raw = read.text;
        if (!raw) return;
        if (!editorBackendRef || !editorBackendRef.format_plain_text) return;
        // 正文格式化规则归 Core：这里只触发后端业务操作、刷新编辑器快照并保存。
        var plain = editorBackendRef.format_plain_text(raw);

        isApplyingFormat = true;
        autoSaveTimer.stop();
        try {
            targetEditorItem.reload_plain_text(plain);
            targetEditorItem.clear_undo_stack();
        } finally {
            isApplyingFormat = false;
        }

        saveCurrentChapter();
    }
}
