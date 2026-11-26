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
//   saveCurrentChapter() → readCommittedEditorSnapshot（一次取得同一 Core 已提交
//   版本的正文 + revision + 内容摘要）→ save_chapter；切章前先保存当前章节。
//
// 防死循环机制：
//   isLoadingChapter 标记防止 chapter_path_changed 信号重入
// =============================================================================

import QtQuick
import QtQuick.Controls

QtObject {
    id: controller

    signal emptySaveBlocked(string message)
    signal chapterSaveConflict(string message)
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
    // Issue #1052：保存必须与"编辑器实际载入的章节 + 同一 Core 已提交 revision"
    // 配对。旧实现只用 controller 的章节 ID 读即时正文；章节切换窗口里的旧
    // snapshot 可能被写进另一个章节，或把过期正文覆盖回当前章节。
    property string editorDocumentProjectId: ""
    property string editorDocumentVolumeId: ""
    property string editorDocumentChapterId: ""
    // 最近一笔编辑事务 signal 报告的 Core revision。保存快照的 revision 不得
    // 早于它，否则说明拿到的是旧版本正文。
    property double committedEditorRevision: -1
    // 最近一次成功落盘时对应的编辑器快照 revision。
    property double lastSavedEditorRevision: -1

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
    // Issue #1064：纠错调度只认「应用前台 + 已打开活动章节」。前台状态由
    // WritingWorkspace 从 main.qml 的写作会话前台定义注入；退后台只停表，
    // 不清任何待检查脏区。
    property bool applicationForeground: false
    // 当前章节是否仍有待检查区间。编辑事务只标脏，词典/规则变化重新标脏；
    // 只有一次真实扫描明确报告没有未检查段时才清零。
    property bool correctionDirty: false
    // 是否已经收到当前文档的有效快照。冷启动已全部有效时不得为一次空检查
    // 初始化模型。
    property bool correctionSnapshotKnown: false
    // 最近一次快照覆盖的区间数，仅用于面板进度展示。
    property int correctionCheckedRangeCount: 0
    // 最近一次真实扫描（restore-only 显示恢复不算）的完成时间。
    property double correctionLastCheckAtMs: 0
    property int correctionRequestSerial: 0
    property int pendingCorrectionRequestId: 0
    property double pendingCorrectionRevision: -1
    property string pendingCorrectionProjectId: ""
    property string pendingCorrectionChapterId: ""
    property string pendingCorrectionText: ""
    property bool pendingCorrectionSilent: false
    readonly property bool correctionScanRunning: correctionInFlightScanId > 0
    // Issue #1064：手动检查与 60s 周期检查共用 #1062 的单飞执行队列。
    // 扫描进行中只记录补跑标记，绝不并行提交第二笔模型任务。
    property int correctionInFlightScanId: 0
    property bool correctionPeriodicScanQueued: false
    property bool correctionManualScanQueued: false
    readonly property int correctionPeriodicIntervalMs: 60000
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
    property var correctionRequestKinds: ({})
    // Issue #1032 复审：标注映射也进入 Rust 后台固定队列，QML 只投递轻量
    // 编辑事实并接收按章节 + revision 返回的异步结果。这里只保存展示侧的
    // 请求标识；切章 / 重置时丢弃旧页面回调，但不会删除已提交的后台事实。
    property int annotationRequestSerial: 0
    property double annotationDocumentSessionId: 0
    property var annotationApplyRequests: ({})
    property var annotationRefreshRequests: ({})
    property int correctionApplyRequestSerial: 0
    property var correctionApplyRequests: ({})
    // Issue #1055：同一事件循环只排一次高度刷新；逐键各排一次会把
    // flush_content_height 和布局/绑定更新叠成多份。
    property bool contentHeightFlushScheduled: false
    // 同一正文 revision 已下发的装饰 JSON。标注与校对回调可能连续要求
    // 刷新同一份装饰，重复全量重建 native 几何只会拖慢光标反馈。
    property string lastAppliedDecorationJson: ""
    property double lastAppliedDecorationRevision: -1
    property var editorViewportRef: null
    // Issue #1059：当前编辑器已提交的章节持久化版本。保存时把它作为 base
    // 交给 Core；Core 发现磁盘版本已前进时返回 CHAPTER_VERSION_CONFLICT，
    // 不再用旧 buffer 无条件覆盖新正文。
    property string committedChapterGeneration: ""
    property double committedChapterRevision: 0
    property string committedChapterSha256: ""
    // Issue #1062 评论 #7187981523：本次文档正文是否已和一份完整的三元组
    // 同时安装。开章前先置 false，只有开章成功并写入 version 后才与
    // markEditorDocumentLoaded 同一时刻置 true。
    property bool chapterVersionInstalled: false

    // Issue #1059 评论 7160801242 第 4 项：正文保存不再同步阻塞 GUI。
    //
    // saveEditorSnapshotVersioned 只把不可变快照交给 Rust 单一后台队列并立刻
    // 返回 requestId；所有回执带 `文档身份 + 保存会话 + 快照 revision/sha256`，
    // 只有仍匹配当前文档时才应用。每次真正载入/清空正文都会开启新的保存会话，
    // 旧会话回执只能被丢弃，不能改写新会话的 base 或 buffer。
    property double chapterSaveSession: 0
    property var pendingChapterSaves: ({})
    property var pendingChapterSaveIds: []
    // Issue #1062 1.3：保存回执续接。requestId -> 回调；终态回执到达后由
    // consumeChapterSaveReceipt 取走并只调用一次，pending 不回调。
    property var chapterSaveContinuations: ({})
    // 切章 / 同步 / 关窗等边界的等待上限；超时只通知等待方失败，
    // 回执仍继续轮询并应用，避免旧 base 被永久丢掉。
    readonly property int chapterSaveReceiptTimeoutMs: 3000
    // 最近一次交给章节 actor 的草稿正文缓存：编辑器销毁后只能读这份值。
    property string cachedDraftText: ""
    property double cachedDraftRevision: 0
    property string cachedDraftSha256: ""
    property bool cachedDraftAvailable: false
    property bool chapterSaveConflictNotified: false

    property var annotationRefreshTimer: Timer {
        interval: 120
        repeat: false
        onTriggered: controller.refreshChapterAnnotations()
    }

    // Issue #1064：删除 250ms 输入停顿防抖。纠错扫描改为手动触发 + 前台
    // 活动章节每 60s 周期检查；连续输入、纯选区变化都不重置周期。定时器
    // 只在 chapterId 提交、回到前台时启动，切章/退后台/关闭时停止。
    property var correctionPeriodicTimer: Timer {
        interval: controller.correctionPeriodicIntervalMs
        repeat: true
        running: false
        onTriggered: controller.onCorrectionPeriodicTick()
    }

    // 章节身份提交（开章 / 清空）后本章重新起算首轮 60s；旧章的补跑标记
    // 属于旧文档，直接丢弃。
    onChapterIdChanged: {
        controller.correctionPeriodicScanQueued = false;
        controller.correctionManualScanQueued = false;
        controller.syncCorrectionPeriodicTimer(true);
    }

    // Issue #1064：回到前台只恢复 Core 缓存中的有效结果并重新起算周期，
    // 不立即跑模型；退后台停止本章定时器，待检查脏区保持原样。
    onApplicationForegroundChanged: {
        if (controller.applicationForeground) {
            controller.syncCorrectionPeriodicTimer(true);
            controller.requestCorrectionSnapshot();
        } else {
            controller.correctionPeriodicTimer.stop();
        }
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
            controller.queueCurrentChapterSave();
        }
    }

    // Issue #1059：后台保存回执轮询。worker 串行执行，QML 按 request id
    // 顺序应用回执；同一章节后一笔请求的 base 已由 Rust 队列用前一回执推进，
    // QML 不在这里改写 committedChapter*。
    property var chapterSavePollTimer: Timer {
        interval: 40
        repeat: true
        running: false
        onTriggered: controller.pollChapterSaveResults()
    }

    // Issue #1062 评论 #7187981523：保存 guard 活跃时同步前的 durable 落盘
    // 不再假装成功。等 guard 解除后重新走完整保存路径，期间只保留一个
    // continuation，避免调用方永久挂起。
    property var pendingSyncFlushContinuation: null

    property var syncFlushGuardTimer: Timer {
        interval: 50
        repeat: true
        running: false
        onTriggered: controller.pollPendingSyncFlush()
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
            // Issue #1062 1.3：一次 Core 快照同时供字数统计与可恢复草稿使用，
            // 草稿正文摘要直接取快照里的 contentSha256，不在 QML 自己算。
            var snapshot = controller.readCommittedEditorSnapshot("draft_autosave");
            var plainText = snapshot.ok ? snapshot.text : controller.getEditorPlainText();
            controller.editorBackendRef.calculate_word_count(plainText);
            if (snapshot.ok) controller.updateActiveChapterDraft(snapshot, false);
        }
    }

    // 手写证明链起点：章节加载时的完整正文。只在本次加载后的第一条编辑事务上报一次；
    // 后续事件由 Core 用链式 hash 承接，不重复传整章。
    property bool proofBaselineSent: false
    property string proofBaselineText: ""
    // 本次文档加载时的编辑器 revision（编辑器坐标，不是章节持久化 revision）；
    // 草稿证明水位只用编辑器坐标比较（Issue #1062 评论 7192111899 第 1 项）。
    property int chapterEditorBaseRevision: 0

    // NOTE: sync is NOT triggered by save. Auto-sync has been removed
    // (Issue #729). This is intentional: save ≠ sync.

    property var editorItemConnections: Connections {
        target: targetEditorItem ? targetEditorItem : null
        function onText_changed() {
            controller.handlePlainTextChanged();
            controller.scheduleContentHeightFlush();
        }
        // Issue #843: 编辑事务事实从 Rust EditorEditResult 直接传来，
        // cause + inserted/deleted 不再在 QML 中用文本长度 diff 猜。
        function onEditor_change_applied(cause, insertedChars, deletedChars,
                                         baseRevision, newRevision, editRangesJson,
                                         changesJson, oldSelectionAnchor, oldSelectionHead,
                                         newSelectionAnchor, newSelectionHead,
                                         beforeTextSha256, afterTextSha256) {
            // Issue #1052：只记录已提交 Core revision，用于保存快照的单调性校验。
            // 不再按删除字符数累计"删了多少"，保存合法性完全由同一时点快照
            // （正文 + revision + 内容摘要）证明，不做长度猜测。
            var appliedRevision = Number(newRevision)
            if (isFinite(appliedRevision)) {
                controller.committedEditorRevision = appliedRevision
            }
            controller.submitAnnotationEditFact({
                projectId: controller.projectId,
                chapterId: controller.chapterId,
                cause: cause,
                baseRevision: Number(baseRevision),
                newRevision: Number(newRevision),
                editRangesJson: String(editRangesJson || "[]")
            });
            if (controller.chapterId && controller.editorBackendRef) {
                // Issue #1062 1.3：编辑事实由后端同时持久化为可恢复草稿，
                // QML 不再维护待提交证明事件批次。
                controller.editorBackendRef.record_editor_change_stats(
                    controller.projectId, controller.volumeId, controller.chapterId,
                    cause, insertedChars, deletedChars, changesJson,
                    baseRevision, newRevision, oldSelectionAnchor, oldSelectionHead,
                    newSelectionAnchor, newSelectionHead,
                    controller.proofBaselineSent ? "" : controller.proofBaselineText,
                    !controller.proofBaselineSent,
                    beforeTextSha256, afterTextSha256,
                    Number(controller.chapterSaveSession), controller.committedChapterGeneration,
                    Number(controller.committedChapterRevision), controller.committedChapterSha256);
                if (changesJson && changesJson !== "[]") {
                    controller.proofBaselineSent = true;
                }
            }
        }
    }

    property var annotationBackendConnections: Connections {
        target: annotationBackendRef ? annotationBackendRef : null
        function onAnnotation_chapter_ready(requestId, documentSessionId,
                                            projectId, chapterId,
                                            textRevision, snapshotJson) {
            controller.receiveAnnotationSnapshot(
                requestId, documentSessionId, projectId, chapterId,
                textRevision, snapshotJson)
        }
        function onAnnotation_chapter_failed(requestId, documentSessionId,
                                             projectId, chapterId,
                                             textRevision, message) {
            controller.receiveAnnotationFailure(
                requestId, documentSessionId, projectId, chapterId,
                textRevision, message)
        }
        function onAnnotation_edits_applied(requestId, documentSessionId,
                                            projectId, chapterId,
                                            baseRevision, textRevision,
                                            success, message) {
            controller.receiveAnnotationEditsApplied(
                requestId, documentSessionId, projectId, chapterId,
                baseRevision, textRevision, success, message)
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
        function onCorrection_edits_applied(requestId, projectId, chapterId,
                                            textRevision, success, message) {
            controller.receiveCorrectionEditsApplied(
                requestId, projectId, chapterId, textRevision, success, message);
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

    // Issue #1032 复审：每次加载 / 重建正文 EditorKernel 都从 Rust 全局
    // 生成器领取新的 documentSessionId。不能复用 QML 页面内的数字 epoch，
    // 因为 Loader 销毁重建后计数会从头开始。
    function beginAnnotationDocumentSession() {
        var next = 0
        if (annotationBackendRef && annotationBackendRef.begin_annotation_document_session)
            next = Number(annotationBackendRef.begin_annotation_document_session())
        annotationDocumentSessionId = next
        clearAnnotationDisplayRequests()
        return next
    }

    // Issue #1032 复审：onEditor_change_applied 只投递不可变事实。标注更新
    // 进入 Rust 单线程业务队列，按项目 + 章节 + base/new revision 顺序消费；
    // 纠错 patch 继续进入现有 correction FIFO。这里不再用 Qt.callLater 代替
    // 业务队列，也不在 GUI 线程执行任何 annotation Core 调用。
    function submitAnnotationEditFact(fact) {
        if (!fact || !fact.projectId || !fact.chapterId) return;
        if (annotationBackendRef && annotationBackendRef.queue_annotation_edits
                && annotationDocumentSessionId > 0) {
            annotationRequestSerial += 1;
            if (annotationRequestSerial > 2147483000) annotationRequestSerial = 1;
            var requestId = annotationRequestSerial;
            annotationApplyRequests[requestId] = {
                sessionId: Number(annotationDocumentSessionId),
                projectId: String(fact.projectId),
                chapterId: String(fact.chapterId),
                baseRevision: Number(fact.baseRevision),
                revision: Number(fact.newRevision),
                epoch: correctionDocumentEpoch
            };
            annotationBackendRef.queue_annotation_edits(
                requestId,
                Number(annotationDocumentSessionId),
                String(fact.projectId),
                String(fact.chapterId),
                Number(fact.baseRevision),
                Number(fact.newRevision),
                String(fact.cause || "Typing"),
                String(fact.editRangesJson || "[]"));
        }
        // 纠错 apply 已是后台 FIFO；这里只负责投递 patch 事实。
        if (fact.projectId === controller.projectId
                && fact.chapterId === controller.chapterId) {
            reconcileCorrectionEdit(
                fact.baseRevision, fact.newRevision, fact.editRangesJson);
        }
    }

    function clearAnnotationDisplayRequests() {
        annotationApplyRequests = ({});
        annotationRefreshRequests = ({});
    }

    function cancelPendingCorrectionRequest() {
        correctionInFlightScanId = 0;
        correctionPeriodicScanQueued = false;
        correctionManualScanQueued = false;
        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        correctionCheckPending = false;
    }

    function registerCorrectionRequest(requestId, kind) {
        correctionRequestEpochs[requestId] = correctionDocumentEpoch;
        correctionRequestKinds[requestId] = kind;
    }

    function takeCorrectionRequestKind(requestId) {
        var kind = correctionRequestKinds[requestId];
        delete correctionRequestKinds[requestId];
        return kind;
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

    // Issue #1055：一次事件循环里的多次 text_changed 合并为一次高度刷新。
    // 逐键各排一次会让高度信号、绑定求值和布局在同一个事件循环里重复执行。
    function scheduleContentHeightFlush() {
        if (contentHeightFlushScheduled) return;
        contentHeightFlushScheduled = true;
        Qt.callLater(controller.flushContentHeightAfterEdit);
    }

    function flushContentHeightAfterEdit() {
        contentHeightFlushScheduled = false;
        if (!targetEditorItem || !targetEditorItem.flush_content_height) return;
        var startedAt = Date.now();
        targetEditorItem.flush_content_height();
        var elapsedMs = Date.now() - startedAt;
        if (elapsedMs >= 4) {
            logWriterInfo("editor.ui.content_height",
                "reason=text_changed, flush_ms=" + elapsedMs
                + ", revision=" + Number(targetEditorItem.text_revision));
        }
    }

    function applyTextDecorations() {
        if (!targetEditorItem || !targetEditorItem.set_text_decorations_json) return;
        var spans = proofreadingDecorations.concat(annotationDecorations);
        var decorationsJson = JSON.stringify(spans);
        var revision = Number(targetEditorItem.text_revision);
        // 标注与校对可能为同一 revision 连续要求刷新；native 几何已经一致时
        // 不再全量重建。
        if (decorationsJson === lastAppliedDecorationJson
                && revision === lastAppliedDecorationRevision) return;
        lastAppliedDecorationJson = decorationsJson;
        lastAppliedDecorationRevision = revision;
        var startedAt = Date.now();
        targetEditorItem.set_text_decorations_json(decorationsJson);
        var elapsedMs = Date.now() - startedAt;
        if (elapsedMs >= 2) {
            logWriterInfo("editor.ui.decorations",
                "reason=refresh, apply_ms=" + elapsedMs
                + ", spans=" + spans.length + ", revision=" + revision);
        }
    }

    // Issue #1032 复审：标注快照也由后台 API 生成。请求带当前章节、正文
    // revision 和文档 epoch；返回后只有仍匹配当前文档的最新请求才更新显示。
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
        if (annotationDocumentSessionId <= 0)
            beginAnnotationDocumentSession()
        var revision = Number(targetEditorItem.text_revision)
        var text = getEditorPlainText()
        annotationRequestSerial += 1
        if (annotationRequestSerial > 2147483000) annotationRequestSerial = 1
        var requestId = annotationRequestSerial
        annotationRefreshRequests = ({})
        annotationRefreshRequests[requestId] = {
            sessionId: Number(annotationDocumentSessionId),
            projectId: String(projectId),
            chapterId: String(chapterId),
            revision: revision,
            epoch: correctionDocumentEpoch
        }
        annotationBackendRef.request_chapter_annotations(
            requestId, Number(annotationDocumentSessionId),
            String(projectId), String(chapterId), revision, text)
    }

    function receiveAnnotationSnapshot(requestId, documentSessionId,
                                       projectId, chapterId,
                                       textRevision, snapshotJson) {
        var expected = annotationRefreshRequests[requestId]
        delete annotationRefreshRequests[requestId]
        if (!expected) return
        if (Number(expected.sessionId) !== Number(documentSessionId)
                || Number(annotationDocumentSessionId) !== Number(documentSessionId))
            return
        if (expected.projectId !== projectId || expected.chapterId !== chapterId
                || Number(expected.revision) !== Number(textRevision)
                || expected.epoch !== correctionDocumentEpoch
                || projectId !== controller.projectId
                || chapterId !== controller.chapterId)
            return
        var snapshot = null
        try { snapshot = JSON.parse(String(snapshotJson || "{}")) } catch (e) {}
        if (!snapshot) {
            annotationError = qsTr("正文标记响应无法读取")
            return
        }
        var revision = Number(snapshot.textRevision)
        if (revision !== Number(textRevision)
                || revision !== Number(targetEditorItem.text_revision)) {
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

    function receiveAnnotationFailure(requestId, documentSessionId,
                                      projectId, chapterId,
                                      textRevision, message) {
        var expected = annotationRefreshRequests[requestId]
        delete annotationRefreshRequests[requestId]
        if (!expected) return
        if (Number(expected.sessionId) !== Number(documentSessionId)
                || Number(annotationDocumentSessionId) !== Number(documentSessionId))
            return
        if (expected.projectId !== projectId
                || expected.chapterId !== chapterId
                || Number(expected.revision) !== Number(textRevision)
                || expected.epoch !== correctionDocumentEpoch
                || projectId !== controller.projectId
                || chapterId !== controller.chapterId)
            return
        annotationError = String(message || qsTr("正文标记暂不可用"))
        if (annotationNavigationSnapshot === null)
            applyTextDecorations()
    }

    function receiveAnnotationEditsApplied(requestId, documentSessionId,
                                          projectId, chapterId,
                                          baseRevision, textRevision,
                                          success, message) {
        var expected = annotationApplyRequests[requestId]
        delete annotationApplyRequests[requestId]
        if (!expected) return
        // 后台事实已经按章节完成；这里只决定是否更新当前页面的展示状态。
        if (Number(expected.sessionId) !== Number(documentSessionId)
                || Number(annotationDocumentSessionId) !== Number(documentSessionId))
            return
        if (expected.projectId !== projectId
                || expected.chapterId !== chapterId
                || Number(expected.baseRevision) !== Number(baseRevision)
                || expected.epoch !== correctionDocumentEpoch
                || projectId !== controller.projectId
                || chapterId !== controller.chapterId)
            return
        if (!success) {
            annotationError = String(message || qsTr("正文标记未能更新"))
            annotationRefreshTimer.restart()
            return
        }
        annotationError = ""
        annotationBaselineRevision = Math.max(
            Number(annotationBaselineRevision), Number(textRevision))
        annotationRefreshTimer.restart()
    }

    function annotationTargetLabel(target, namedEntries, matchedText) {
        if (!target) return qsTr("链接目标")
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
            starMapNode: qsTr("星图节点"), starMapEmbed: qsTr("星图嵌入"),
            nodeAsset: qsTr("星图节点"), nodeInstance: qsTr("星图节点实例"),
            embedInstance: qsTr("子星图") })
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
                ? rawTarget : { objectRef: rawTarget }
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
        if (state !== "exists") {
            annotationLinkActivationFailed(qsTr("无法解析链接目标"))
            return false
        }
        var target = choice.target
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
        clearAnnotationDisplayRequests();
        correctionApplyRequests = ({});
        correctionRequestEpochs = ({});
        correctionRequestKinds = ({});
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

    // markDirty=true 用于正文/词典/规则/模型已经变化的调用点：旧结果不能
    // 再当作当前文档的已校对状态，等待下一个周期或手动请求重新扫描。
    function invalidateCorrectionResults(markDirty) {
        cancelPendingCorrectionRequest();
        clearAnnotationDisplayRequests();
        correctionApplyRequests = ({});
        correctionDocumentEpoch += 1;
        if (correctionDocumentEpoch > 2147483000) correctionDocumentEpoch = 1;
        correctionRequestEpochs = ({});
        correctionRequestKinds = ({});
        correctionDirty = markDirty === true;
        correctionSnapshotKnown = markDirty === true;
        correctionCheckedRangeCount = 0;
        correctionLastCheckAtMs = 0;
        pendingCorrectionRevision = -1;
        pendingCorrectionProjectId = "";
        pendingCorrectionChapterId = "";
        pendingCorrectionText = "";
        pendingCorrectionSilent = false;
        updateCorrectionSuggestions([]);
        // 文档 identity / epoch 已变，旧的标注展示快照不能再挂在新正文上。
        annotationDecorations = [];
        annotationNavigationSnapshot = null;
        annotationBaselineRevision = -1;
        // native 装饰已随新文档清空，同一 JSON/revision 的短路缓存也必须失效。
        lastAppliedDecorationJson = "";
        lastAppliedDecorationRevision = -1;
        applyTextDecorations();
        // 新 documentEpoch 必须由后台重新建立 baseline；不能让旧周期的
        // revision 或 pending patch 被当成当前文档已应用。
        if (annotationBackendRef && targetEditorItem && projectId && chapterId)
            annotationRefreshTimer.restart();
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

    // Issue #1064：手动校对入口。复用 #1062 的单飞校对队列，立即提交一次
    // 当前已结算正文的检查；Core 只扫描脏区与尚未检查的段。正在扫描时
    // 不并行提交，只记下补跑请求，本轮终态回执后再跑一次。
    function requestCorrectionChapter() {
        return submitCorrectionScan("manual");
    }

    // 周期到点：仅前台活动章节计时；有待检查区间才提交，扫描进行中只置
    // pending 标记，不新开并行扫描。
    function onCorrectionPeriodicTick() {
        if (!applicationForeground || !chapterId) {
            correctionPeriodicTimer.stop();
            return;
        }
        if (correctionInFlightScanId > 0) {
            correctionPeriodicScanQueued = true;
            return;
        }
        // 进度未知（刚开章、恢复快照还没回来）时不能因为 dirty=false 就跳过：
        // 首次周期仍要提交，由 Core 判断是否真的没有待检查区间。
        if (correctionSnapshotKnown && !correctionDirty) return;
        submitCorrectionScan("periodic");
    }

    function syncCorrectionPeriodicTimer(restartFirstRound) {
        if (!applicationForeground || !chapterId) {
            correctionPeriodicTimer.stop();
            return;
        }
        if (restartFirstRound === true) correctionPeriodicTimer.restart();
        else if (!correctionPeriodicTimer.running) correctionPeriodicTimer.start();
    }

    function submitCorrectionScan(origin) {
        if (!projectId || !chapterId) {
            if (origin === "manual") correctionError = qsTr("请先打开一个章节。");
            return false;
        }
        if (!editorBackendRef || !targetEditorItem
                || !editorBackendRef.request_correction_pending) {
            if (origin === "manual") correctionError = qsTr("当前版本不支持校对。");
            return false;
        }
        if (!applicationForeground) return false;
        if (isLoadingChapter || isApplyingFormat || saveGuardActive()) return false;
        if (loadedEditorDocumentIdentity() !== editorDocumentIdentity()) return false;

        if (correctionInFlightScanId > 0) {
            if (origin === "manual") {
                correctionManualScanQueued = true;
                correctionStatus = qsTr("当前检查完成后将继续检查待校对区间…");
            } else {
                correctionPeriodicScanQueued = true;
            }
            return false;
        }

        // 冷启动已全部有效（或刚完成扫描且没有新脏区）时，不为一次空检查
        // 初始化模型。
        if (correctionSnapshotKnown && !correctionDirty) {
            if (origin === "manual") correctionStatus = qsTr("本章已全部校对。");
            return false;
        }

        var revision = Number(targetEditorItem.text_revision);
        if (!isFinite(revision) || revision < 0) {
            correctionError = qsTr("无法读取正文版本。");
            return false;
        }
        var snapshotText = getEditorPlainText();
        ensureCorrectionTracking(snapshotText, revision);

        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        var requestId = correctionRequestSerial;
        pendingCorrectionRequestId = requestId;
        registerCorrectionRequest(requestId, "pending");
        pendingCorrectionRevision = revision;
        pendingCorrectionProjectId = projectId;
        pendingCorrectionChapterId = chapterId;
        pendingCorrectionText = snapshotText;
        pendingCorrectionSilent = false;
        correctionError = "";
        correctionStatus = origin === "manual"
                ? qsTr("正在检查待校对区间…")
                : qsTr("正在后台检查待校对区间…");
        correctionCheckPending = true;
        correctionInFlightScanId = requestId;
        editorBackendRef.request_correction_pending(
            requestId,
            projectId,
            chapterId,
            correctionDocumentEpoch,
            revision,
            snapshotText);
        return true;
    }

    // 扫描终态后消费补跑标记：手动请求立即补跑一次；周期请求只在仍有
    // 脏区时补跑一次，正文移动过快时不追着连续扫。
    function runQueuedCorrectionScan() {
        if (correctionInFlightScanId > 0) return;
        if (!applicationForeground || !chapterId) return;
        if (correctionManualScanQueued) {
            correctionManualScanQueued = false;
            correctionPeriodicScanQueued = false;
            submitCorrectionScan("manual");
            return;
        }
        if (!correctionPeriodicScanQueued) return;
        correctionPeriodicScanQueued = false;
        if (correctionDirty) submitCorrectionScan("periodic");
    }

    // Issue #954/#1064：从 Core 缓存读取当前章节仍有效的校对结果，不重跑
    // 模型。开章、回前台、同步回流都只走这条 restore-only 路径；编辑事务
    // 连续性丢失时也只恢复显示，扫描交给周期或手动入口。
    function requestCorrectionSnapshot() {
        if (!editorBackendRef || !targetEditorItem || !projectId || !chapterId) return;
        if (!editorBackendRef.request_correction_snapshot) return;
        if (isLoadingChapter || isApplyingFormat || saveGuardActive()) return;
        // 已有真实扫描在途时，它的整章快照会覆盖显示，不再排一次 restore。
        if (correctionInFlightScanId > 0) return;
        // Issue #1055 复审：编辑事务连续性重建也会走到这里。编辑器实际载入的
        // 文档与 controller 当前身份不一致时，读到的正文可能已属于另一章，
        // 绝不能用旧 ID 投递。
        if (loadedEditorDocumentIdentity() !== editorDocumentIdentity()) {
            logWriterWarning("correction_snapshot_identity_mismatch",
                "loaded=" + loadedEditorDocumentIdentity()
                + ", current=" + editorDocumentIdentity());
            return;
        }

        var snapshotText = getEditorPlainText();
        var revision = Number(targetEditorItem.text_revision);
        if (!isFinite(revision) || revision < 0) return;
        ensureCorrectionTracking(snapshotText, revision);
        // Issue #1055：空白章节没有任何可恢复的波线，不必为它排一次后台
        // 校对任务；Core 的空章快照本来也是空建议。
        if (snapshotText.length === 0) {
            correctionCheckPending = false;
            pendingCorrectionRequestId = 0;
            pendingCorrectionRevision = -1;
            pendingCorrectionProjectId = "";
            pendingCorrectionChapterId = "";
            pendingCorrectionText = "";
            pendingCorrectionSilent = false;
            correctionSnapshotKnown = true;
            correctionDirty = false;
            correctionCheckedRangeCount = 0;
            updateCorrectionSuggestions([]);
            return;
        }

        correctionRequestSerial += 1;
        if (correctionRequestSerial > 2147483000) correctionRequestSerial = 1;
        pendingCorrectionRequestId = correctionRequestSerial;
        registerCorrectionRequest(pendingCorrectionRequestId, "restore");
        pendingCorrectionRevision = revision;
        pendingCorrectionProjectId = projectId;
        pendingCorrectionChapterId = chapterId;
        pendingCorrectionText = snapshotText;
        pendingCorrectionSilent = true;
        // 快照请求本身也要标记在途，避免连续性丢失时连续按键反复触发整章读取。
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
                throw new Error("invalid proofreading byte range");
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
                if (textIndex >= text.length) throw new Error("proofreading byte range exceeds text");
                var first = text.charCodeAt(textIndex);
                var codePoint = first;
                var units = 1;
                if (first >= 0xD800 && first <= 0xDBFF) {
                    if (textIndex + 1 >= text.length) throw new Error("text contains invalid UTF-16 surrogate");
                    var second = text.charCodeAt(textIndex + 1);
                    if (second < 0xDC00 || second > 0xDFFF) throw new Error("text contains invalid UTF-16 surrogate");
                    codePoint = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                    units = 2;
                } else if (first >= 0xDC00 && first <= 0xDFFF) {
                    throw new Error("text contains invalid UTF-16 surrogate");
                }
                var utf8Bytes = codePoint <= 0x7F ? 1
                              : codePoint <= 0x7FF ? 2
                              : codePoint <= 0xFFFF ? 3 : 4;
                byteOffset += utf8Bytes;
                utf16Offset += units;
                charOffset += 1;
                textIndex += units;
            }
            if (byteOffset !== endpoint.byte) throw new Error("proofreading position not on UTF-8 boundary");
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
                throw new Error("invalid proofreading byte range");
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
                throw new Error("proofreading source does not match byte range");
            }
        }
        return suggestions;
    }

    // Issue #954/#1064：编辑事务只做轻量段落位置映射与脏区标记，不读取/
    // 复制整章正文，也不触发扫描。模型阶段统一由 60s 周期或手动入口提交。
    function reconcileCorrectionEdit(baseRevision, newRevision, patchRangesJson) {
        if (!targetEditorItem || isLoadingChapter || isApplyingFormat) return;
        correctionDirty = true;
        var actualRevision = Number(targetEditorItem.text_revision);
        var base = Number(baseRevision);
        var next = Number(newRevision);

        // Issue #978 新评论: apply_edit_with_visuals 已在发出
        // editor_change_applied 前把 text_revision 推进到 next。
        // Issue #1032：事实改成逐条排队后，处理某条历史 patch 时编辑器可能已经
        // 走到更晚的 revision；只要 base 与已跟踪 revision 连续，就按队列顺序
        // 重放该 patch。actualRevision < next 才表示真的丢失了连续性。
        // Issue #1064：失去连续性时只恢复一次缓存显示，不再借 restore 回调
        // 立即补扫；脏区留给下一个周期或手动检查。
        if (!isFinite(base) || !isFinite(next) || actualRevision < next
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
        // 不复制整章正文。Issue #1032：QML 不再同步等待；Rust 校对队列按
        // 章节 FIFO 执行，完成结果通过 correction_edits_applied 带回。
        correctionApplyRequestSerial += 1;
        if (correctionApplyRequestSerial > 2147483000) correctionApplyRequestSerial = 1;
        var applyRequestId = correctionApplyRequestSerial;
        correctionApplyRequests[applyRequestId] = {
            projectId: projectId,
            chapterId: chapterId,
            revision: next,
            epoch: correctionDocumentEpoch
        };
        editorBackendRef.apply_correction_edits(
            applyRequestId,
            projectId,
            chapterId,
            correctionDocumentEpoch,
            next,
            base,
            String(patchRangesJson || "[]"));
    }

    function receiveCorrectionEditsApplied(requestId, projectId, chapterId,
                                          textRevision, success, errorMessage) {
        var expected = correctionApplyRequests[requestId];
        delete correctionApplyRequests[requestId];
        if (!expected) return;
        if (expected.projectId !== projectId
                || expected.chapterId !== chapterId
                || Number(expected.revision) !== Number(textRevision)
                || expected.epoch !== correctionDocumentEpoch
                || projectId !== controller.projectId
                || chapterId !== controller.chapterId) {
            return;
        }
        if (!success) {
            correctionDirty = true;
            if (!correctionCheckPending) requestCorrectionSnapshot();
            return;
        }
        if (Number(textRevision) < correctionTrackedRevision) return;
        // 编辑映射只推进轻量索引并保留脏区；扫描由周期或手动入口提交。
        correctionError = "";
        correctionDirty = true;
    }

    // Issue #954：Core 返回整章合并快照 { textRevision, suggestions, checkedRanges }，
    // 已按当前正文重新定位；QML 直接整体替换结果列表，不再本地合并。
    function receiveCorrectionSuggestions(requestId, textRevision, snapshotJson) {
        takeCorrectionRequestKind(requestId);
        var requestEpoch = takeCorrectionRequestEpoch(requestId);
        var scanFinished = requestId === correctionInFlightScanId;
        if (scanFinished) correctionInFlightScanId = 0;
        if (requestEpoch === undefined || requestEpoch !== correctionDocumentEpoch) {
            if (scanFinished) runQueuedCorrectionScan();
            return;
        }
        var isPending = requestId === pendingCorrectionRequestId;
        if (isPending || scanFinished) {
            correctionCheckPending = false;
        }
        // Request IDs only drive the pending indicator. Core always returns a
        // merged chapter snapshot; any response matching the current revision
        // and document is safe to display even if a newer request is in flight.
        if (!targetEditorItem
                || textRevision !== Number(targetEditorItem.text_revision)
                || projectId !== correctionTrackedProjectId
                || chapterId !== correctionTrackedChapterId) {
            if (scanFinished) {
                // 扫描期间正文又前进：本轮结果不能挂到新正文上，脏区保留，
                // 不追着移动中的正文连续扫，等下一个周期或手动请求。
                correctionDirty = true;
                correctionPeriodicScanQueued = false;
                correctionStatus = qsTr("正文已更新，本轮检查结果已跳过。");
                runQueuedCorrectionScan();
            }
            return;
        }
        var currentText = getEditorPlainText();
        try {
            var parsed = JSON.parse(String(snapshotJson));
            if (!parsed || typeof parsed !== "object"
                    || !Array.isArray(parsed.suggestions)
                    || Number(parsed.textRevision) !== textRevision) {
                throw new Error("invalid proofreading result format");
            }
            var suggestions = parsed.suggestions;
            for (var i = 0; i < suggestions.length; i += 1) {
                if (Number(suggestions[i].textRevision) !== textRevision) throw new Error("invalid proofreading text revision");
            }
            suggestions.sort(function(a, b) { return a.startByte - b.startByte; });
            annotateCorrectionPositions(currentText, suggestions);
            updateCorrectionSuggestions(suggestions);
            syncCorrectionTracking(currentText, textRevision);
            // Issue #1064：restore-only 只证明缓存结果可显示，不证明当前
            // revision 的待检查区间已经过模型；只有快照明确报告没有未检查段
            // 才把本章视为已校对。窗口中止/过期任务同样会带 true。
            var hasUnchecked = parsed.hasUncheckedSegments === true;
            correctionDirty = hasUnchecked;
            correctionSnapshotKnown = true;
            correctionCheckedRangeCount = Array.isArray(parsed.checkedRanges)
                    ? parsed.checkedRanges.length : 0;
            if (scanFinished) {
                correctionLastCheckAtMs = Date.now();
            }
            if (isPending) {
                correctionStatus = suggestions.length > 0
                        ? qsTr("发现 %1 条建议").arg(suggestions.length)
                        : qsTr("未发现建议。");
            }
        } catch (error) {
            if (isPending) {
                correctionError = String(error);
                correctionStatus = "";
            }
        }
        if (scanFinished) runQueuedCorrectionScan();
    }

    function receiveCorrectionFailure(requestId, textRevision, message) {
        takeCorrectionRequestKind(requestId);
        var requestEpoch = takeCorrectionRequestEpoch(requestId);
        var scanFinished = requestId === correctionInFlightScanId;
        if (scanFinished) correctionInFlightScanId = 0;
        if (requestEpoch === undefined || requestEpoch !== correctionDocumentEpoch) {
            if (scanFinished) runQueuedCorrectionScan();
            return;
        }
        if (requestId !== pendingCorrectionRequestId && !scanFinished) return;
        var silent = scanFinished ? false : pendingCorrectionSilent;
        correctionCheckPending = false;
        if (scanFinished) {
            correctionDirty = true;
            correctionLastCheckAtMs = Date.now();
        }
        if (silent) return;
        if (textRevision !== Number(targetEditorItem ? targetEditorItem.text_revision : -1)) {
            correctionStatus = qsTr("正文已变化，请重新校对。");
            if (scanFinished) runQueuedCorrectionScan();
            return;
        }
        correctionStatus = "";
        correctionError = String(message || qsTr("校对失败。"));
        if (scanFinished) runQueuedCorrectionScan();
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
            invalidateCorrectionResults(true);
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
            invalidateCorrectionResults(true);
            correctionStatus = qsTr("建议已过期或原文不匹配，请重新校对。");
            return false;
        }
        correctionStatus = qsTr("已应用建议。");
        updateCorrectionSuggestions(correctionSuggestions);
        return true;
    }

    function ignoreCorrection(suggestion, scope) {
        if (!suggestionIsCurrent(suggestion)) {
            invalidateCorrectionResults(true);
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
        if (scope === "rule") {
            // 停用规则会改变 scanner 指纹；沿用脏区标记，等周期或手动请求
            // 重新生成结果，不在忽略回调里直接扫描。
            correctionDirty = true;
            correctionSnapshotKnown = true;
        }
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
        // Issue #1064：词典变化只重新标脏，等 60s 周期或手动检查再扫描。
        invalidateCorrectionResults(true);
        correctionStatus = qsTr("已删除名称对应关系，将在下次检查时生效。");
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
        // Issue #1064：词典变化只重新标脏，等 60s 周期或手动检查再扫描。
        invalidateCorrectionResults(true);
        correctionStatus = kind === "alias"
                ? qsTr("已登记为同一名称的别名，将在下次检查时生效。")
                : qsTr("已登记为错误写法，将在下次检查时生效。");
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
        // The dictionary changed the scanner fingerprint. Invalidate the
        // visible results against the new protection set, but let the periodic
        // or manual scan rebuild them instead of scanning in this callback.
        invalidateCorrectionResults(true);
        correctionStatus = entryKind === "person"
                ? qsTr("已作为人物加入作品词典，将在下次检查时生效。")
                : entryKind === "place"
                  ? qsTr("已作为地点加入作品词典，将在下次检查时生效。")
                  : qsTr("已作为普通术语加入作品词典，将在下次检查时生效。");
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
        // 模型身份变化会让旧建议失效；只重新标脏，等周期或手动请求。
        invalidateCorrectionResults(true);
        correctionStatus = qsTr("模型路径已保存，将在下次检查时生效。");
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
        // 词库文件属于 scanner 指纹的一部分；只重新标脏，等周期或手动请求。
        invalidateCorrectionResults(true);
        correctionStatus = qsTr("已导入词库：%1，将在下次检查时生效。").arg(result.fileName || "");
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
        if (editorItemText.length === 0 && hadKnownEditorItemContent && hasRecentExplicitClearCandidate(true)) {
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

    // ── Issue #1052：保存与已提交事务 revision/章节身份配对 ──────────────────

    function editorDocumentIdentity() {
        return projectId + "|" + volumeId + "|" + chapterId;
    }

    function loadedEditorDocumentIdentity() {
        return editorDocumentProjectId + "|" + editorDocumentVolumeId + "|" + editorDocumentChapterId;
    }

    // 编辑器每次真正载入正文（load_document_text / reload_plain_text / 清空）后
    // 调用：把文档身份和 Core revision 基线绑定到同一份已提交正文。
    //
    // Issue #1059：同时开启新的保存会话并丢弃上一会话的待应用回执。旧请求的
    // 写盘仍由 Rust 队列完成（不会丢数据），但它不能再改写新文档的 base/buffer。
    // Issue #1062 1.3：上一会话仍在等待的 continuation 以失败回执收口，
    // 不能让调用方永久挂起。
    function markEditorDocumentLoaded(pId, vId, cId) {
        // Issue #1062 评论 #7187981523：版本与文档身份的原子性由开章入口
        // （openChapterContentNow）负责——它先校验并安装完整 version，再调用
        // 本函数。格式化/重载走的是同一章节的持久版本，不重新安装 version，
        // 因此这里不能以 chapterVersionInstalled 拒绝，否则会跳过会话重置。
        editorDocumentProjectId = pId || "";
        editorDocumentVolumeId = vId || "";
        editorDocumentChapterId = cId || "";
        var revision = targetEditorItem ? Number(targetEditorItem.text_revision) : -1;
        committedEditorRevision = revision;
        lastSavedEditorRevision = revision;
        chapterSaveSession += 1;
        chapterSaveConflictNotified = false;
        pendingChapterSaves = ({});
        pendingChapterSaveIds = [];
        abandonChapterSaveContinuations("editor_document_loaded");
        cachedDraftText = "";
        cachedDraftRevision = 0;
        cachedDraftSha256 = "";
        cachedDraftAvailable = false;
        chapterSavePollTimer.stop();
        return true;
    }

    // Issue #1051：跨卷移动了当前正在编辑的章节时，只迁移文档身份（卷 ID），
    // 不重载正文、不重置 revision 基线。否则保存快照的身份校验会拒绝落盘，
    // 且用旧卷 ID 保存会写到已不存在的路径。
    function rebindEditorVolume(pId, newVolumeId, cId) {
        editorDocumentProjectId = pId || "";
        editorDocumentVolumeId = newVolumeId || "";
        editorDocumentChapterId = cId || "";
    }

    function clearEditorDocumentIdentity() {
        editorDocumentProjectId = "";
        editorDocumentVolumeId = "";
        editorDocumentChapterId = "";
        committedEditorRevision = -1;
        lastSavedEditorRevision = -1;
    }

    // Issue #1052：保存边界唯一允许的正文来源。
    //
    // 只调用 targetEditorItem.get_save_snapshot() 一次：Rust 在同一次读取内
    // 给出同一 Core 已提交版本的正文、revision 和内容摘要（并已按 Core 权威
    // 摘要校验镜像正文，不一致时从 kernel 重建）。QML 不再分两次 get_plain_text
    // 拼快照，也不根据长度变化猜测正文是否缺失。
    //
    // 校验项：文档身份 = 编辑器实际载入章节；revision 与
    // authoritativeRevision 指向同一 Core 版本且不早于最近一笔编辑事务；
    // contentSha256 为 Core `content-v1:` 摘要。任何一项不成立即 ok=false，
    // 调用方必须拒绝保存并记录具体原因。
    // 返回 { ok, text, revision, contentSha256, suspiciousEmpty, reason }。
    function readCommittedEditorSnapshot(reason) {
        var snapshot = {
            "ok": false,
            "text": "",
            "revision": -1,
            "contentSha256": "",
            "suspiciousEmpty": false,
            "reason": String(reason || "")
        };
        if (!targetEditorItem) return snapshot;
        if (loadedEditorDocumentIdentity() !== editorDocumentIdentity()) {
            logWriterWarning("editor_snapshot_identity_mismatch",
                "reason=" + snapshot.reason
                + ", loaded=" + loadedEditorDocumentIdentity()
                + ", expected=" + editorDocumentIdentity());
            return snapshot;
        }
        if (!targetEditorItem.get_save_snapshot) {
            logWriterWarning("editor_snapshot_unavailable",
                "reason=" + snapshot.reason + ", missing=get_save_snapshot");
            return snapshot;
        }
        var raw = null;
        try {
            raw = targetEditorItem.get_save_snapshot();
        } catch (e) {
            logWriterWarning("editor_snapshot_read_failed",
                "reason=" + snapshot.reason + ", error=" + e);
            return snapshot;
        }
        var payload = null;
        try {
            payload = JSON.parse(String(raw === undefined || raw === null ? "" : raw));
        } catch (e) {
            logWriterWarning("editor_snapshot_parse_failed",
                "reason=" + snapshot.reason
                + ", rawLength=" + String(raw === undefined || raw === null ? "" : raw).length);
            return snapshot;
        }
        if (!payload || typeof payload.content !== "string"
                || typeof payload.textRevision !== "number"
                || typeof payload.authoritativeRevision !== "number"
                || typeof payload.contentSha256 !== "string") {
            logWriterWarning("editor_snapshot_invalid", "reason=" + snapshot.reason);
            return snapshot;
        }
        var revision = Number(payload.textRevision);
        var authoritativeRevision = Number(payload.authoritativeRevision);
        var contentSha256 = String(payload.contentSha256);
        if (!isFinite(revision) || revision < 0
                || !isFinite(authoritativeRevision) || authoritativeRevision < 0) {
            logWriterWarning("editor_snapshot_invalid",
                "reason=" + snapshot.reason
                + ", revision=" + payload.textRevision
                + ", authoritativeRevision=" + payload.authoritativeRevision);
            return snapshot;
        }
        if (revision !== authoritativeRevision) {
            logWriterWarning("editor_snapshot_revision_mismatch",
                "reason=" + snapshot.reason
                + ", revision=" + revision
                + ", authoritativeRevision=" + authoritativeRevision);
            return snapshot;
        }
        if (!/^content-v1:[0-9a-f]{64}$/.test(contentSha256)) {
            logWriterWarning("editor_snapshot_hash_invalid",
                "reason=" + snapshot.reason + ", revision=" + revision);
            return snapshot;
        }
        if (committedEditorRevision >= 0 && revision < committedEditorRevision) {
            logWriterWarning("editor_snapshot_stale",
                "reason=" + snapshot.reason
                + ", snapshotRevision=" + revision
                + ", committedRevision=" + committedEditorRevision);
            return snapshot;
        }
        var text = payload.content;
        snapshot.text = text;
        snapshot.revision = revision;
        snapshot.contentSha256 = contentSha256;
        snapshot.ok = true;
        var hadKnownEditorItemContent = lastSavedEditorText.length > 0;
        if (text.length === 0 && hadKnownEditorItemContent && hasRecentExplicitClearCandidate(true)) {
            explicitEmptySavePending = true;
        }
        snapshot.suspiciousEmpty = text.length === 0 && hadKnownEditorItemContent
                && !explicitEmptySavePending;
        return snapshot;
    }

    function logWriterInfo(event, message) {
        var msg = message || "";
        if (editorBackendRef && editorBackendRef.log_qml) {
            editorBackendRef.log_qml("info", "editor", event, msg);
        }
    }

    // Issue #1052：调用方已在同一份快照里知道正文是否为空，这里不再重读整章
    // 正文（保存快照只允许做一次全章读取）。只判断"刚发生显式清空候选"。
    function hasRecentExplicitClearCandidate(editorTextEmpty) {
        if (!targetEditorItem) return false;
        return editorTextEmpty === true && (Date.now() - lastPotentialExplicitClearAtMs) < 2000;
    }

    function markPotentialExplicitClear() {
        if (!targetEditorItem || saveGuardActive()) return;
        // Issue #1052：该函数在每个破坏性按键（Backspace/Delete/Ctrl+X）上调用。
        // 旧实现用 get_plain_text() 复制整章正文只为判断"是否接近空"，大章节
        // 连续删除会造成明显卡顿。改用 O(1) 的 plain_text_length / content_empty。
        var editorTextBytes = -1;
        if (targetEditorItem.plain_text_length !== undefined) {
            editorTextBytes = Number(targetEditorItem.plain_text_length);
        } else if (targetEditorItem.content_empty !== undefined) {
            editorTextBytes = targetEditorItem.content_empty === true ? 0 : 1;
        } else {
            editorTextBytes = readEditorItemPlainText().length;
        }
        // <=4 bytes 覆盖最多一个非 ASCII 字符（CJK 3 bytes / emoji 4 bytes），
        // 保持旧语义"只剩 <=1 个字符时视为显式清空候选"。
        var nearEmpty = editorTextBytes >= 0 && editorTextBytes <= 4;
        var hasSelection = targetEditorItem.has_selection === true;
        if (nearEmpty || hasSelection) {
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

    // Issue #1062 评论 #7187981523：guard 解除后重走一次完整 flush。
    // 没有 continuation 时停止轮询；guard 仍活跃则继续等。
    function pollPendingSyncFlush() {
        if (typeof pendingSyncFlushContinuation !== "function") {
            syncFlushGuardTimer.stop();
            return;
        }
        if (saveGuardActive()) return;
        syncFlushGuardTimer.stop();
        var continuation = pendingSyncFlushContinuation;
        pendingSyncFlushContinuation = null;
        flushActiveEditorBeforeSync(continuation);
    }

    // Issue #1062 1.3：同步前的正文 durable 落盘改为异步续接。
    // 成功 / 失败都通过 onDone(ok) 通知；失败原因写入 syncFlushFailureReason。
    function flushActiveEditorBeforeSync(onDone) {
        syncFlushFailureReason = ""
        var finished = typeof onDone === "function" ? onDone : null;
        if (!editorBackendRef || !chapterId || !projectId || !volumeId) {
            if (finished) finished(true);
            return;
        }
        if (saveGuardActive()) {
            // Issue #1062 评论 #7187981523：guard 期间没有 durable 回执，
            // 不能直接 finished(true)。保留 continuation，guard 解除后
            // 重新走快照 + 入队 + 回执；被新调用覆盖时旧的以失败收口。
            if (typeof pendingSyncFlushContinuation === "function") {
                var supersededContinuation = pendingSyncFlushContinuation;
                pendingSyncFlushContinuation = null;
                logWriterWarning("flush_before_sync_guard_superseded",
                    "reason=guard_active_new_request, identity=" + editorDocumentIdentity());
                supersededContinuation(false);
            }
            pendingSyncFlushContinuation = function(ok) {
                if (finished) finished(ok);
            };
            syncFlushGuardTimer.running = true;
            return;
        }
        // Issue #1052：同步前的落盘同样只消费同一时点的完整 Core 快照。
        var snapshot = readCommittedEditorSnapshot("flush_before_sync");
        if (!snapshot.ok) {
            syncFlushFailureReason = qsTr("编辑器正文快照与已提交版本不一致，已阻止保存")
            logWriterWarning("flush_before_sync_snapshot_rejected",
                "reason=" + snapshot.reason
                + ", identity=" + editorDocumentIdentity())
            if (finished) finished(false);
            return;
        }
        if (snapshot.suspiciousEmpty) {
            syncFlushFailureReason = qsTr("检测到可疑空正文，已阻止保存")
            logWriterWarning("flush_before_sync_blocked_empty", "blocked suspicious empty save before sync");
            if (finished) finished(false);
            return;
        }
        // Issue #1059：同步前必须有已落盘回执；“已入队”不等于已保存。
        var requestId = enqueueChapterSaveSnapshot(snapshot, "flush_before_sync",
            function(result) {
                var status = String(result && result.status ? result.status : "");
                if (status === "ok") {
                    if (finished) finished(true);
                    return;
                }
                // 不新增未翻译文案：pending/timeout 只在等不到回执时出现，细节进日志。
                if (status === "pending" || (result && result.timedOut === true)) {
                    syncFlushFailureReason = qsTr("保存当前章节失败")
                    logWriterWarning("flush_before_sync_pending",
                        "chapter save receipt timed out before sync")
                } else {
                    syncFlushFailureReason = result
                        ? (result.rawError || result.messageKey || result.errorCode
                           || qsTr("保存当前章节失败"))
                        : qsTr("保存当前章节失败")
                }
                logWriterWarning("flush_before_sync_failed", "save failed before sync, blocking sync");
                if (finished) finished(false);
            });
        if (requestId <= 0) {
            if (!syncFlushFailureReason) syncFlushFailureReason = qsTr("保存当前章节失败")
            logWriterWarning("flush_before_sync_failed", "save queue rejected snapshot");
            if (finished) finished(false);
        }
    }

    // Issue #1062 评论 #7187981523：保存 base 只能来自完整的持久化版本。
    // objectGeneration 允许任意非空字符串（不做格式判断）；revision 必须是
    // 合法非负整数，0 是合法版本；contentSha256 必须是字符串。
    function isCompleteChapterVersion(version) {
        if (!version || typeof version !== "object") return false;
        if (typeof version.objectGeneration !== "string") return false;
        if (typeof version.revision !== "number" || !Number.isInteger(version.revision)
                || version.revision < 0) return false;
        if (typeof version.contentSha256 !== "string") return false;
        return true;
    }

    // 只有完整版本才能写入 committedChapter*；返回值表示是否安装成功。
    function applyCommittedChapterVersion(version) {
        if (!isCompleteChapterVersion(version)) return false;
        committedChapterGeneration = version.objectGeneration;
        committedChapterRevision = version.revision;
        committedChapterSha256 = version.contentSha256;
        return true;
    }

    function committedChapterVersionMatches(version) {
        if (!version) return false;
        return String(version.objectGeneration || "") === committedChapterGeneration
                && Number(version.revision || 0) === committedChapterRevision
                && String(version.contentSha256 || "") === committedChapterSha256;
    }

    // Issue #1062 1.3：把正文快照交给章节 actor 维护可恢复草稿。
    // force=true 只用于保存 / 切章 / 离开作品 / 关窗等边界。
    // 内容摘要取 Core 快照的 contentSha256，不在 QML 侧重新计算。
    function updateActiveChapterDraft(snapshot, force) {
        if (!editorBackendRef || !editorBackendRef.update_chapter_draft_text) return false;
        if (!snapshot || snapshot.ok !== true) return false;
        if (!projectId || !chapterId) return false;
        if (loadedEditorDocumentIdentity() !== editorDocumentIdentity()) return false;
        editorBackendRef.update_chapter_draft_text(
            projectId, volumeId, chapterId, Number(chapterSaveSession),
            snapshot.text, Number(snapshot.revision), String(snapshot.contentSha256 || ""),
            committedChapterGeneration, Number(committedChapterRevision), committedChapterSha256,
            Number(chapterEditorBaseRevision), force === true);
        cacheChapterDraftSnapshot(snapshot);
        return true;
    }

    // Issue #1062 1.3：离开作品 / 切路由前用当前 Core 快照 force 刷新草稿。
    // update_chapter_draft_text 只向章节 actor 投递命令，不做 GUI 线程磁盘 I/O。
    function flushActiveChapterDraftNow() {
        var snapshot = readCommittedEditorSnapshot("leave_writing");
        if (!snapshot.ok) {
            logWriterWarning("chapter_draft_leave_skipped", "reason=" + snapshot.reason);
            return false;
        }
        return updateActiveChapterDraft(snapshot, true);
    }

    function cacheChapterDraftSnapshot(snapshot) {
        if (!snapshot || snapshot.ok !== true) return;
        cachedDraftText = snapshot.text;
        cachedDraftRevision = Number(snapshot.revision);
        cachedDraftSha256 = String(snapshot.contentSha256 || "");
        cachedDraftAvailable = true;
    }

    // Issue #1062 1.3：编辑器销毁 / 离开作品时只能读缓存值，不再访问
    // 可能已销毁的 SujianEditorItem；读不到就跳过并记日志。
    function flushCachedDraftBeforeUnload() {
        if (!editorBackendRef || !editorBackendRef.update_chapter_draft_text) return false;
        if (!projectId || !chapterId || !cachedDraftAvailable) {
            logWriterWarning("chapter_draft_unload_skipped",
                "reason=no_cached_draft, identity=" + editorDocumentIdentity());
            return false;
        }
        editorBackendRef.update_chapter_draft_text(
            projectId, volumeId, chapterId, Number(chapterSaveSession),
            cachedDraftText, Number(cachedDraftRevision),
            cachedDraftSha256, committedChapterGeneration,
            Number(committedChapterRevision), committedChapterSha256,
            Number(chapterEditorBaseRevision), true);
        return true;
    }

    // 保存成功后只清除 receipt 已确认的这批编辑，不清除随后输入的内容。
    function finishEditorSave(plainText, snapshotRevision, reason, snapshotSha) {
        lastSavedEditorText = plainText;
        previousEditorText = plainText;
        lastSavedEditorRevision = snapshotRevision;
        logWriterInfo("editor_save_committed",
            "reason=" + reason + ", revision=" + snapshotRevision
            + ", contentSha256=" + snapshotSha
            + ", len=" + plainText.length + ", identity=" + editorDocumentIdentity());
        if (plainText.length === 0) {
            explicitEmptySavePending = false;
            lastPotentialExplicitClearAtMs = 0;
        }
    }

    // Issue #1059：把不可变保存快照交给 Rust 后台有序提交队列，立刻返回。
    //
    // 不再同步调用 backend.save_chapter_versioned，也不读取 Core 回执：
    // 返回对象只表示“已入队”，committedChapter* 绝不在入队时改写。结果由
    // pollChapterSaveResults 通过回执应用。Issue #1062 1.3：证明事实由后端随
    // 编辑事实持久化，快照元数据不再携带证明事件批次。
    function saveEditorSnapshotVersioned(plainText, allowEmptyOverwrite, reason, snapshotRevision, snapshotSha) {
        if (!editorBackendRef || !editorBackendRef.enqueue_chapter_save_versioned) {
            return {"success": false, "queued": false, "errorCode": "SAVE_QUEUE_UNAVAILABLE"};
        }
        // 不可变快照元数据（正文 / base 正文单独传，避免整章 JSON 转义）。
        var meta = {
            "sessionId": Number(chapterSaveSession),
            "projectId": String(projectId),
            "volumeId": String(volumeId),
            "chapterId": String(chapterId),
            "allowEmptyOverwrite": allowEmptyOverwrite === true,
            "expectedGeneration": committedChapterGeneration,
            "expectedRevision": Number(committedChapterRevision),
            "expectedContentSha256": committedChapterSha256,
            "snapshotRevision": Number(snapshotRevision),
            "snapshotSha256": String(snapshotSha)
        };
        var requestId = Number(editorBackendRef.enqueue_chapter_save_versioned(
            JSON.stringify(meta), plainText, lastSavedEditorText));
        if (!isFinite(requestId) || requestId <= 0) {
            logWriterWarning("editor_save_queue_rejected",
                "reason=" + reason + ", requestId=" + requestId
                + ", identity=" + editorDocumentIdentity());
            return {"success": false, "queued": false, "errorCode": "SAVE_QUEUE_REJECTED"};
        }
        pendingChapterSaveIds.push(requestId);
        pendingChapterSaves[requestId] = {
            "projectId": String(projectId),
            "volumeId": String(volumeId),
            "chapterId": String(chapterId),
            "session": Number(chapterSaveSession),
            "snapshotRevision": Number(snapshotRevision),
            "snapshotSha256": String(snapshotSha),
            "text": plainText,
            "reason": String(reason || ""),
            "enqueuedAtMs": Date.now(),
            "timedOutNotified": false,
            "baseGeneration": committedChapterGeneration,
            "baseRevision": committedChapterRevision,
            "baseSha256": committedChapterSha256
        };
        chapterSavePollTimer.running = true;
        logWriterInfo("editor_save_enqueued",
            "requestId=" + requestId + ", reason=" + reason
            + ", revision=" + Number(snapshotRevision)
            + ", baseRevision=" + committedChapterRevision
            + ", identity=" + editorDocumentIdentity()
            + ", session=" + chapterSaveSession);
        // changedEntities 仍表示“本次保存最终会影响的实体”，真实回执另行返回。
        return {"success": true, "queued": true, "pending": true,
                "requestId": requestId, "changedEntities": ["ChapterContent"]};
    }

    // 回执必须同时匹配“原请求”和“当前文档 + 当前保存会话”。切章、重载、
    // 格式化、清空以及新的打开会话都会让旧回执失效；旧结果只能丢弃。
    // Issue #1062 1.3：整数回执字段用显式存在性 + 整数检查，0 是合法值，
    // 不能再用 `|| -1` 把 0 当缺失。
    function chapterSaveResultMatchesDocument(expected, result) {
        if (!expected || !result) return false;
        if (String(result.projectId || "") !== String(expected.projectId)
                || String(result.volumeId || "") !== String(expected.volumeId)
                || String(result.chapterId || "") !== String(expected.chapterId)) {
            return false;
        }
        if (typeof result.sessionId !== "number"
                || !Number.isInteger(result.sessionId)
                || result.sessionId < 0
                || result.sessionId !== Number(expected.session)) {
            return false;
        }
        if (typeof result.snapshotRevision !== "number"
                || !Number.isInteger(result.snapshotRevision)
                || result.snapshotRevision < 0
                || result.snapshotRevision !== Number(expected.snapshotRevision)) {
            return false;
        }
        if (String(result.snapshotSha256 || "") !== String(expected.snapshotSha256)) return false;
        // 当前 controller 文档身份与保存会话必须仍是同一个，且编辑器实际载入
        // 的章节没有被外层切换窗口改掉（Issue #1052 的身份校验继续生效）。
        if (Number(chapterSaveSession) !== Number(expected.session)) return false;
        if (String(expected.projectId) !== String(projectId)
                || String(expected.volumeId) !== String(volumeId)
                || String(expected.chapterId) !== String(chapterId)) {
            return false;
        }
        if (loadedEditorDocumentIdentity() !== editorDocumentIdentity()) return false;
        return true;
    }

    function takePendingChapterSave(requestId) {
        var index = pendingChapterSaveIds.indexOf(requestId);
        if (index >= 0) pendingChapterSaveIds.splice(index, 1);
        delete pendingChapterSaves[requestId];
    }

    // Issue #1062 1.3：保存回执的唯一收口。终态时先取走 continuation 再调用，
    // pending 不回调；重复调用或迟到回执不会二次触发。
    function consumeChapterSaveReceipt(requestId, result) {
        if (!result) return false;
        if (String(result.status || "") === "pending") return false;
        var key = String(requestId);
        var callback = chapterSaveContinuations[key];
        if (typeof callback !== "function") {
            delete chapterSaveContinuations[key];
            return false;
        }
        delete chapterSaveContinuations[key];
        callback(result);
        return true;
    }

    // 会话被重建 / 清空时，等待中的调用方必须收到失败回执而不能永久挂起。
    // 延迟到下一事件循环，避免在打开章节的调用栈里重入保存。
    function abandonChapterSaveContinuations(reason) {
        var keys = [];
        for (var key in chapterSaveContinuations) keys.push(String(key));
        for (var i = 0; i < keys.length; i += 1) {
            var requestKey = keys[i];
            var callback = chapterSaveContinuations[requestKey];
            delete chapterSaveContinuations[requestKey];
            if (typeof callback !== "function") continue;
            logWriterWarning("editor_save_continuation_abandoned",
                "requestId=" + requestKey + ", reason=" + reason);
            var abandonedResult = {
                "status": "error",
                "success": false,
                "terminal": true,
                "timedOut": false,
                "errorCode": "SAVE_SUPERSEDED",
                "requestId": Number(requestKey)
            };
            var invoke = (function(cb, value) {
                return function() { cb(value); };
            })(callback, abandonedResult);
            Qt.callLater(invoke);
        }
    }

    // 应用一条终态回执。只有文档/会话/快照全部匹配才推进 base 与
    // lastSavedEditorText；冲突与失败都不覆盖 buffer。
    function applyChapterSaveResult(requestId, expected, result) {
        var status = String(result.status || "error");
        if (status === "ok") {
            if (!chapterSaveResultMatchesDocument(expected, result)) {
                logWriterWarning("editor_save_receipt_dropped",
                    "requestId=" + requestId + ", reason=stale_document"
                    + ", expected=" + (expected
                        ? expected.projectId + "|" + expected.volumeId + "|" + expected.chapterId
                          + "|" + expected.session : "none")
                    + ", current=" + editorDocumentIdentity() + "|" + chapterSaveSession);
                return false;
            }
            var version = result.data ? result.data.version : null;
            if (version) {
                var nextGeneration = String(version.objectGeneration || "");
                var nextRevision = Number(version.revision || 0);
                // 同一 generation 只允许单调前进；generation 变化（迁移/重建）
                // 则以回执为准。
                if (nextGeneration !== committedChapterGeneration
                        || nextRevision >= committedChapterRevision) {
                    applyCommittedChapterVersion(version);
                }
            }
            chapterSaveConflictNotified = false;
            if (expected.snapshotRevision >= lastSavedEditorRevision) {
                finishEditorSave(expected.text, expected.snapshotRevision,
                    expected.reason, expected.snapshotSha256);
            }
            logWriterInfo("editor_save_committed_async",
                "requestId=" + requestId + ", reason=" + expected.reason
                + ", chained=" + (result.chained === true)
                + ", revision=" + committedChapterRevision
                + ", contentSha256=" + committedChapterSha256
                + ", len=" + expected.text.length
                + ", identity=" + editorDocumentIdentity()
                + ", session=" + chapterSaveSession);
            return true;
        }
        if (status === "pending") return false;
        var errorCode = String(result.errorCode || result.messageKey || "SAVE_ERROR");
        if (!chapterSaveResultMatchesDocument(expected, result)) {
            logWriterWarning("editor_save_result_dropped",
                "requestId=" + requestId + ", status=" + status
                + ", errorCode=" + errorCode + ", reason=stale_document");
            return false;
        }
        // CHAPTER_VERSION_CONFLICT：Core 不发布正文，本次快照正文已作为
        // app-meta/chapter-commits/conflicts/<tx>/incoming.md 持久化；这里
        // 保留编辑 buffer、不推进 base，只提示用户。
        if (errorCode === "CHAPTER_VERSION_CONFLICT") {
            logWriterWarning("editor_save_conflict_async",
                "requestId=" + requestId + ", reason=" + expected.reason
                + ", expectedRevision=" + committedChapterRevision
                + ", expectedSha256=" + committedChapterSha256
                + ", conflictPersisted=" + (result.conflictPersisted === true)
                + ", conflictDraftPath=" + String(result.conflictDraftPath || "")
                + ", identity=" + editorDocumentIdentity());
            if (!chapterSaveConflictNotified) {
                chapterSaveConflictNotified = true;
                controller.chapterSaveConflict(qsTr("磁盘上的章节已被更新，当前编辑未覆盖新版本。"));
            }
        } else if (errorCode === "EMPTY_OVERWRITE_BLOCKED") {
            logWriterWarning("empty_save_blocked", "blocked by core: " + errorCode);
            controller.emptySaveBlocked(qsTr("检测到异常空内容覆盖，已阻止保存。"));
        } else {
            logWriterWarning("editor_save_failed_async",
                "requestId=" + requestId + ", errorCode=" + errorCode
                + ", rawError=" + String(result.rawError || ""));
        }
        return false;
    }

    // 按入队顺序消费已完成的回执；未完成返回 pending 继续等下一轮。
    // Issue #1062 1.3：终态回执应用后交给 consumeChapterSaveReceipt 唤醒等待方。
    function pollChapterSaveResults() {
        if (!editorBackendRef || !editorBackendRef.poll_chapter_save_result) {
            chapterSavePollTimer.stop();
            abandonChapterSaveContinuations("save_poll_unavailable");
            return 0;
        }
        var applied = 0;
        var now = Date.now();
        var ids = pendingChapterSaveIds.slice(0);
        for (var i = 0; i < ids.length; i += 1) {
            var requestId = Number(ids[i]);
            var expected = pendingChapterSaves[requestId];
            if (!expected) {
                takePendingChapterSave(requestId);
                consumeChapterSaveReceipt(requestId, {
                    "status": "error", "success": false, "terminal": true,
                    "timedOut": false, "errorCode": "SAVE_STATE_LOST",
                    "requestId": requestId
                });
                continue;
            }
            var result = editorBackendRef.poll_chapter_save_result(
                requestId, projectId, volumeId, chapterId, chapterSaveSession);
            if (!result) continue;
            var status = String(result.status || "");
            if (status === "pending" || result.timedOut === true) {
                // 轮询本身不阻塞；超时只通知等待方失败，回执仍继续应用，
                // 后续请求的 base 仍由 Rust 队列推进。
                if (!expected.timedOutNotified
                        && typeof chapterSaveContinuations[String(requestId)] === "function"
                        && now - Number(expected.enqueuedAtMs || now)
                            >= chapterSaveReceiptTimeoutMs) {
                    expected.timedOutNotified = true;
                    logWriterWarning("editor_save_receipt_timeout",
                        "requestId=" + requestId + ", reason=" + expected.reason
                        + ", identity=" + editorDocumentIdentity());
                    consumeChapterSaveReceipt(requestId, {
                        "status": "error", "success": false, "terminal": true,
                        "timedOut": true, "errorCode": "SAVE_RECEIPT_TIMEOUT",
                        "requestId": requestId
                    });
                }
                continue;
            }
            if (status === "superseded") {
                // Issue #1062 1.1：同一会话更新的快照接替了本请求，正文与
                // 证明由接替者的回执落盘。把等待方续接到接替者；接替者已
                // 被消费时用显式失败让调用方重试，不能假装成功。
                var supersededBy = Number(result.supersededBy || 0);
                var supersededKey = String(requestId);
                var supersededCallback = chapterSaveContinuations[supersededKey];
                delete chapterSaveContinuations[supersededKey];
                takePendingChapterSave(requestId);
                applied += 1;
                logWriterInfo("editor_save_superseded",
                    "requestId=" + requestId + ", by=" + supersededBy
                    + ", reason=" + expected.reason);
                if (typeof supersededCallback === "function") {
                    var successorKey = String(supersededBy);
                    if (supersededBy > 0 && pendingChapterSaves[supersededBy]) {
                        var successorCallback = chapterSaveContinuations[successorKey];
                        if (typeof successorCallback === "function") {
                            chapterSaveContinuations[successorKey] = (function(first, second) {
                                return function(value) { first(value); second(value); };
                            })(successorCallback, supersededCallback);
                        } else {
                            chapterSaveContinuations[successorKey] = supersededCallback;
                        }
                    } else {
                        var supersededResult = {
                            "status": "error", "success": false, "terminal": true,
                            "timedOut": false, "errorCode": "SAVE_SUPERSEDED",
                            "requestId": requestId, "supersededBy": supersededBy
                        };
                        var invokeSuperseded = (function(cb, value) {
                            return function() { cb(value); };
                        })(supersededCallback, supersededResult);
                        Qt.callLater(invokeSuperseded);
                    }
                }
                continue;
            }
            takePendingChapterSave(requestId);
            applied += 1;
            applyChapterSaveResult(requestId, expected, result);
            consumeChapterSaveReceipt(requestId, result);
        }
        if (pendingChapterSaveIds.length === 0) chapterSavePollTimer.stop();
        return applied;
    }

    // Issue #1059：同步/外部写入后的按章版本协调。只有同一章节、本地没有
    // 未保存编辑且远端版本确实前进时才重载；dirty 时保留 buffer，交给用户
    // 或下一次保存按版本冲突处理。
    function reconcileChapterVersion(receipt) {
        if (!receipt || !receipt.chapterId) return false;
        if (receipt.projectId !== projectId
                || receipt.volumeId !== volumeId
                || receipt.chapterId !== chapterId) {
            return false;
        }
        if (!receipt.contentChanged && !receipt.deleted) return false;
        if (receipt.deleted) {
            var deletedRead = readEditorPlainText();
            var deletedText = deletedRead ? String(deletedRead.text || "") : "";
            if (deletedText !== lastSavedEditorText) {
                logWriterWarning("chapter_deleted_while_dirty",
                    "chapter=" + chapterId + ", kept_buffer=true");
                chapterSaveConflict(qsTr("当前章节已被删除，未保存的编辑已保留。"));
                return false;
            }
            clearActiveChapter();
            return true;
        }
        if (receipt.after && committedChapterVersionMatches(receipt.after)) {
            return false;
        }
        if (isLoadingChapter || saveGuardActive()) return false;
        var read = readEditorPlainText();
        var currentText = read ? String(read.text || "") : "";
        if (currentText !== lastSavedEditorText) {
            logWriterWarning("chapter_external_change_deferred",
                "reason=dirty_buffer, chapter=" + chapterId
                + ", committedSha256=" + committedChapterSha256
                + ", incomingSha256=" + (receipt.after ? receipt.after.contentSha256 : ""));
            return false;
        }
        return loadChapterContentWithIds(projectId, volumeId, chapterId) !== null;
    }

    // 没有逐章 receipt 的旧同步结果：先探版本，只有 clean 且版本前进才重载。
    function reconcileActiveChapterVersion() {
        if (!editorBackendRef || !projectId || !volumeId || !chapterId) return false;
        if (isLoadingChapter || saveGuardActive()) return false;
        var read = readEditorPlainText();
        var currentText = read ? String(read.text || "") : "";
        if (currentText !== lastSavedEditorText) return false;
        var result = editorBackendRef.read_chapter_version(projectId, volumeId, chapterId);
        if (!result || !result.success || !result.data || !result.data.version) return false;
        if (committedChapterVersionMatches(result.data.version)) return false;
        return loadChapterContentWithIds(projectId, volumeId, chapterId) !== null;
    }

    // Unified save entry — always writes the validated Core snapshot content.
    //
    // Issue #1052：保存正文必须来自同一时点的完整 Core 快照：
    // 1. 快照的文档身份必须等于编辑器实际载入的章节；
    // 2. 正文、revision、内容摘要来自同一次 get_save_snapshot，且 revision
    //    不早于最近一笔编辑事务；
    // 3. 不再根据正文长度变化猜测"丢字"——保存是否合法只由快照本身证明。
    //
    // Issue #1059：普通保存只入队，不阻塞 GUI。
    // Issue #1062 1.3：requestChapterSave 读快照、入队并登记 continuation，
    // 立即返回 requestId（0 表示未入队）；结果由轮询回执经
    // consumeChapterSaveReceipt 唤醒。durable 语义通过 onDone(ok, result)
    // 异步续接，GUI 线程不再同步等待。
    function requestChapterSave(reason, callback) {
        if (!editorBackendRef || !chapterId || !projectId || !volumeId) return 0;
        if (saveGuardActive()) {
            pendingAutoSaveAfterGuard = true;
            return 0;
        }

        var snapshot = readCommittedEditorSnapshot(reason);
        if (!snapshot.ok) {
            logWriterWarning("save_current_chapter_snapshot_rejected",
                "reason=" + snapshot.reason
                + ", identity=" + editorDocumentIdentity());
            return 0;
        }
        if (snapshot.suspiciousEmpty) {
            blockUnsafeEmptySave("save_current_chapter", {
                "editorItemLength": snapshot.text.length
            });
            return 0;
        }
        return enqueueChapterSaveSnapshot(snapshot, reason, callback);
    }

    // 快照入队 + continuation 登记的唯一实现。保存前先用 force=true 把
    // 最新正文写进可恢复草稿（保存 / 切章 / 同步 / 关窗边界）。
    function enqueueChapterSaveSnapshot(snapshot, reason, callback) {
        if (!snapshot || snapshot.ok !== true) return 0;
        updateActiveChapterDraft(snapshot, true);
        var plainText = snapshot.text;
        // 不能仅凭全文相等跳过：输入后还原会留下待提交编辑事实，正文相同
        // 仍需让 Core 建仅事件 journal 并确认事件 durable。
        var allowEmptyOverwrite = plainText.length === 0 && explicitEmptySavePending;
        var enqueued = saveEditorSnapshotVersioned(plainText, allowEmptyOverwrite,
            reason, snapshot.revision, snapshot.contentSha256);
        if (!enqueued || enqueued.queued !== true) return 0;
        var requestId = Number(enqueued.requestId);
        if (typeof callback === "function") {
            chapterSaveContinuations[String(requestId)] = callback;
        }
        return requestId;
    }

    // 显式保存入口：onDone(ok, result) 一定会被调用一次；立即失败时延后到
    // 下一事件循环，避免在调用栈里重入保存。
    function saveCurrentChapter(onDone) {
        var finished = typeof onDone === "function" ? onDone : null;
        var completed = false;
        var complete = function(ok, result) {
            if (completed) return;
            completed = true;
            if (finished) finished(ok, result);
        };
        var requestId = requestChapterSave("save_current_chapter", function(result) {
            complete(String(result && result.status ? result.status : "") === "ok", result);
        });
        if (requestId <= 0) {
            Qt.callLater(function() { complete(false, null); });
        }
        return requestId;
    }

    // 自动保存路径：只投递快照并立刻返回，结果由轮询回执应用。
    function queueCurrentChapterSave() {
        return requestChapterSave("autosave_queue");
    }

    // Load chapter: content comes from Rust core as plain text.
    //
    // Issue #1062 1.3：需要先保存当前章节时改为异步续接——保存回执 ok 后
    // 才打开新章；失败时写 save_status 并放弃本次切章，不冒险丢正文。
    // 返回：立即打开时返回 Core 结果对象；已排入保存回执时返回
    // {queued:true, requestId}；直接拒绝返回 null。onLoaded 恰好调用一次。
    function loadChapterContentWithIds(pId, vId, cId, onLoaded) {
        var notify = typeof onLoaded === "function" ? onLoaded : null;
        if (!cId || !pId || !vId || !editorBackendRef) return null;
        if (!targetEditorItem) return null;
        if (isLoadingChapter) return null;

        // Issue #1052：切到另一章节前必须先落盘当前章节的已提交正文。
        // 旧实现直接 open_chapter + load_document_text，自动保存防抖窗口内的
        // 未保存编辑会被新章节正文覆盖，而且不产生任何删除事务——正是诊断包里
        // "没有删除事务但正文变少"的形状。保存失败时阻止切章，不冒险丢正文。
        var switchingChapter = chapterId.length > 0
                && (pId !== projectId || vId !== volumeId || cId !== chapterId);
        if (!switchingChapter) {
            var immediate = openChapterContentNow(pId, vId, cId);
            if (notify) notify(immediate);
            return immediate;
        }

        var requestId = requestChapterSave("chapter_switch", function(result) {
            if (String(result && result.status ? result.status : "") !== "ok") {
                isLoadingChapter = false;
                logWriterWarning("chapter_switch_save_blocked",
                    "from=" + editorDocumentIdentity()
                    + ", to=" + pId + "|" + vId + "|" + cId
                    + ", status=" + String(result && result.status ? result.status : "")
                    + ", errorCode=" + String(result
                        && (result.errorCode || result.messageKey) || ""));
                if (editorBackendRef) {
                    editorBackendRef.save_status = qsTr("当前章节尚未成功保存，已阻止切换");
                }
                if (notify) notify(null);
                return;
            }
            var loaded = openChapterContentNow(pId, vId, cId);
            if (notify) notify(loaded);
        });
        if (requestId <= 0) {
            logWriterWarning("chapter_switch_save_blocked",
                "from=" + editorDocumentIdentity()
                + ", to=" + pId + "|" + vId + "|" + cId
                + ", saveQueueRejected=true");
            if (editorBackendRef) {
                editorBackendRef.save_status = qsTr("当前章节尚未成功保存，已阻止切换");
            }
            return null;
        }
        // 回执到达前继续保持加载 guard，阻止重复切章 / 保存。
        isLoadingChapter = true;
        return {"queued": true, "requestId": requestId};
    }

    // 开章失败 / 版本不完整时的统一收口：恢复上一章已提交三元组，不替换正文、
    // 不清 undo、不调用 markEditorDocumentLoaded，保证上一文档的原始保存状态仍在。
    function finishChapterOpenFailure(pId, vId, cId, reason, previousVersion, previousInstalled) {
        committedChapterGeneration = String(previousVersion.objectGeneration || "");
        committedChapterRevision = Number(previousVersion.revision || 0);
        committedChapterSha256 = String(previousVersion.contentSha256 || "");
        // 回滚到上一文档时，它原有的版本安装状态也必须一并恢复，否则下次
        // 开章前会被当成"新正文 + 未安装版本"。
        chapterVersionInstalled = previousInstalled === true;
        isLoadingChapter = false;
        chapterLoadFailed(pId, vId, cId, String(reason || "open_failed"));
        return null;
    }

    // 同步打开章节正文（不负责切章前的保存）。
    function openChapterContentNow(pId, vId, cId) {
        isLoadingChapter = true;

        // Issue #1062 评论 #7187981523：替换正文前先留住上一章版本用于失败
        // 回滚，并清空本章 base。任何失败都不能让上一章版本继续给本章当 base。
        var previousVersion = {
            "objectGeneration": committedChapterGeneration,
            "revision": committedChapterRevision,
            "contentSha256": committedChapterSha256
        };
        var previousInstalled = chapterVersionInstalled;
        committedChapterGeneration = "";
        committedChapterRevision = 0;
        committedChapterSha256 = "";
        chapterVersionInstalled = false;

        var result = editorBackendRef.open_chapter(pId, vId, cId);

        if (!result.success) {
            console.error("[SujianDebug] Failed to open chapter:", result.errorCode || result.rawError);
            return finishChapterOpenFailure(pId, vId, cId,
                result.rawError || result.errorCode || "open_failed", previousVersion, previousInstalled);
        }

        var loadedData = result.data || {};
        // Issue #1062 评论 #7187981523：版本必须在 load_document_text /
        // reload_plain_text 之前校验。缺字段时先失败，不能留下"新正文 + 空 base"。
        if (!isCompleteChapterVersion(loadedData.version)) {
            console.error("[SujianDebug] Open chapter returned incomplete version");
            return finishChapterOpenFailure(pId, vId, cId, "version_missing", previousVersion, previousInstalled);
        }

        // 打开文档绝不隐式改写正文：直接使用 Core 返回的纯文本。
        var content = result.data ? String(result.data.content || "") : "";

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
        // Issue #1052：正文载入后立刻把"编辑器实际载入的章节身份 + Core
        // revision"绑定成同一份已提交快照；外层 WritingWorkspace 稍后才更新
        // controller 的章节 ID，这段窗口内绝不允许旧快照落盘。
        // 身份取 Core 返回的权威 data.projectId/volumeId/chapterId（与
        // WritingWorkspace 提交给 controller 的值同源），避免请求参数与
        // authoritative 身份不一致导致所有保存被误判为过期快照。
        // Issue #1062 评论 #7187981523：先安装版本，再置 installed，最后与
        // markEditorDocumentLoaded 同一时刻完成，二者不可分开。
        if (!applyCommittedChapterVersion(loadedData.version)) {
            return finishChapterOpenFailure(pId, vId, cId, "version_missing", previousVersion);
        }
        chapterVersionInstalled = true;
        var loadedProjectId = String(loadedData.projectId || pId);
        var loadedVolumeId = String(loadedData.volumeId || vId);
        var loadedChapterId = String(loadedData.chapterId || cId);
        if (!markEditorDocumentLoaded(loadedProjectId, loadedVolumeId, loadedChapterId)) {
            chapterVersionInstalled = false;
            return finishChapterOpenFailure(pId, vId, cId, "version_not_installed", previousVersion, previousInstalled);
        }
        // 即使同一章节文本相同，新页面 / 新 EditorKernel 也是新的 Core
        // 标注会话；必须领取进程级唯一 ID，不能沿用旧页面的数字 epoch。
        beginAnnotationDocumentSession();

        previousEditorText = content;
        lastSavedEditorText = content;
        proofBaselineText = content;
        proofBaselineSent = false;
        chapterEditorBaseRevision = 0;
        explicitEmptySavePending = false;
        lastPotentialExplicitClearAtMs = 0;
        releaseCorrectionSession(controller.projectId, controller.chapterId);
        invalidateCorrectionResults();
        syncCorrectionTracking(content, Number(targetEditorItem.text_revision),
            loadedProjectId, loadedChapterId);
        refreshCorrectionEntityNames(pId);
        isLoadingChapter = false;
        // Issue #954/#1064：章节打开只恢复 Core 缓存中仍有效的校对结果，
        // 不立即加载模型；待检查区间交给开章 60s 后的周期或手动检查。
        // Issue #1055 复审：正文已经换成新章节，但 controller.projectId/volumeId/
        // chapterId 由外层 WritingWorkspace 在本次调用返回后才提交。直接调用会
        // 用新正文 + 旧章节 ID 投递快照，因此排到下一事件循环；身份或 epoch
        // 对不上（期间又切到 C）就丢弃这次恢复。
        var snapshotEpoch = correctionDocumentEpoch;
        Qt.callLater(function() {
            if (correctionDocumentEpoch !== snapshotEpoch) return;
            if (controller.projectId !== loadedProjectId
                    || controller.volumeId !== loadedVolumeId
                    || controller.chapterId !== loadedChapterId) return;
            if (loadedEditorDocumentIdentity() !== editorDocumentIdentity()) return;
            if (isLoadingChapter) return;
            requestCorrectionSnapshot();
        });

        // 成功信号由外层 WritingWorkspace 在章节身份、选中状态和视口都提交后发出。
        // 这里只负责载入正文，避免搜索定位回调在身份尚未更新时重入 openChapter。

        // Return full result so caller updates chapter state from authoritative source
        return result;
    }

    // Issue #754 评论 5815901258: 清空当前章节编辑器状态。
    // 用 isLoadingChapter 包住清理，避免清空正文被当成用户编辑触发保存。
    // 同步删除当前章节/分卷/作品后调用，使编辑器退回"请选择或新建章节"。
    function clearActiveChapter() {
        // Issue #1062 1.3：清空身份前把最新正文 force 写进可恢复草稿。
        var unloadSnapshot = readCommittedEditorSnapshot("clear_active_chapter");
        if (unloadSnapshot.ok) {
            updateActiveChapterDraft(unloadSnapshot, true);
        } else {
            logWriterWarning("chapter_draft_clear_skipped",
                "reason=" + unloadSnapshot.reason
                + ", identity=" + editorDocumentIdentity());
        }
        isLoadingChapter = true;
        try {
            releaseCorrectionSession(projectId, chapterId);
            if (targetEditorItem) {
                targetEditorItem.load_document_text("");
                beginAnnotationDocumentSession();
                targetEditorItem.clear_undo_stack();
            }
            projectId = "";
            volumeId = "";
            chapterId = "";
            chapterTitle = "";
            previousEditorText = "";
            lastSavedEditorText = "";
            committedChapterGeneration = "";
            committedChapterRevision = 0;
            committedChapterSha256 = "";
            chapterVersionInstalled = false;
            explicitEmptySavePending = false;
            lastPotentialExplicitClearAtMs = 0;
            // Issue #1059：清空文档即开启新的保存会话；未完成回执不再应用到
            // 空文档，旧请求的写盘仍由 Rust 队列完成。
            chapterSaveSession += 1;
            chapterSaveConflictNotified = false;
            pendingChapterSaves = ({});
            pendingChapterSaveIds = [];
            abandonChapterSaveContinuations("clear_active_chapter");
            cachedDraftText = "";
            cachedDraftRevision = 0;
            cachedDraftSha256 = "";
            cachedDraftAvailable = false;
            chapterSavePollTimer.stop();
            clearEditorDocumentIdentity();
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
        // Issue #1052：格式化读入的正文同样必须与已提交事务版本配对。
        var snapshot = readCommittedEditorSnapshot("format_text_read");
        if (!snapshot.ok) return;
        if (snapshot.suspiciousEmpty) {
            blockUnsafeEmptySave("format_text_read", {
                "editorItemLength": snapshot.text.length
            });
            return;
        }

        var raw = snapshot.text;
        if (!raw) return;
        if (!editorBackendRef || !editorBackendRef.format_plain_text) return;
        // 正文格式化规则归 Core：这里只触发后端业务操作、刷新编辑器快照并保存。
        // Issue #1052：格式化后的正文经 reload_plain_text 重建 EditorKernel，
        // 保存改由 markEditorDocumentLoaded 之后的同一时点快照提供，不再需要
        // "登记格式化删减字符数"来让长度守卫放行。
        var plain = editorBackendRef.format_plain_text(raw);

        isApplyingFormat = true;
        autoSaveTimer.stop();
        try {
            targetEditorItem.reload_plain_text(plain);
            beginAnnotationDocumentSession();
            targetEditorItem.clear_undo_stack();
            // reload 建立新 EditorKernel，revision 从新文档起点重新开始；重新
            // 绑定文档身份与已提交基线，避免旧 revision 被当成过期快照。
            markEditorDocumentLoaded(projectId, volumeId, chapterId);
        } finally {
            isApplyingFormat = false;
        }

        // 格式化重建了正文与 revision：旧结果失效并重新标脏，先恢复缓存显示，
        // 实际扫描等下一个周期或手动请求。
        invalidateCorrectionResults(true);
        syncCorrectionTracking(plain, Number(targetEditorItem.text_revision));
        requestCorrectionSnapshot();

        saveCurrentChapter();
    }
}
