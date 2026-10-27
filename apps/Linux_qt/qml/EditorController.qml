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
    signal annotationTargetActivated(var target)
    signal annotationLinkChoicesRequested(var choices)
    signal annotationLinkActivationFailed(string message)

    // Target UI bindings
    property var targetEditorItem: null
    property var editorBackendRef: null
    property var annotationBackendRef: null
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
    property var proofreadingDecorations: []
    property var annotationDecorations: []
    property var annotationNavigationSnapshot: null
    property double annotationBaselineRevision: -1
    property string annotationError: ""
    property string annotationTrackedProjectId: ""
    property string annotationTrackedChapterId: ""
    property double annotationTrackedRevision: -1
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

    property var annotationRefreshTimer: Timer {
        interval: 120
        repeat: false
        onTriggered: controller.refreshChapterAnnotations()
    }

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

    // 手写证明链起点：章节加载时的完整正文。只在本次加载后的第一条编辑事务上报一次；
    // 后续事件由 Core 用链式 hash 承接，不重复传整章。
    property bool proofBaselineSent: false
    property string proofBaselineText: ""

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
                                         baseRevision, newRevision, editRangesJson,
                                         changesJson, oldSelectionAnchor, oldSelectionHead,
                                         newSelectionAnchor, newSelectionHead) {
            controller.reconcileCorrectionEdit(baseRevision, newRevision, editRangesJson);
            controller.applyAnnotationEditRanges(cause, baseRevision, newRevision, editRangesJson);
            if (controller.chapterId && controller.editorBackendRef) {
                controller.editorBackendRef.record_editor_change_stats(
                    controller.projectId, controller.volumeId, controller.chapterId,
                    cause, insertedChars, deletedChars, changesJson,
                    baseRevision, newRevision, oldSelectionAnchor, oldSelectionHead,
                    newSelectionAnchor, newSelectionHead,
                    controller.proofBaselineSent ? "" : controller.proofBaselineText);
            if (changesJson && changesJson !== "[]") {
                controller.proofBaselineSent = true;
            }
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
        proofreadingDecorations = [];
        for (var i = 0; i < suggestions.length; i += 1) {
            var suggestion = suggestions[i];
            proofreadingDecorations.push({
                startByte: suggestion.startByte,
                endByte: suggestion.endByte,
                severity: suggestion.severity,
                textRevision: suggestion.textRevision
            });
        }
        applyTextDecorations();
    }

    function applyTextDecorations() {
        if (!targetEditorItem || !targetEditorItem.set_text_decorations_json) return;
        var spans = proofreadingDecorations.concat(annotationDecorations);
        targetEditorItem.set_text_decorations_json(JSON.stringify(spans));
    }

    function applyAnnotationEditRanges(cause, baseRevision, newRevision, rangesJson) {
        if (!annotationBackendRef || !projectId || !chapterId || isLoadingChapter || isApplyingFormat)
            return;
        var edits = [];
        try {
            var ranges = JSON.parse(String(rangesJson || "[]"));
            for (var i = 0; i < ranges.length; ++i) {
                var range = ranges[i];
                if (!range || range.length < 4) continue;
                edits.push({
                    oldStartByte: Number(range[0]), oldEndByte: Number(range[1]),
                    newStartByte: Number(range[2]), newEndByte: Number(range[3])
                });
            }
        } catch (e) {
            annotationError = qsTr("正文标记更新范围无效")
            annotationDecorations = []
            annotationNavigationSnapshot = null
            annotationBaselineRevision = -1
            applyTextDecorations()
            return
        }
        var snapshot = annotationNavigationSnapshot
        if (!snapshot || String(snapshot.projectId) !== String(projectId)
                || String(snapshot.chapterId) !== String(chapterId)
                || Number(annotationBaselineRevision) !== Number(baseRevision)
                || String(snapshot.baselineId || "").length === 0) {
            annotationError = qsTr("正文标记基准已失效，正在重新载入")
            annotationNavigationSnapshot = null
            annotationBaselineRevision = -1
            annotationRefreshTimer.restart()
            return
        }
        var raw = annotationBackendRef.apply_annotation_edits_json(
            projectId, chapterId, String(snapshot.baselineId),
            Number(baseRevision), Number(newRevision), String(cause),
            JSON.stringify(edits))
        var envelope = null
        try { envelope = JSON.parse(String(raw || "{}")) } catch (e) {}
        if (!envelope || envelope.success !== true) {
            annotationError = envelope && envelope.rawError ? String(envelope.rawError) : qsTr("正文标记未能更新")
            annotationDecorations = []
            annotationNavigationSnapshot = null
            annotationBaselineRevision = -1
            applyTextDecorations()
            annotationRefreshTimer.restart()
            return
        }
        annotationError = ""
        annotationBaselineRevision = Number(newRevision)
        annotationRefreshTimer.restart()
    }

    function refreshChapterAnnotations() {
        if (!annotationBackendRef || !targetEditorItem || !projectId || !chapterId) {
            annotationDecorations = []
            annotationNavigationSnapshot = null
            annotationBaselineRevision = -1
            annotationTrackedProjectId = ""
            annotationTrackedChapterId = ""
            annotationTrackedRevision = -1
            applyTextDecorations()
            return
        }
        var revision = Number(targetEditorItem.text_revision)
        var text = getEditorPlainText()
        var existingSnapshot = annotationNavigationSnapshot
        var baselineId = existingSnapshot
                && String(existingSnapshot.projectId) === String(projectId)
                && String(existingSnapshot.chapterId) === String(chapterId)
                ? String(existingSnapshot.baselineId || "") : ""
        var raw = annotationBackendRef.get_chapter_annotations_json(
            projectId, chapterId, revision, text, baselineId)
        var envelope = null
        try { envelope = JSON.parse(String(raw || "{}")) } catch (e) {}
        if (!envelope || envelope.success !== true || !envelope.data) {
            annotationError = envelope && envelope.rawError ? String(envelope.rawError) : qsTr("正文标记暂不可用")
            annotationDecorations = []
            annotationNavigationSnapshot = null
            annotationBaselineRevision = -1
            applyTextDecorations()
            return
        }
        var snapshot = envelope.data
        if (Number(snapshot.textRevision) !== revision
                || revision !== Number(targetEditorItem.text_revision)) {
            annotationNavigationSnapshot = null
            annotationBaselineRevision = -1
            annotationRefreshTimer.restart()
            return
        }
        var spans = []
        var anchors = snapshot.anchors || []
        var bindings = snapshot.bindings || []
        for (var a = 0; a < anchors.length; ++a) {
            var anchor = anchors[a]
            for (var b = 0; b < bindings.length; ++b) {
                var binding = bindings[b]
                if (binding && binding.target && binding.target.kind === "textAnchor"
                        && String(binding.target.objectId) === String(anchor.anchorId)
                        && !binding.deletedAtMs) {
                    spans.push({ startByte: anchor.startByte, endByte: anchor.endByte,
                        severity: "highlight", textRevision: revision })
                    break
                }
            }
        }
        var links = snapshot.links || []
        for (var l = 0; l < links.length; ++l) {
            var source = links[l] ? links[l].source : null
            if (!source || source.kind !== "textAnchor") continue
            for (var ai = 0; ai < anchors.length; ++ai) {
                if (String(anchors[ai].anchorId) !== String(source.objectId)) continue
                spans.push({ startByte: anchors[ai].startByte, endByte: anchors[ai].endByte,
                    severity: "manualLink", textRevision: revision })
                break
            }
        }
        var autoMatches = snapshot.autoMatches || []
        for (var m = 0; m < autoMatches.length; ++m) {
            spans.push({ startByte: autoMatches[m].startByte, endByte: autoMatches[m].endByte,
                severity: "autoName", textRevision: revision })
        }
        annotationDecorations = spans
        annotationNavigationSnapshot = snapshot
        annotationBaselineRevision = revision
        annotationTrackedProjectId = projectId
        annotationTrackedChapterId = chapterId
        annotationTrackedRevision = revision
        annotationError = ""
        applyTextDecorations()
    }

    function annotationTargetLabel(target, namedEntries, matchedText) {
        if (!target) return qsTr("链接目标")
        if (target.externalUrl) return String(target.externalUrl)
        var ref = target.objectRef || target
        if (!ref || !ref.kind) return qsTr("链接目标")
        if (ref.kind === "namedEntry") {
            for (var i = 0; i < namedEntries.length; ++i) {
                var entry = namedEntries[i]
                if (entry && !entry.deletedAtMs
                        && String(entry.entryId) === String(ref.objectId))
                    return String(entry.canonicalName || matchedText || ref.objectId)
                        + " · " + String(ref.objectId).slice(0, 8)
            }
            return String(matchedText || qsTr("登记名称"))
                + " · " + String(ref.objectId || "").slice(0, 8)
        }
        var labels = ({ project: qsTr("作品"), volume: qsTr("分卷"), chapter: qsTr("章节"),
            textAnchor: qsTr("正文位置"), starMap: qsTr("星图"),
            starMapNode: qsTr("星图节点"), starMapEmbed: qsTr("星图嵌入") })
        return (labels[ref.kind] || String(ref.kind)) + " · " + String(ref.objectId || "")
    }

    function resolvedAnnotationLinkChoice(target, namedEntries, matchedText, origin) {
        var choice = {
            target: target,
            label: annotationTargetLabel(target, namedEntries, matchedText),
            matchedText: String(matchedText || ""),
            origin: String(origin || "manual"),
            state: "unavailable"
        }
        if (!annotationBackendRef || !annotationBackendRef.resolve_link_json) return choice
        var raw = annotationBackendRef.resolve_link_json(JSON.stringify(target))
        var envelope = null
        try { envelope = JSON.parse(String(raw || "{}")) } catch (e) {}
        if (envelope && envelope.success === true && envelope.data) {
            choice.state = String(envelope.data.state || "unavailable")
            choice.target = envelope.data.target || target
        }
        return choice
    }

    function annotationLinkChoicesAtByte(byteOffset) {
        var snapshot = annotationNavigationSnapshot
        if (!snapshot || !targetEditorItem || String(snapshot.projectId) !== String(projectId)
                || String(snapshot.chapterId) !== String(chapterId)
                || Number(snapshot.textRevision) !== Number(targetEditorItem.text_revision))
            return []
        var offset = Number(byteOffset)
        if (!isFinite(offset) || offset < 0) return []

        var anchorsById = ({})
        var anchors = snapshot.anchors || []
        for (var a = 0; a < anchors.length; ++a) {
            var anchor = anchors[a]
            if (anchor && !anchor.deletedAtMs)
                anchorsById[String(anchor.anchorId)] = anchor
        }

        // A manually created link always wins over an automatic name match at
        // the same text position. Multiple manual links on one anchor remain
        // separate choices.
        var manualTargets = []
        var links = snapshot.links || []
        for (var l = 0; l < links.length; ++l) {
            var link = links[l]
            if (!link || link.deletedAtMs || !link.source || link.source.kind !== "textAnchor") continue
            var linkedAnchor = anchorsById[String(link.source.objectId)]
            if (!linkedAnchor) continue
            var start = Number(linkedAnchor.startByte)
            var end = Number(linkedAnchor.endByte)
            if (offset < start || offset >= end) continue
            manualTargets.push(link.target)
        }

        var targets = []
        var matchedText = ""
        var origin = "manual"
        if (manualTargets.length > 0) {
            targets = manualTargets
        } else {
            origin = "auto"
            var matches = snapshot.autoMatches || []
            for (var m = 0; m < matches.length; ++m) {
                var match = matches[m]
                if (!match || offset < Number(match.startByte) || offset >= Number(match.endByte)) continue
                matchedText = String(match.matchedText || "")
                targets = match.candidates || []
                break
            }
        }

        if (targets.length === 0) return []
        var namedEntries = []
        if (annotationBackendRef && annotationBackendRef.list_named_entries_json) {
            var entriesEnvelope = null
            try { entriesEnvelope = JSON.parse(String(annotationBackendRef.list_named_entries_json(projectId) || "{}")) } catch (e) {}
            if (entriesEnvelope && entriesEnvelope.success === true && entriesEnvelope.data)
                namedEntries = entriesEnvelope.data
        }

        var choices = []
        var seen = ({})
        for (var t = 0; t < targets.length; ++t) {
            var rawTarget = targets[t]
            var target = rawTarget && rawTarget.objectRef !== undefined
                ? rawTarget : { objectRef: rawTarget, externalUrl: null }
            var key = JSON.stringify(target)
            if (seen[key]) continue
            seen[key] = true
            choices.push(resolvedAnnotationLinkChoice(target, namedEntries, matchedText, origin))
        }
        return choices
    }

    function activateAnnotationLinkAtByte(byteOffset) {
        var choices = annotationLinkChoicesAtByte(byteOffset)
        if (choices.length === 0) return false
        if (choices.length > 1) {
            annotationLinkChoicesRequested(choices)
            return true
        }
        activateAnnotationLinkChoice(choices[0])
        return true
    }

    function activateAnnotationLinkChoice(choice) {
        if (!choice) return false
        var state = String(choice.state || "unavailable")
        if (state === "projectNotDownloaded") {
            annotationLinkActivationFailed(qsTr("目标作品尚未下载"))
            return false
        }
        if (state === "missing") {
            annotationLinkActivationFailed(qsTr("链接目标已删除或不存在"))
            return false
        }
        if (state !== "exists" && state !== "externalUrl") {
            annotationLinkActivationFailed(qsTr("无法解析链接目标"))
            return false
        }
        var target = choice.target
        if (state === "externalUrl" && target && target.externalUrl) {
            Qt.openUrlExternally(String(target.externalUrl))
            return true
        }
        var ref = target ? (target.objectRef || target) : null
        if (!ref || !ref.kind) return false
        if (ref.kind === "textAnchor") {
            annotationTargetActivated(target)
            return true
        }
        annotationTargetActivated(target)
        return true
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
        proofBaselineText = content;
        proofBaselineSent = false;
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
