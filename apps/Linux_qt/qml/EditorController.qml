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
    signal chapterLoaded(string projectId, string volumeId, string chapterId)
    signal chapterLoadFailed(string projectId, string volumeId, string chapterId, string reason)

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
    // 本章字数：Core 在 statsTimer 防抖后计算 word_count，开章加载文本同样会触发刷新。
    readonly property int chapterWordCount: editorBackendRef ? editorBackendRef.word_count : 0
    property string syncFlushFailureReason: ""

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
    property var correctionEntityNames: []
    property bool correctionCheckPending: false
    property int correctionRequestSerial: 0
    property int pendingCorrectionRequestId: 0
    property double pendingCorrectionRevision: -1
    property string pendingCorrectionProjectId: ""
    property string pendingCorrectionChapterId: ""
    property string pendingCorrectionText: ""
    property bool pendingCorrectionSilent: false
    property string correctionTrackedText: ""
    property double correctionTrackedRevision: -1
    property string correctionTrackedProjectId: ""
    property string correctionTrackedChapterId: ""
    // Identifies one opened editor document inside Core's per-chapter cache.
    // It is a backend identity only; request IDs above remain UI-only.
    property int correctionDocumentEpoch: 1
    // Epoch attached to every in-flight request, so a delayed response from a
    // previous document cannot overwrite the current chapter's annotations.
    property var correctionRequestEpochs: ({})
    property var editorViewportRef: null

    // Issue #954：编辑事务先走轻量位置更新，模型扫描防抖到输入停顿后一次。
    property var correctionScanDebounceTimer: Timer {
        interval: 250
        repeat: false
        onTriggered: controller.requestCorrectionPendingNow()
    }

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

    // NOTE: sync is NOT triggered by save. Auto-sync has been removed
    // (Issue #729). This is intentional: save ≠ sync.

    property var editorItemConnections: Connections {
        target: targetEditorItem ? targetEditorItem : null
        function onText_changed() {
            controller.handlePlainTextChanged();
            Qt.callLater(targetEditorItem.flush_content_height);
        }
        // Issue #843: 编辑事务事实从 Rust EditorEditResult 直接传来，
        // cause + inserted/deleted 不再在 QML 中用文本长度 diff 猜。
        function onEditor_change_applied(cause, insertedChars, deletedChars,
                                         baseRevision, newRevision, editRangesJson) {
            controller.reconcileCorrectionEdit(baseRevision, newRevision, editRangesJson);
            if (controller.chapterId && controller.editorBackendRef) {
                controller.editorBackendRef.record_editor_change_stats(
                    controller.projectId, controller.volumeId, controller.chapterId,
                    cause, insertedChars, deletedChars);
            }
        }
    }

    property var correctionBackendConnections: Connections {
        target: editorBackendRef ? editorBackendRef : null
        function onCorrection_check_ready(requestId, textRevision, snapshotJson) {
            controller.receiveCorrectionSuggestions(requestId, textRevision, snapshotJson);
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

    function cancelPendingCorrectionRequest() {
        correctionScanDebounceTimer.stop();
        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        correctionCheckPending = false;
    }

    function registerCorrectionRequest(requestId) {
        correctionRequestEpochs[requestId] = correctionDocumentEpoch;
    }

    function takeCorrectionRequestEpoch(requestId) {
        var epoch = correctionRequestEpochs[requestId];
        delete correctionRequestEpochs[requestId];
        return epoch;
    }

    function updateCorrectionSuggestions(suggestions) {
        correctionSuggestions = suggestions;
        if (!targetEditorItem || !targetEditorItem.set_text_decorations_json) return;
        var spans = [];
        for (var i = 0; i < suggestions.length; i += 1) {
            var suggestion = suggestions[i];
            spans.push({
                startByte: suggestion.startByte,
                endByte: suggestion.endByte,
                severity: suggestion.severity,
                textRevision: suggestion.textRevision
            });
        }
        targetEditorItem.set_text_decorations_json(JSON.stringify(spans));
    }

    // Clears the visible marks without changing the document identity. In-flight
    // responses are rejected by dropping their request epochs; the Core cache
    // stays valid so later edits remain incremental.
    function clearCorrectionAnnotations() {
        cancelPendingCorrectionRequest();
        correctionRequestEpochs = ({});
        pendingCorrectionRevision = -1;
        pendingCorrectionProjectId = "";
        pendingCorrectionChapterId = "";
        pendingCorrectionText = "";
        pendingCorrectionSilent = false;
        updateCorrectionSuggestions([]);
        correctionError = "";
        correctionStatus = "";
    }

    function releaseCorrectionSession(releaseProjectId, releaseChapterId) {
        if (!releaseProjectId || !releaseChapterId) return;
        if (!editorBackendRef || !editorBackendRef.release_correction_session) return;
        editorBackendRef.release_correction_session(releaseProjectId, releaseChapterId);
    }

    function invalidateCorrectionResults() {
        cancelPendingCorrectionRequest();
        correctionDocumentEpoch += 1;
        if (correctionDocumentEpoch > 2147483000) correctionDocumentEpoch = 1;
        correctionRequestEpochs = ({});
        pendingCorrectionRevision = -1;
        pendingCorrectionProjectId = "";
        pendingCorrectionChapterId = "";
        pendingCorrectionText = "";
        pendingCorrectionSilent = false;
        updateCorrectionSuggestions([]);
        correctionError = "";
        correctionStatus = "";
    }

    function syncCorrectionTracking(text, revision, trackedProjectId, trackedChapterId) {
        correctionTrackedText = text;
        correctionTrackedRevision = Number(revision);
        correctionTrackedProjectId = trackedProjectId === undefined ? projectId : trackedProjectId;
        correctionTrackedChapterId = trackedChapterId === undefined ? chapterId : trackedChapterId;
    }

    function ensureCorrectionTracking(text, revision) {
        if (correctionTrackedText === text
                && correctionTrackedRevision === Number(revision)
                && correctionTrackedProjectId === projectId
                && correctionTrackedChapterId === chapterId) return true;
        updateCorrectionSuggestions([]);
        syncCorrectionTracking(text, revision);
        return false;
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

        cancelPendingCorrectionRequest();

        var snapshotText = getEditorPlainText();
        var revision = Number(targetEditorItem.text_revision);
        if (!isFinite(revision) || revision < 0) {
            correctionError = qsTr("无法读取正文版本。");
            return;
        }
        ensureCorrectionTracking(snapshotText, revision);
        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        registerCorrectionRequest(pendingCorrectionRequestId);
        pendingCorrectionRevision = revision;
        pendingCorrectionProjectId = projectId;
        pendingCorrectionChapterId = chapterId;
        pendingCorrectionText = snapshotText;
        pendingCorrectionSilent = false;
        correctionError = "";
        correctionStatus = qsTr("正在校对本章…");
        correctionCheckPending = true;
        editorBackendRef.request_correction_chapter(
            pendingCorrectionRequestId,
            projectId,
            chapterId,
            correctionDocumentEpoch,
            revision,
            snapshotText);
    }

    // Issue #954：从 Core 缓存读取当前章节仍有效的校对结果，不重跑模型。
    // 章节打开以及编辑事务连续性丢失时调用；Core 按文本/上下文指纹重新定位
    // 每条波线，QML 直接整体替换结果，不再本地推断 byte 偏移。
    function requestCorrectionSnapshot() {
        if (!editorBackendRef || !targetEditorItem || !projectId || !chapterId) return;
        if (!editorBackendRef.request_correction_snapshot) return;
        if (isLoadingChapter || isApplyingFormat || saveGuardActive()) return;
        correctionScanDebounceTimer.stop();

        var snapshotText = getEditorPlainText();
        var revision = Number(targetEditorItem.text_revision);
        if (!isFinite(revision) || revision < 0) return;
        ensureCorrectionTracking(snapshotText, revision);

        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        registerCorrectionRequest(pendingCorrectionRequestId);
        pendingCorrectionRevision = revision;
        pendingCorrectionProjectId = projectId;
        pendingCorrectionChapterId = chapterId;
        pendingCorrectionText = snapshotText;
        pendingCorrectionSilent = true;
        // Issue #978 新评论: 快照请求本身也要标记在途，避免连续性丢失时连续按键
        // 反复触发整章读取。
        correctionCheckPending = true;
        editorBackendRef.request_correction_snapshot(
            pendingCorrectionRequestId,
            projectId,
            chapterId,
            correctionDocumentEpoch,
            revision,
            snapshotText);
    }

    // Map the requested UTF-8 byte boundaries with one forward walk. QML text
    // positions are UTF-16 units; char offsets count Unicode scalar values.
    function correctionPositionsForBytes(text, byteOffsets) {
        var endpoints = [];
        for (var index = 0; index < byteOffsets.length; index += 1) {
            var byte = Number(byteOffsets[index]);
            if (!isFinite(byte) || Math.floor(byte) !== byte || byte < 0) {
                throw new Error("校对 byte range 无效");
            }
            endpoints.push({ byte: byte, index: index });
        }
        endpoints.sort(function(a, b) { return a.byte - b.byte || a.index - b.index; });
        var positions = new Array(byteOffsets.length);
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
            positions[endpoint.index] = { utf16: utf16Offset, char: charOffset };
        }
        return positions;
    }

    function annotateCorrectionPositions(text, suggestions) {
        var endpoints = [];
        for (var index = 0; index < suggestions.length; index += 1) {
            var startByte = Number(suggestions[index].startByte);
            var endByte = Number(suggestions[index].endByte);
            if (!isFinite(startByte) || !isFinite(endByte)
                    || Math.floor(startByte) !== startByte
                    || Math.floor(endByte) !== endByte
                    || startByte < 0 || endByte <= startByte) {
                throw new Error("校对 byte range 无效");
            }
            endpoints.push(startByte, endByte);
        }
        var positions = correctionPositionsForBytes(text, endpoints);
        for (var suggestionIndex = 0; suggestionIndex < suggestions.length; suggestionIndex += 1) {
            var item = suggestions[suggestionIndex];
            item.startUtf16 = positions[suggestionIndex * 2].utf16;
            item.startChar = positions[suggestionIndex * 2].char;
            item.endUtf16 = positions[suggestionIndex * 2 + 1].utf16;
            item.endChar = positions[suggestionIndex * 2 + 1].char;
            if (text.slice(item.startUtf16, item.endUtf16) !== item.original) {
                throw new Error("校对原文与 byte range 不匹配");
            }
        }
        return suggestions;
    }

    // Issue #954：编辑事务先只把真实 patch 范围交给 Core 更新内存段索引，
    // 不读取/复制整章正文，也不立刻运行模型。输入停顿后再提交一次“扫描脏段”。
    function reconcileCorrectionEdit(baseRevision, newRevision, patchRangesJson) {
        if (!targetEditorItem || isLoadingChapter || isApplyingFormat) return;
        var actualRevision = Number(targetEditorItem.text_revision);
        var base = Number(baseRevision);
        var next = Number(newRevision);

        // Issue #978 新评论: apply_edit_with_visuals 已在发出
        // editor_change_applied 前把 text_revision 推进到 next，因此这里的
        // actualRevision 应直接等于 next，正常输入走 patch-only 路径。
        // 只有真正失去连续性时才由快照请求读取一次正文；已有请求在途时不重复。
        if (!isFinite(base) || !isFinite(next) || actualRevision !== next
                || base !== correctionTrackedRevision
                || correctionTrackedProjectId !== projectId
                || correctionTrackedChapterId !== chapterId
                || !editorBackendRef || !editorBackendRef.apply_correction_edits) {
            if (!correctionCheckPending) {
                requestCorrectionSnapshot();
            }
            return;
        }

        correctionTrackedRevision = next;
        correctionTrackedProjectId = projectId;
        correctionTrackedChapterId = chapterId;
        // Issue #954/#978: 只把 patch/revision 交给 Core 更新内存段索引，
        // 不复制整章正文；模型阶段由 requestCorrectionPendingNow 在停顿后读取一次。
        var applied = editorBackendRef.apply_correction_edits(
            projectId,
            chapterId,
            correctionDocumentEpoch,
            next,
            base,
            String(patchRangesJson || "[]"));
        if (applied && applied.ok === false) {
            if (!correctionCheckPending) {
                requestCorrectionSnapshot();
            }
            return;
        }
        correctionError = "";
        correctionStatus = qsTr("正在校对修改段落…");
        correctionScanDebounceTimer.restart();
    }

    // Issue #954：模型阶段。仅在输入停顿后取一次当前正文，Core 只扫描第一阶段
    // 记录的脏窗口；连续输入不会为每个按键排队一次全文模型任务。
    function requestCorrectionPendingNow() {
        if (!editorBackendRef || !targetEditorItem || !projectId || !chapterId) return;
        if (!editorBackendRef.request_correction_pending) return;
        if (isLoadingChapter || isApplyingFormat || saveGuardActive()) return;
        var snapshotText = getEditorPlainText();
        var revision = Number(targetEditorItem.text_revision);
        if (!isFinite(revision) || revision < 0) return;
        syncCorrectionTracking(snapshotText, revision);

        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        registerCorrectionRequest(pendingCorrectionRequestId);
        pendingCorrectionRevision = revision;
        pendingCorrectionProjectId = projectId;
        pendingCorrectionChapterId = chapterId;
        pendingCorrectionText = snapshotText;
        pendingCorrectionSilent = false;
        correctionCheckPending = true;
        editorBackendRef.request_correction_pending(
            pendingCorrectionRequestId,
            projectId,
            chapterId,
            correctionDocumentEpoch,
            revision,
            snapshotText);
    }

    // Issue #954：Core 返回整章合并快照 { textRevision, suggestions, checkedRanges }，
    // 已按当前正文重新定位；QML 直接整体替换结果列表，不再本地合并。
    function receiveCorrectionSuggestions(requestId, textRevision, snapshotJson) {
        var requestEpoch = takeCorrectionRequestEpoch(requestId);
        if (requestEpoch === undefined || requestEpoch !== correctionDocumentEpoch) return;
        var isPending = requestId === pendingCorrectionRequestId;
        if (isPending) {
            correctionCheckPending = false;
        }
        // Request IDs only drive the pending indicator. Core always returns a
        // merged chapter snapshot; any response matching the current revision
        // and document is safe to display even if a newer request is in flight.
        if (!targetEditorItem
                || textRevision !== Number(targetEditorItem.text_revision)
                || projectId !== correctionTrackedProjectId
                || chapterId !== correctionTrackedChapterId) return;
        var currentText = getEditorPlainText();
        try {
            var parsed = JSON.parse(String(snapshotJson));
            if (!parsed || typeof parsed !== "object"
                    || !Array.isArray(parsed.suggestions)
                    || Number(parsed.textRevision) !== textRevision) {
                throw new Error("校对结果格式无效");
            }
            var suggestions = parsed.suggestions;
            for (var i = 0; i < suggestions.length; i += 1) {
                if (Number(suggestions[i].textRevision) !== textRevision) throw new Error("校对正文版本无效");
            }
            suggestions.sort(function(a, b) { return a.startByte - b.startByte; });
            annotateCorrectionPositions(currentText, suggestions);
            for (var contextIndex = 0; contextIndex < suggestions.length; contextIndex += 1) {
                suggestions[contextIndex].contextHtml =
                        correctionContextHtmlForText(currentText, suggestions[contextIndex]);
            }
            updateCorrectionSuggestions(suggestions);
            syncCorrectionTracking(currentText, textRevision);
            if (isPending) {
                correctionStatus = suggestions.length > 0
                        ? qsTr("发现 %1 条建议").arg(suggestions.length)
                        : qsTr("未发现建议。");
            }
        } catch (error) {
            if (!isPending) return;
            correctionError = String(error);
            correctionStatus = "";
        }
    }

    function receiveCorrectionFailure(requestId, textRevision, message) {
        var requestEpoch = takeCorrectionRequestEpoch(requestId);
        if (requestEpoch === undefined || requestEpoch !== correctionDocumentEpoch) return;
        if (requestId !== pendingCorrectionRequestId) return;
        var silent = pendingCorrectionSilent;
        correctionCheckPending = false;
        if (silent) return;
        if (textRevision !== Number(targetEditorItem ? targetEditorItem.text_revision : -1)) {
            correctionStatus = qsTr("正文已变化，请重新校对。");
            return;
        }
        correctionStatus = "";
        correctionError = String(message || qsTr("校对失败。"));
    }

    function suggestionIsCurrent(suggestion) {
        if (!(targetEditorItem
                && Number(suggestion.textRevision) === Number(targetEditorItem.text_revision)
                && Number(suggestion.textRevision) === correctionTrackedRevision
                && projectId === correctionTrackedProjectId
                && chapterId === correctionTrackedChapterId)) return false;
        var currentText = getEditorPlainText();
        return currentText.slice(suggestion.startUtf16, suggestion.endUtf16) === suggestion.original;
    }

    function escapeCorrectionHtml(value) {
        return String(value)
                .replace(/&/g, "&amp;")
                .replace(/</g, "&lt;")
                .replace(/>/g, "&gt;");
    }

    // A short clickable context snippet with the reported range highlighted.
    // The snapshot response precomputes this once per suggestion so delegates
    // never read the full chapter text during list layout.
    function correctionContextHtmlForText(text, suggestion) {
        if (!suggestion) return "";
        var highlighted = escapeCorrectionHtml(suggestion.original || "");
        var start = Number(suggestion.startUtf16);
        var end = Number(suggestion.endUtf16);
        if (!isFinite(start) || !isFinite(end) || start < 0 || end < start || end > text.length) {
            return "<b>" + highlighted + "</b>";
        }
        var radius = 12;
        var beforeStart = Math.max(0, start - radius);
        var afterEnd = Math.min(text.length, end + radius);
        var before = escapeCorrectionHtml(text.slice(beforeStart, start).replace(/[\r\n\u2028\u2029]/g, " "));
        var after = escapeCorrectionHtml(text.slice(end, afterEnd).replace(/[\r\n\u2028\u2029]/g, " "));
        var color = correctionSeverityColor(suggestion);
        return (beforeStart > 0 ? "…" : "")
                + before
                + "<font color=\"" + color + "\"><b>" + highlighted + "</b></font>"
                + after
                + (afterEnd < text.length ? "…" : "");
    }

    function correctionContextHtml(suggestion) {
        if (!suggestion) return "";
        return correctionContextHtmlForText(getEditorPlainText(), suggestion);
    }

    // The dictionary stores complete, author-confirmed words, never the
    // engine's suspicious span. Prefer the writing selection; otherwise show a
    // short editable context around the suggestion.
    function correctionDictionaryEntryDraft(suggestion) {
        var selected = targetEditorItem && targetEditorItem.selected_text
                ? String(targetEditorItem.selected_text()).trim() : "";
        if (selected.length > 0) return selected;
        if (!suggestion) return "";
        var text = getEditorPlainText();
        var start = Number(suggestion.startUtf16);
        var end = Number(suggestion.endUtf16);
        if (isFinite(start) && isFinite(end) && start >= 0 && end >= start && end <= text.length) {
            var before = text.slice(Math.max(0, start - 4), start).replace(/[\r\n\u2028\u2029]/g, " ");
            var after = text.slice(end, Math.min(text.length, end + 4)).replace(/[\r\n\u2028\u2029]/g, " ");
            return (before + String(suggestion.original || "") + after).trim();
        }
        return String(suggestion.original || "").trim();
    }

    // Issue #982：把 Core 搜索结果中的原始 UTF-8 字节范围映射为编辑器选区。
    // 使用与校对跳转相同的字节校验路径，不依赖 QML 自行换算正文位置。
    function revealSearchRange(startByte, endByte) {
        if (!targetEditorItem || !targetEditorItem.reveal_correction) return false;
        var start = Number(startByte);
        var end = Number(endByte);
        if (!isFinite(start) || !isFinite(end) || start < 0 || end < start) return false;
        var text = getEditorPlainText();
        try {
            var positions = correctionPositionsForBytes(text, [start, end]);
            var startUtf16 = positions[0].utf16;
            var endUtf16 = positions[1].utf16;
            if (endUtf16 <= startUtf16) return false;
            var original = text.slice(startUtf16, endUtf16);
            var revision = Number(targetEditorItem.text_revision);
            if (!isFinite(revision) || revision < 0) return false;
            var revealed = targetEditorItem.reveal_correction(
                start,
                end,
                original,
                revision
            );
            if (revealed && editorViewportRef && editorViewportRef.revealCurrentCaret) {
                editorViewportRef.revealCurrentCaret();
            }
            return revealed;
        } catch (e) {
            return false;
        }
    }

    function jumpToCorrection(suggestion) {
        if (!suggestion || !suggestionIsCurrent(suggestion)) return false;
        if (!targetEditorItem || !targetEditorItem.reveal_correction) return false;
        var revealed = targetEditorItem.reveal_correction(
            Number(suggestion.startByte),
            Number(suggestion.endByte),
            String(suggestion.original),
            Number(suggestion.textRevision));
        if (!revealed) return false;
        if (editorViewportRef && editorViewportRef.revealCurrentCaret) {
            editorViewportRef.revealCurrentCaret();
        }
        return true;
    }

    function correctionReason(suggestion) {
        var reasons = {
            "confusion.custom": qsTr("命中作品中的自定义易混词替换。"),
            "proper.confirmed_misspelling": qsTr("这是你为该专名登记的错误写法，建议改为完整原名。"),
            "proper.entity_check": qsTr("与作品词典中的专名相近，请核对。"),
            "proper.person_check": qsTr("与作品词典中的人名相近，请核对。"),
            "proper.place_check": qsTr("与作品词典中的地名相近，请核对。"),
            "detector.exact": qsTr("词典检测给出了明确替换。"),
            "detector.oov": qsTr("该片段未被通用分词词典收录；只有同时存在可解释的字符或词形依据时才提示。"),
            "detector.char_mad": qsTr("该字在当前上下文中的语言模型得分异常，且存在可解释的替换候选。"),
            "detector.orthographic": qsTr("该片段存在可解释的形近字冲突，且上下文支持替换。"),
            "detector.candidate": qsTr("词典词形与上下文字符证据共同支持该替换，请结合语境核对。"),
            "punctuation.repeated": qsTr("发现连续重复标点；这也可能是有意的表达。"),
            "character.repeated": qsTr("发现连续重复汉字；请结合语境确认。")
        };
        var sourceNames = {
            "confusion_word": qsTr("易混词"),
            "proper_name": qsTr("作品专名核对"),
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
        updateCorrectionSuggestions(correctionSuggestions);
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
        updateCorrectionSuggestions(remaining);
        correctionStatus = scope === "once" ? qsTr("已忽略这一次。")
                : scope === "chapter" ? qsTr("已在本章忽略此类建议。")
                : qsTr("已停用此规则。") ;
        return true;
    }

    function correctionEntityWrittenDraft(suggestion) {
        var selected = targetEditorItem && targetEditorItem.selected_text
                ? String(targetEditorItem.selected_text()).trim() : "";
        if (selected.length > 0) return selected;
        return suggestion ? String(suggestion.original || "").trim() : "";
    }

    function refreshCorrectionEntityNames(projectIdOverride) {
        var targetProjectId = String(projectIdOverride || projectId || "").trim();
        if (!editorBackendRef || !editorBackendRef.get_correction_entity_names
                || !targetProjectId) {
            correctionEntityNames = [];
            return;
        }
        var result = editorBackendRef.get_correction_entity_names(targetProjectId);
        if (!result || !result.success) {
            correctionEntityNames = [];
            return;
        }
        var data = result.data || {};
        correctionEntityNames = data.entities || [];
    }

    function removeCorrectionEntityName(canonical, name) {
        var canonicalName = String(canonical || "").trim();
        var writtenForm = String(name || "").trim();
        if (!canonicalName || !writtenForm) return false;
        if (!editorBackendRef || !editorBackendRef.remove_correction_entity_name) return false;
        var result = editorBackendRef.remove_correction_entity_name(
            projectId, canonicalName, writtenForm);
        if (!result || !result.success) {
            correctionDictionaryError = result && result.rawError
                    ? result.rawError : qsTr("删除名称对应失败。");
            return false;
        }
        correctionDictionaryError = "";
        refreshCorrectionEntityNames();
        invalidateCorrectionResults();
        requestCorrectionChapter();
        correctionStatus = qsTr("已删除名称对应关系，正在重新校对本章。");
        return true;
    }

    function addCorrectionEntityName(canonical, name, kind) {
        var canonicalName = String(canonical || "").trim();
        var writtenForm = String(name || "").trim();
        if (!canonicalName || !writtenForm) {
            correctionError = qsTr("请填写完整原名和对应写法。");
            return false;
        }
        if (!editorBackendRef || !editorBackendRef.add_correction_entity_name) return false;
        var result = editorBackendRef.add_correction_entity_name(
            projectId, canonicalName, writtenForm, String(kind || "misspelling"));
        if (!result || !result.success) {
            correctionError = result && result.rawError ? result.rawError : qsTr("登记名称对应失败。");
            return false;
        }
        correctionError = "";
        refreshCorrectionEntityNames();
        invalidateCorrectionResults();
        requestCorrectionChapter();
        correctionStatus = kind === "alias"
                ? qsTr("已登记为同一名称的别名，正在重新校对本章。")
                : qsTr("已登记为错误写法，正在重新校对本章。");
        return true;
    }

    function addCorrectionWord(entry, kind) {
        var fullEntry = String(entry || "").trim();
        if (!fullEntry) {
            correctionError = qsTr("请填写完整的作品词条。");
            return false;
        }
        if (!editorBackendRef) return false;
        var entryKind = String(kind || "term");
        var result;
        if (editorBackendRef.add_correction_dictionary_entry_with_kind) {
            result = editorBackendRef.add_correction_dictionary_entry_with_kind(
                projectId, fullEntry, entryKind);
        } else if (editorBackendRef.add_correction_dictionary_entry) {
            result = editorBackendRef.add_correction_dictionary_entry(projectId, fullEntry);
        } else {
            return false;
        }
        if (!result || !result.success) {
            correctionError = result && result.rawError ? result.rawError : qsTr("加入作品词典失败。");
            return false;
        }
        correctionError = "";
        // The dictionary changed the scanner fingerprint. Rebuild the visible
        // results against the new protection set instead of hiding unrelated
        // suggestions by matching their original text.
        invalidateCorrectionResults();
        requestCorrectionChapter();
        correctionStatus = entryKind === "person"
                ? qsTr("已作为人物加入作品词典，正在重新校对本章。")
                : entryKind === "place"
                  ? qsTr("已作为地点加入作品词典，正在重新校对本章。")
                  : qsTr("已作为普通术语加入作品词典，正在重新校对本章。");
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

    function handlePlainTextChanged() {
        if (controller.saveGuardActive()) return;

        // Issue #978: 每键只更新轻量事实并重启防抖计时器，不读取整章正文。
        // 空正文保护在 saveCurrentChapter / flushActiveEditorBeforeSync 等保存边界执行。
        var editorItemLength = -1;
        if (targetEditorItem) {
            if (targetEditorItem.plain_text_length !== undefined) {
                editorItemLength = Number(targetEditorItem.plain_text_length);
            } else if (targetEditorItem.content_empty !== undefined) {
                editorItemLength = targetEditorItem.content_empty === true ? 0 : 1;
            }
        }
        if (isFinite(editorItemLength) && editorItemLength > 0) {
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
        syncFlushFailureReason = ""
        if (!editorBackendRef || !chapterId || !projectId || !volumeId) return true;
        if (saveGuardActive()) {
            pendingAutoSaveAfterGuard = false;
            return true;
        }
        var read = readEditorPlainText();
        if (read.suspiciousEmpty) {
            syncFlushFailureReason = qsTr("检测到可疑空正文，已阻止保存")
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
        syncFlushFailureReason = result
            ? (result.rawError || result.messageKey || result.errorCode || qsTr("保存当前章节失败"))
            : qsTr("保存当前章节失败")
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
            chapterLoadFailed(pId, vId, cId, String(result.rawError || result.errorCode || "open_failed"));
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
        releaseCorrectionSession(controller.projectId, controller.chapterId);
        invalidateCorrectionResults();
        syncCorrectionTracking(content, Number(targetEditorItem.text_revision), pId, cId);
        refreshCorrectionEntityNames(pId);
        isLoadingChapter = false;
        // Issue #954：章节打开只恢复 Core 缓存中仍有效的校对结果；未校对过的
        // 段落由后续编辑事务或手动“检查本章”补算，不因切章重扫整章。
        requestCorrectionSnapshot();

        // 成功信号由外层 WritingWorkspace 在章节身份、选中状态和视口都提交后发出。
        // 这里只负责载入正文，避免搜索定位回调在身份尚未更新时重入 openChapter。

        // Return full result so caller updates chapter state from authoritative source
        return result;
    }

    // Issue #754 评论 5815901258: 清空当前章节编辑器状态。
    // 用 isLoadingChapter 包住清理，避免清空正文被当成用户编辑触发保存。
    // 同步删除当前章节/分卷/作品后调用，使编辑器退回"请选择或新建章节"。
    function clearActiveChapter() {
        isLoadingChapter = true;
        try {
            releaseCorrectionSession(projectId, chapterId);
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
            correctionEntityNames = [];
            invalidateCorrectionResults();
            syncCorrectionTracking("", targetEditorItem ? Number(targetEditorItem.text_revision) : -1, "", "");
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

        invalidateCorrectionResults();
        syncCorrectionTracking(plain, Number(targetEditorItem.text_revision));
        requestCorrectionSnapshot();

        saveCurrentChapter();
    }
}
