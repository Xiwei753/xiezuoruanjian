// =============================================================================
// main.qml — 应用入口窗口
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：应用主窗口、全局路由管理、初始化 AppBackend、全局错误处理
// 约束：
//   - 不包含任何业务逻辑，所有操作委托给 AppBackend (Rust QObject)
//   - 页面切换通过 appController.route 状态驱动
//   - 不直接操作文件系统或 Core 层
//
// 调用链：main.qml → AppBackend (Rust QObject) → WriterCore (Core 层)
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import org.kde.kirigami as Kirigami
import Sujian 1.0

// Issue #900: 应用壳层迁移到 KDE Kirigami。QQC2 控件风格使用 KDE 桌面样式
// org.kde.desktop（Issue #933：Linux KDE/Plasma 下由已安装的 Breeze + Plasma
// Integration 自动选择，不再由 src/main.rs 强制指定）；此处用 Kirigami.ApplicationWindow
// 提供 pageStack/全局工具栏/主题桥。当前内容仍由既有 Loader 路由承载作品库/
// 写作/星图页面，避免用 Kirigami 页面切换动效驱动正文与光标。
Kirigami.ApplicationWindow {
    id: window
    visible: true
    // Issue #692 评论 5692612221: 默认逻辑尺寸 1100×768，窗口真正关联到屏幕后
    // 用当前窗口所属屏幕的 QScreen::availableGeometry()（已扣 KDE 面板/任务栏等
    // 窗口管理器保留区域）做一次性初始尺寸收口。QML 的 window.screen
    // （QQuickScreenInfo）只暴露 width/height（完整几何）和
    // desktopAvailableWidth/Height（虚拟桌面可用），都不合适：前者含任务栏，
    // 后者是所有屏幕合起来的可用尺寸，双屏混合 DPI 时限制等于没限制。
    // 因此通过 appBackend.available_screen_geometry_json() 桥接 Qt C++
    // QWindow::screen()->availableGeometry()，拿到当前屏幕扣保留区域后的可用
    // 尺寸做上限收口。窗口跨屏后的 DPR / fractional scaling 完全交给 Qt，
    // 不手工重算。
    width: 1100
    height: 768
    title: qsTr("素笺写作")
    // Issue #900：窗口底色跟随 KDE Breeze/系统主题（Kirigami.Theme），
    // 不再由 DesignTokens 决定通用界面外观；DesignTokens 只保留编辑器正文/
    // 星图等专用色彩的桥接职责。
    color: Kirigami.Theme.backgroundColor

    // Issue #912：深浅色以「实际显示的 KDE/Breeze 调色板」为准，而不是
    // Qt.styleHints.colorScheme。壳层底色来自 Kirigami.Theme，因此用它的
    // backgroundColor 与 textColor 亮度对比得到真实深浅；Kirigami.Theme 的
    // colorsChanged 会让这个绑定重新求值，从而驱动纸张与 system_is_dark 收口。
    //
    // 注意：显式深色/浅色由 themeController 通过 KDE ColorScheme 入口切换应用
    // 配色方案（见 sync_kde_color_scheme）。那时 Kirigami.Theme 反映的是应用
    // 选择而非桌面系统，因此只在「跟随系统」时才把它回写为 system_is_dark，
    // 避免从手动深色切回跟随系统时拿到错误的“系统深浅”。
    readonly property bool kdePaletteIsDark: colorLuminance(Kirigami.Theme.backgroundColor) < colorLuminance(Kirigami.Theme.textColor)

    // ── Issue #692 评论 5692612221: 初始窗口尺寸收口（一次性） ──
    // 等窗口真正关联到所属屏幕后，按当前屏幕 availableGeometry 收口默认尺寸。
    // initialWindowSizeApplied 之后不再跟随多屏切换强制改窗口大小；窗口跨屏后的
    // DPR / fractional scaling 完全交给 Qt。
    property bool initialWindowSizeApplied: false

    function applyInitialWindowSize() {
        if (initialWindowSizeApplied)
            return
        if (appBackend === null)
            return

        var geomJson = appBackend.available_screen_geometry_json()
        var geom = null
        try {
            geom = JSON.parse(geomJson)
        } catch (e) {
            geom = null
        }
        if (!geom || !geom.valid)
            return

        var aw = geom.width
        var ah = geom.height
        if (aw <= 0 || ah <= 0)
            return

        window.width = Math.min(1100, Math.max(320, aw))
        window.height = Math.min(768, Math.max(240, ah))
        initialWindowSizeApplied = true
    }

    onVisibleChanged: {
        if (visible && !initialWindowSizeApplied)
            Qt.callLater(applyInitialWindowSize)
    }

    onScreenChanged: {
        if (!initialWindowSizeApplied)
            Qt.callLater(applyInitialWindowSize)
    }

    // Issue #900: 移除 Material 全局覆写与手工 palette 注入，改用 KDE 桌面样式
    // （org.kde.desktop）的系统调色板。深浅色由 KDE 样式与系统 colorScheme 决定；
    // 应用自有主题 / 编辑区专用色仍由 designTokens 提供给编辑器与星图。
    // pageStack 仅作为 Kirigami 壳层存在，不使用其页面切换动画承载正文路由。
    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.None

    function reportNullBackend(name) {
        var message = "Required QML context property is null: " + name;
        console.error(message);
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("error", "app", "null_context_property", message);
        }
    }

    function verifyBackendRuntime() {
        if (appBackend === null) reportNullBackend("appBackend");
        if (workspaceBackend === null) reportNullBackend("workspaceBackend");
        if (projectBackend === null) reportNullBackend("projectBackend");
        if (editorBackend === null) reportNullBackend("editorBackend");
        if (settingsBackend === null) reportNullBackend("settingsBackend");
        if (syncBackend === null) reportNullBackend("syncBackend");
        if (starmapBackend === null) reportNullBackend("starmapBackend");
    }

    function debugLog(module, event, message) {
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("info", module, event, message);
        }
    }

    function debugWarn(module, event, message) {
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("warn", module, event, message);
        }
    }

    function debugError(module, event, message) {
        if (appBackend !== null && appBackend.debug_qml_enabled) {
            appBackend.log_qml("error", module, event, message);
        }
    }

    // Issue #981：正文页 + 应用/窗口前台才允许累计写作计时。离开正文、切应用、
    // 锁屏、最小化或隐藏窗口立即结束当前切片；回到前台只解除暂停，不补算离开时间。
    readonly property bool writingSessionApplicationActive: Qt.application.state === Qt.ApplicationActive
    readonly property bool writingSessionForeground: rootHasWorkspace
        && appController.inWriting
        && window.writingSessionApplicationActive
        && window.active
        && window.visibility !== Window.Minimized
        && window.visibility !== Window.Hidden

    function syncWritingSessionLifecycle() {
        if (!editorBackend) return
        if (!appController.inWriting) {
            editorBackend.close_writing_session("leftEditor")
            return
        }
        if (!window.writingSessionApplicationActive) {
            editorBackend.pause_writing_session("background")
        } else if (!window.active || window.visibility === Window.Minimized
                || window.visibility === Window.Hidden) {
            editorBackend.pause_writing_session("windowInactive")
        } else {
            editorBackend.resume_writing_session()
        }
    }

    onActiveChanged: syncWritingSessionLifecycle()
    onVisibilityChanged: syncWritingSessionLifecycle()
    onWritingSessionForegroundChanged: syncWritingSessionLifecycle()

    Connections {
        target: appController
        function onInWritingChanged() {
            window.syncWritingSessionLifecycle()
        }
    }

    // Issue #981：写作中每 30 秒把已经测量的前台切片交给 Core；没有输入也继续
    // 计时（思考时间不能因为“最近没按键”被扣掉）。
    Timer {
        id: writingSessionHeartbeatTimer
        interval: 30000
        repeat: true
        running: window.writingSessionForeground
        onTriggered: {
            if (editorBackend) editorBackend.writing_session_heartbeat()
        }
    }

    function preSyncBarrier() {
        manualSyncBarrierFailureReason = ""
        if (appController.inWriting && writingWorkspaceLoader.item) {
            if (!writingWorkspaceLoader.item.flushActiveEditorBeforeSync()) {
                manualSyncBarrierFailureReason = writingWorkspaceLoader.item.preSyncFlushFailureReason || ""
                return false
            }
        }
        if (editorBackend) {
            editorBackend.flush_recent_edits()
        }
        if (settingsBackend) {
            if (!settingsBackend.flush_pending_settings_save()) {
                manualSyncBarrierFailureReason = qsTr("保存应用设置失败")
                return false
            }
        }
        return true
    }

    // Issue #983：自动同步的唯一 QML 定时入口。QML 只负责每 30 秒唤醒一次；
    // 是否到期、工作区身份和单任务互斥全部由 SyncBackend/AppBackend 判断。
    // 真正启动沿用已有 perform_sync_internal("auto", ...)，不复用手动请求链路。
    property bool autoSyncShutdown: false
    property string autoSyncLastPreflightFailureReason: ""

    function logAutoSyncEvent(level, event, message) {
        if (!appBackend || typeof appBackend.log_qml !== "function") return
        try {
            appBackend.log_qml(level, "sync", event, message)
        } catch (e) {
            console.warn("Unable to log auto sync event", event, e)
        }
    }

    function maybeStartAutoSync(trigger) {
        if (autoSyncShutdown || !syncBackend || typeof syncBackend.auto_sync_due !== "function")
            return false
        var due = false
        try {
            due = syncBackend.auto_sync_due() === true
        } catch (e) {
            window.logAutoSyncEvent("warn", "auto_sync_due_failed",
                                    "trigger=" + trigger + ", error=" + e)
            return false
        }
        if (!due) return false

        var barrierPassed = false
        try {
            barrierPassed = window.preSyncBarrier()
        } catch (e) {
            barrierPassed = false
            window.manualSyncBarrierFailureReason = String(e)
        }
        if (!barrierPassed) {
            var reason = window.manualSyncBarrierFailureReason || qsTr("保存失败")
            // 同一失败原因不每 30 秒重复刷日志；原因变化时再记一次。
            if (reason !== autoSyncLastPreflightFailureReason) {
                autoSyncLastPreflightFailureReason = reason
                window.logAutoSyncEvent("warn", "auto_sync_preflight_failed",
                                        "trigger=" + trigger + ", reason=" + reason)
            }
            return false
        }
        autoSyncLastPreflightFailureReason = ""

        var operationId = ""
        try {
            operationId = String(syncBackend.start_auto_sync() || "").trim()
        } catch (e) {
            window.logAutoSyncEvent("warn", "auto_sync_dispatch_failed",
                                    "trigger=" + trigger + ", error=" + e)
            return false
        }
        if (operationId.length > 0) {
            window.logAutoSyncEvent("info", "auto_sync_started",
                                    "trigger=" + trigger + ", operation_id=" + operationId)
            return true
        }
        window.logAutoSyncEvent("info", "auto_sync_not_started",
                                "trigger=" + trigger + ", reason=backend_recheck")
        return false
    }

    // Issue #963：三个工作区顶栏共用这一条手动同步入口。先留下一条正常级别
    // 的操作日志（即使 QML debug 日志关闭也会进入诊断日志），再执行编辑保存屏障，
    // 最后按后端返回的操作 ID 跟踪同一条 sync_operation_state。
    property string manualSyncOperationId: ""
    // 最近一次手动同步对应的状态 ID（包含后端前置检查失败的请求）。
    // 只有 manualSyncOperationId 才表示已接受执行，用于顶栏反馈和终态追踪。
    property string manualSyncDisplayOperationId: ""
    property string manualSyncRequestId: ""
    property string manualSyncSource: ""
    property string manualSyncFeedbackText: ""
    property bool manualSyncFeedbackVisible: false
    property bool manualSyncFeedbackIsError: false
    property string manualSyncLastLoggedTerminalStatus: ""
    property int manualSyncRequestSerial: 0
    property string manualSyncBarrierFailureReason: ""

    function logManualSyncEvent(event, message) {
        if (!appBackend || typeof appBackend.log_qml !== "function") return
        try {
            appBackend.log_qml("info", "sync", event, message)
        } catch (e) {
            console.warn("Unable to log manual sync event", event, e)
        }
    }

    function isManualSyncOperationId(value) {
        return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value || "")
    }

    function manualSyncMessageForKey(key) {
        switch (key || "") {
        case "sync.block.no_workspace": return qsTr("未打开工作区")
        case "sync.block.no_workspace_layout": return qsTr("工作区同步布局尚未就绪")
        case "sync.block.disabled": return qsTr("同步已禁用")
        case "sync.block.remote_url_missing": return qsTr("未设置远程仓库地址")
        case "sync.block.token_missing": return qsTr("未设置访问 Token")
        case "sync.block.invalid_directory": return qsTr("工作区目录无效")
        case "sync.result.token_missing": return qsTr("未设置访问 Token")
        case "sync.result.token_invalid": return qsTr("GitHub Token 无效或已过期，请检查 Token 是否正确")
        case "sync.result.token_permission_denied": return qsTr("GitHub Token 权限不足，请为目标仓库授予 Contents 读写权限")
        case "sync.result.repo_not_found_or_no_permission": return qsTr("仓库不存在或无访问权限")
        case "sync.result.branch_missing": return qsTr("远程分支不存在")
        case "sync.result.network_failed": return qsTr("网络连接失败")
        case "sync.result.auth_failed": return qsTr("身份验证失败")
        case "sync.result.non_fast_forward": return qsTr("推送被拒绝：远端包含本地尚未同步的提交")
        case "sync.result.unrelated_histories": return qsTr("本地与远端仓库历史不相关，无法自动同步")
        case "sync.status.already_running": return qsTr("当前同步正在运行，本次请求已排队")
        case "sync.phase.syncing": return qsTr("正在同步")
        case "sync.phase.background_syncing": return qsTr("正在同步")
        case "sync.phase.github_init": return qsTr("正在初始化同步连接")
        case "sync.result.success_summary": return qsTr("同步完成")
        case "sync.result.latest_wins_summary": return qsTr("同步完成（已应用较新版本）")
        case "sync.result.no_changes_summary": return qsTr("无需同步，本地与远端一致")
        case "sync.result.conflict_summary": return qsTr("同步完成，存在冲突文件")
        case "sync.result.partial_conflict_summary": return qsTr("部分同步，仍有冲突待处理")
        case "sync.result.dirty_repo_blocked": return qsTr("同步被阻止：本地有未提交的更改")
        case "sync.result.branch_recovered_summary": return qsTr("分支已恢复并同步完成")
        case "sync.result.configured_not_tested": return qsTr("已配置，尚未测试")
        case "sync.result.generic_error": return qsTr("同步出错")
        case "error.sync_panic": return qsTr("同步过程异常终止")
        case "error.other": return qsTr("操作失败")
        default: return key || ""
        }
    }

    function parseManualSyncOperationState() {
        if (!syncBackend) return null
        var raw = syncBackend.sync_operation_state || ""
        if (raw === "") return null
        try { return JSON.parse(raw) } catch (e) { return null }
    }

    function manualSyncStateFeedback(state) {
        var status = state.status_code || ""
        var pending = syncBackend && syncBackend.manual_sync_pending
        var isRunning = status === "syncing"
        var knownSuccess = status === "success" || status === "no_changes"
            || status === "latest_wins_applied" || status === "branch_missing_recovered"
        var isError = !isRunning && !knownSuccess
        var lines = []

        if (isRunning) {
            lines.push(pending ? qsTr("已排队：当前同步结束后开始") : qsTr("正在同步"))
            if (!pending && state.phase_key) {
                var phase = manualSyncMessageForKey(state.phase_key)
                if (phase && phase !== lines[0]) lines.push(phase)
            }
        } else {
            var summary = manualSyncMessageForKey(state.summary_key)
            if (!summary) summary = knownSuccess ? qsTr("同步完成") : qsTr("同步失败")
            lines.push(summary)
            if (state.status_code && summary === state.summary_key)
                lines.push(qsTr("状态：%1").arg(state.status_code))
        }

        var failedTargets = state.summary_args ? state.summary_args.failed_targets : ""
        if (failedTargets) {
            var failedTargetCount = 0
            if (Array.isArray(failedTargets)) {
                failedTargetCount = failedTargets.length
                failedTargets = failedTargets.join("\n  - ")
            } else {
                failedTargets = String(failedTargets)
                failedTargetCount = failedTargets.split(/\n\s*-\s*/).filter(function(item) {
                    return item.trim().length > 0
                }).length
            }
            lines.push(qsTr("失败目标（%1）：\n%2").arg(failedTargetCount).arg(failedTargets))
        }
        var currentTarget = state.current_target
        if (isError && currentTarget && currentTarget.path)
            lines.push(qsTr("路径：%1").arg(currentTarget.path))
        if (state.raw_error)
            lines.push(qsTr("错误详情：%1").arg(state.raw_error))

        return { text: lines.join("\n"), isError: isError }
    }

    function refreshManualSyncFeedback() {
        if (!manualSyncOperationId) return
        var state = parseManualSyncOperationState()
        if (!state || state.operation_kind !== "sync"
                || state.operation_id !== manualSyncOperationId) return

        var feedback = manualSyncStateFeedback(state)
        manualSyncFeedbackText = feedback.text
        manualSyncFeedbackIsError = feedback.isError
        manualSyncFeedbackVisible = true

        var status = state.status_code || "unknown"
        if (status !== "syncing" && status !== manualSyncLastLoggedTerminalStatus) {
            manualSyncLastLoggedTerminalStatus = status
            logManualSyncEvent("manual_sync_result",
                "source=" + manualSyncSource
                    + " operation_id=" + manualSyncOperationId
                    + " status=" + status
                    + " phase=" + (state.phase_key || state.summary_key || ""))
        }
    }

    function showManualSyncNotStarted(message, event, operationId) {
        manualSyncOperationId = ""
        manualSyncDisplayOperationId = isManualSyncOperationId(operationId) ? operationId : ""
        manualSyncFeedbackText = qsTr("同步未开始：%1").arg(message)
        manualSyncFeedbackIsError = true
        manualSyncFeedbackVisible = true
        var trackedOperationId = isManualSyncOperationId(operationId) ? operationId : ""
        logManualSyncEvent(event,
            "source=" + manualSyncSource
                + " request_id=" + manualSyncRequestId
                + " operation_id=" + trackedOperationId)
        return manualSyncDisplayOperationId
    }

    function requestManualSync(source) {
        var validSources = ["starmap", "hub", "writing", "settings"]
        manualSyncSource = validSources.indexOf(source) >= 0 ? source : "unknown"
        manualSyncRequestSerial += 1
        manualSyncOperationId = ""
        manualSyncDisplayOperationId = ""
        manualSyncRequestId = ""
        manualSyncLastLoggedTerminalStatus = ""
        manualSyncFeedbackText = qsTr("正在准备同步…")
        manualSyncFeedbackIsError = false
        manualSyncFeedbackVisible = true

        if (!syncBackend)
            return showManualSyncNotStarted(qsTr("同步服务当前不可用"), "manual_sync_backend_unavailable")

        logManualSyncEvent("manual_sync_requested",
            "source=" + manualSyncSource + " phase=entry")

        try {
            manualSyncRequestId = String(syncBackend.record_manual_sync_request(manualSyncSource) || "").trim()
        } catch (e) {
            var recordReason = e && e.toString ? e.toString() : qsTr("无法记录同步请求")
            return showManualSyncNotStarted(recordReason, "manual_sync_request_record_failed")
        }
        if (!isManualSyncOperationId(manualSyncRequestId))
            return showManualSyncNotStarted(qsTr("未能取得有效的同步请求编号"), "manual_sync_request_record_failed")

        logManualSyncEvent("manual_sync_request_recorded",
            "source=" + manualSyncSource
                + " request_id=" + manualSyncRequestId
                + " phase=request_recorded")

        var barrierPassed = false
        try {
            barrierPassed = window.preSyncBarrier()
        } catch (e) {
            var exceptionReason = e && e.toString ? e.toString() : qsTr("编辑内容保存过程发生异常")
            return showManualSyncNotStarted(
                qsTr("当前编辑内容尚未保存。%1").arg(exceptionReason),
                "manual_sync_precondition_failed")
        }
        if (!barrierPassed) {
            var saveReason = manualSyncBarrierFailureReason
                || (editorBackend && editorBackend.save_status
                    ? String(editorBackend.save_status) : "")
            if (!manualSyncBarrierFailureReason
                    && !/失败|error|failed|阻止|拒绝|未保存/i.test(saveReason))
                saveReason = qsTr("编辑器未能保存当前章节")
            return showManualSyncNotStarted(
                qsTr("当前编辑内容尚未保存。原因：%1").arg(saveReason),
                "manual_sync_precondition_failed")
        }
        var returnedOperationId = ""
        try {
            returnedOperationId = String(syncBackend.perform_sync(manualSyncSource, manualSyncRequestId) || "").trim()
        } catch (e) {
            var callReason = e && e.toString ? e.toString() : qsTr("后端调用失败")
            return showManualSyncNotStarted(callReason, "manual_sync_backend_call_failed")
        }

        var state = parseManualSyncOperationState()
        var stateOperationId = state && state.operation_kind === "sync"
            && isManualSyncOperationId(state.operation_id) ? state.operation_id : ""
        if (isManualSyncOperationId(returnedOperationId))
            manualSyncOperationId = returnedOperationId
        else if (returnedOperationId === "" && stateOperationId
                && state.status_code === "syncing")
            manualSyncOperationId = stateOperationId

        if (!manualSyncOperationId) {
            var callError = null
            try { callError = JSON.parse(returnedOperationId) } catch (e) { callError = null }
            var errorKey = callError
                ? (callError.message_key || callError.messageKey || "") : ""
            var errorCode = callError
                ? (callError.error_code || callError.errorCode || "") : ""
            var reason = callError && (callError.raw_error || callError.rawError
                    || errorKey || errorCode || callError.message)
                ? String(callError.raw_error || callError.rawError
                          || manualSyncMessageForKey(errorKey)
                          || callError.message || errorCode)
                : qsTr("后端未返回有效的同步操作编号")
            return showManualSyncNotStarted(reason, "manual_sync_backend_call_failed")
        }

        manualSyncDisplayOperationId = manualSyncOperationId

        // perform_sync 会为前置校验失败也返回 operation_id。只有同一 ID 的
        // 状态确认为 Core 未启动、且当前没有排队/运行中的手动同步时，才将它
        // 反馈为“同步未开始”；不能把旧状态或其它操作的 busy 标记套到本次请求。
        var operationStateMatchesRequest = state && state.operation_kind === "sync"
            && state.operation_id === manualSyncOperationId
        var thisAttemptStarted = operationStateMatchesRequest
            && state.core_attempt_started === true
        var syncIsInProgress = Boolean(syncBackend.sync_in_progress)
        var manualSyncIsPending = Boolean(syncBackend.manual_sync_pending)
        var thisAttemptIsQueued = operationStateMatchesRequest
            && (state.status_code === "syncing" || manualSyncIsPending)
        if (operationStateMatchesRequest && state.status_code === "error"
                && state.core_attempt_started === false
                && !syncIsInProgress && !manualSyncIsPending) {
            var summary = manualSyncMessageForKey(state.summary_key)
            var rawError = state.raw_error ? String(state.raw_error) : ""
            var preflightReason = rawError && summary
                ? summary + "：" + rawError
                : (rawError || summary || qsTr("同步前置检查失败"))
            manualSyncOperationId = ""
            return showManualSyncNotStarted(
                preflightReason, "manual_sync_preflight_failed", manualSyncDisplayOperationId)
        }
        if (!operationStateMatchesRequest
                || (!thisAttemptStarted && !thisAttemptIsQueued && !syncIsInProgress)) {
            var unconfirmedReason = operationStateMatchesRequest
                ? (state.raw_error || manualSyncMessageForKey(state.summary_key)
                    || qsTr("未能确认本次同步已启动或排队"))
                : qsTr("未能确认本次同步状态")
            return showManualSyncNotStarted(
                unconfirmedReason, "manual_sync_state_unconfirmed",
                operationStateMatchesRequest ? manualSyncDisplayOperationId : "")
        }

        logManualSyncEvent("manual_sync_accepted",
            "source=" + manualSyncSource
                + " request_id=" + manualSyncRequestId
                + " operation_id=" + manualSyncOperationId)
        manualSyncFeedbackText = qsTr("同步请求已接收，正在确认执行状态")
        manualSyncFeedbackVisible = true
        refreshManualSyncFeedback()
        return manualSyncOperationId
    }

    function openSettingsDialog(section, settingKey) {
        if (!settingsDialogLoader.active) {
            settingsDialogLoader.active = true;
        }
        if (settingsDialogLoader.item) {
            settingsDialogLoader.item.open();
            if (section || settingKey) {
                Qt.callLater(function() {
                    var dialog = settingsDialogLoader.item
                    if (dialog && dialog.revealSection) {
                        dialog.revealSection(String(section || ""), String(settingKey || ""))
                    }
                })
            }
        }
    }

    // Issue #762 评论 5826175490 第 4 点：从 SyncPage 全局冲突入口跳到具体作品。
    // 待选中的冲突路径先记在这里，等 WritingWorkspace 实例化后再交给它——
    // 用户可能在 hub/设置页触发，此时 writingWorkspaceLoader.item 还不存在。
    // Issue #770 评论 5842877986: 跨作品点冲突时不能只缓存 path，还要缓存 projectId，
    // 否则项目没切到位就消费 pending，会落到错误作品的冲突列表里。
    property string pendingConflictProjectId: ""
    property string pendingConflictPath: ""
    // Issue #902 第二十轮：pending 冲突的 target 类型，只处理 project target。
    property string pendingConflictTargetKind: ""

    function projectTitleById(projectId) {
        var treeData = appState ? (appState.tree || []) : []
        for (var i = 0; i < treeData.length; i++) {
            if (treeData[i].type === "project" && treeData[i].id === projectId) {
                return treeData[i].title || ""
            }
        }
        return ""
    }

    function applyPendingConflictPath() {
        if (!pendingConflictPath) return
        var workspace = writingWorkspaceLoader.item
        if (!workspace) return
        // Issue #770 评论 5842877986: 只有项目真正切到位才消费 pending，
        // 否则保留等 projectReady 信号到来再消费。
        if (workspace.workspaceProjectId !== pendingConflictProjectId) return
        workspace.openConflictPath(pendingConflictPath)
        pendingConflictPath = ""
        pendingConflictProjectId = ""
    }

    // ── Issue #982：全局搜索入口与跨页面定位 ──────────────────────────────
    // 搜索本身由常驻 worker 上的 Core 索引完成；这里只负责收集当前页面上下文、
    // 应用保存屏障，以及把点击结果交给现有路由和编辑器定位。

    function openGlobalSearch(context) {
        // 打开搜索前先收口当前编辑内容，让 Core 索引包含刚落盘的正文；
        // 保存失败不阻断搜索，只保留后端给出的保存状态。
        try {
            window.preSyncBarrier()
        } catch (e) {
            window.debugLog("search", "pre_search_flush_failed", String(e))
        }

        window.searchNavigationSerial += 1
        window.cancelPendingSearchNavigation("")

        var selected = window.appState && window.appState.selected
            ? window.appState.selected : ({})
        var origin = "general"
        if (settingsDialogLoader.item && settingsDialogLoader.item.visible) {
            origin = "settings"
        } else if (appController.inWriting) {
            origin = "writing"
        } else if (appController.inStarmap) {
            origin = "starmap"
        } else if (window.rootHasWorkspace) {
            origin = "home"
        }

        var ctx = context || ({})
        if (!ctx.originScreen) ctx.originScreen = origin
        if (!ctx.currentProjectId) {
            ctx.currentProjectId = appController.writingProjectId
                || selected.projectId || ""
        }
        if (!ctx.currentVolumeId) ctx.currentVolumeId = selected.volumeId || ""
        if (!ctx.currentChapterId) ctx.currentChapterId = selected.chapterId || ""
        if (!ctx.currentStarmapId) ctx.currentStarmapId = appController.starmapId || ""

        window.globalSearchContext = ctx
        window.globalSearchVisible = true
        globalSearchOverlay.open(ctx)
        window.debugLog("search", "open", "origin=" + ctx.originScreen)
    }

    function searchTargetExists(result) {
        if (!result || !result.target) return false
        var target = result.target
        // 星图和设置是全局数据，不依赖当前打开的作品树；绑定星图带 projectId
        // 也不能因此被误判为「已删除」。
        if (target.starmapId || target.settingKey || target.settingSection) return true
        if (!target.projectId) return true

        var tree = window.appState && window.appState.tree
            ? window.appState.tree : []
        var projectFound = false
        var volumeFound = !target.volumeId
        var chapterFound = !target.chapterId
        for (var i = 0; i < tree.length; i += 1) {
            var node = tree[i]
            if (!node) continue
            if (node.type === "project" && node.id === target.projectId) {
                projectFound = true
            } else if (node.type === "volume"
                       && node.id === target.volumeId
                       && node.projectId === target.projectId) {
                volumeFound = true
            } else if (node.type === "chapter"
                       && node.id === target.chapterId
                       && node.volumeId === target.volumeId
                       && node.projectId === target.projectId) {
                chapterFound = true
            }
        }
        if (target.chapterId) return projectFound && volumeFound && chapterFound
        if (target.volumeId) return projectFound && volumeFound
        return projectFound
    }

    function searchPathTail(path) {
        var parts = String(path || "").split(" / ")
        return parts.length > 0 ? parts[parts.length - 1] : ""
    }

    function currentWorkspaceGeneration() {
        return workspaceBackend ? workspaceBackend.workspace_generation_value : 0
    }

    function cancelPendingSearchNavigation(message) {
        window.pendingSearchReveal = null
        window.pendingStarmapSearchTarget = null
        if (message && globalSearchOverlay.visible) {
            globalSearchOverlay.showMessage(message)
        }
    }

    function schedulePendingSearchReveal(pId, vId, cId, startByte, endByte) {
        window.pendingStarmapSearchTarget = null
        window.pendingSearchReveal = {
            projectId: pId,
            volumeId: vId,
            chapterId: cId,
            startByte: Number(startByte),
            endByte: Number(endByte),
            workspaceGeneration: window.currentWorkspaceGeneration(),
            serial: window.searchNavigationSerial
        }
        window.applyPendingSearchReveal()
    }

    function applyPendingSearchReveal() {
        var pending = window.pendingSearchReveal
        if (!pending) return false
        if (pending.serial !== window.searchNavigationSerial) {
            window.pendingSearchReveal = null
            return false
        }
        var workspace = writingWorkspaceLoader.item
        if (!workspace || !workspace.requestSearchReveal) return false
        var revealed = workspace.requestSearchReveal(
            pending.projectId,
            pending.volumeId,
            pending.chapterId,
            pending.startByte,
            pending.endByte
        )
        if (revealed) {
            window.pendingSearchReveal = null
            // 只有正文真正选中并滚动后才关闭搜索；打开章节本身不算成功。
            if (globalSearchOverlay.visible) globalSearchOverlay.close()
        }
        return revealed
    }

    function handleWritingWorkspaceReady() {
        var pending = window.pendingSearchReveal
        if (!pending || pending.serial !== window.searchNavigationSerial) return
        var workspace = writingWorkspaceLoader.item
        if (!workspace) return
        if (workspace.workspaceProjectId
                && pending.projectId
                && workspace.workspaceProjectId !== pending.projectId) {
            window.cancelPendingSearchNavigation(qsTr("无法打开目标作品，请重新搜索"))
            return
        }
        // 工作区已切到目标作品，记录新的 generation 后再尝试打开/定位章节。
        pending.workspaceGeneration = window.currentWorkspaceGeneration()
        window.applyPendingSearchReveal()
    }

    function handleSearchChapterLoaded(pId, vId, cId) {
        var pending = window.pendingSearchReveal
        if (!pending || pending.serial !== window.searchNavigationSerial) return
        if (pending.projectId !== pId
                || pending.volumeId !== vId
                || pending.chapterId !== cId) return
        pending.workspaceGeneration = window.currentWorkspaceGeneration()
        window.applyPendingSearchReveal()
    }

    function handleSearchChapterLoadFailed(pId, vId, cId, reason) {
        var pending = window.pendingSearchReveal
        if (!pending || pending.serial !== window.searchNavigationSerial) return
        if (pending.projectId !== pId
                || pending.volumeId !== vId
                || pending.chapterId !== cId) return
        window.cancelPendingSearchNavigation(
            qsTr("无法打开目标章节：%1").arg(String(reason || qsTr("未知错误")))
        )
        window.debugLog("search", "chapter_reveal_failed", String(reason || ""))
    }

    function schedulePendingStarmapSearchTarget(target) {
        window.pendingSearchReveal = null
        window.pendingStarmapSearchTarget = {
            target: target || null,
            starmapId: target && target.starmapId ? String(target.starmapId) : "",
            workspaceGeneration: window.currentWorkspaceGeneration(),
            serial: window.searchNavigationSerial
        }
        window.applyPendingStarmapSearchTarget()
    }

    function applyPendingStarmapSearchTarget() {
        var pending = window.pendingStarmapSearchTarget
        if (!pending || !pending.target) return false
        if (pending.serial !== window.searchNavigationSerial) {
            window.pendingStarmapSearchTarget = null
            return false
        }
        var workspace = starmapWorkspaceLoader.item
        if (!workspace || !workspace.revealSearchTarget) return false
        if (workspace.revealSearchTarget(pending.target)) {
            window.pendingStarmapSearchTarget = null
            // 星图目标真正选中并进入视野后才关闭搜索；只打开星图不算成功。
            if (globalSearchOverlay.visible) globalSearchOverlay.close()
            return true
        }
        return false
    }

    function navigateAnnotationTarget(target) {
        if (!target) return false
        if (target.externalUrl) {
            Qt.openUrlExternally(String(target.externalUrl))
            return true
        }
        var ref = target.objectRef || target
        if (!ref || !ref.kind) return false
        var projectId = String(ref.projectId || "")
        if (ref.kind === "namedEntry") {
            if (!projectId || !ref.objectId || !annotationBackend
                    || !annotationBackend.list_named_entries_json)
                return false
            var entryEnvelope = null
            try {
                entryEnvelope = JSON.parse(String(annotationBackend.list_named_entries_json(projectId) || "{}"))
            } catch (e) {}
            var entries = entryEnvelope && entryEnvelope.success === true
                ? (entryEnvelope.data || []) : []
            for (var e = 0; e < entries.length; ++e) {
                var entry = entries[e]
                if (entry && !entry.deletedAtMs
                        && String(entry.entryId) === String(ref.objectId)) {
                    annotationEntryDetailsDialog.entry = entry
                    annotationEntryDetailsDialog.open()
                    return true
                }
            }
            errorDialog.message = qsTr("登记名称目标已删除或不存在")
            errorDialog.open()
            return false
        }
        if (ref.kind === "textAnchor") {
            if (!projectId || !ref.objectId || !annotationBackend
                    || !annotationBackend.get_text_anchor_json) {
                errorDialog.message = qsTr("无法读取正文锚点位置")
                errorDialog.open()
                return false
            }
            var anchorEnvelope = null
            try {
                anchorEnvelope = JSON.parse(String(
                    annotationBackend.get_text_anchor_json(projectId, String(ref.objectId)) || "{}"))
            } catch (e) {}
            var anchor = anchorEnvelope && anchorEnvelope.success === true
                ? anchorEnvelope.data : null
            if (!anchor || anchor.deletedAtMs || !anchor.chapterId) {
                errorDialog.message = qsTr("正文锚点已删除或不存在")
                errorDialog.open()
                return false
            }
            window.pendingAnnotationChapter = {
                projectId: projectId,
                chapterId: String(anchor.chapterId),
                title: "",
                startByte: Number(anchor.startByte),
                endByte: Number(anchor.endByte)
            }
            appController.openWriting(projectId, window.projectTitleById(projectId) || qsTr("作品"))
            window.applyPendingAnnotationChapter()
            return true
        }
        if (ref.kind === "starMap") {
            appController.openRootStarmap(String(ref.objectId), String(ref.objectId))
            return true
        }
        if (ref.kind === "starMapNode" || ref.kind === "starMapEmbed") {
            if (!projectId || !ref.objectId) return false
            var owner = window.findAnnotationStarmapOwner(
                projectId, String(ref.kind), String(ref.objectId))
            if (!owner) return false
            var mapTarget = { starmapId: owner.starmapId }
            if (ref.kind === "starMapNode") mapTarget.nodeId = String(ref.objectId)
            else mapTarget.embedId = String(ref.objectId)
            window.pendingAnnotationMapTarget = {
                starmapId: owner.starmapId,
                target: mapTarget
            }
            appController.openRootStarmap(owner.starmapId, owner.title)
            if (window.applyPendingAnnotationMapTarget()) return true
            return true
        }
        if (ref.kind === "project" || ref.kind === "volume" || ref.kind === "chapter") {
            if (!projectId) return false
            appController.openWriting(projectId, window.projectTitleById(projectId) || qsTr("作品"))
            if (ref.kind === "chapter") {
                var chapter = null
                var tree = window.appState && window.appState.tree ? window.appState.tree : []
                for (var i = 0; i < tree.length; ++i) {
                    var node = tree[i]
                    if (node && node.type === "chapter" && String(node.id) === String(ref.objectId)
                            && String(node.projectId) === projectId) {
                        chapter = node
                        break
                    }
                }
                if (!chapter || !chapter.volumeId) return false
                window.pendingAnnotationChapter = {
                    projectId: projectId,
                    volumeId: String(chapter.volumeId),
                    chapterId: String(chapter.id),
                    title: String(chapter.title || "")
                }
                window.applyPendingAnnotationChapter()
            }
            return true
        }
        return false
    }

    // Annotation refs keep stable node/embed ids. Resolve the containing map
    // from the current project graph before opening the existing StarMap route.
    function findAnnotationStarmapOwner(projectId, kind, objectId) {
        if (!starmapBackend || !projectId || !objectId) return null
        var mapsEnvelope = appController.parseJson(
            starmapBackend.list_starmaps_for_project_json(String(projectId)),
            qsTr("无法查找星图目标"))
        var maps = mapsEnvelope && mapsEnvelope.success === true
            ? (mapsEnvelope.data || []) : []
        for (var i = 0; i < maps.length; ++i) {
            var meta = maps[i]
            if (!meta || !meta.starmapId) continue
            var graphEnvelope = appController.parseJson(
                starmapBackend.get_starmap_graph_json(String(meta.starmapId)),
                qsTr("无法读取星图目标"))
            var graphData = graphEnvelope && graphEnvelope.success === true
                ? graphEnvelope.data : null
            var graph = graphData && graphData.graph ? graphData.graph : null
            if (!graph) continue
            var objects = kind === "starMapNode" ? (graph.nodes || []) : (graph.embeds || [])
            for (var j = 0; j < objects.length; ++j) {
                var candidateId = kind === "starMapNode"
                    ? objects[j].id : objects[j].instanceId
                if (String(candidateId || "") === String(objectId)) {
                    return {
                        starmapId: String(meta.starmapId),
                        title: String(meta.title || qsTr("星图"))
                    }
                }
            }
        }
        return null
    }

    function applyPendingAnnotationMapTarget() {
        var pending = window.pendingAnnotationMapTarget
        var workspace = starmapWorkspaceLoader.item
        if (!pending || !workspace || !workspace.revealSearchTarget
                || String(workspace.starmapId || "") !== pending.starmapId)
            return false
        if (!workspace.revealSearchTarget(pending.target)) return false
        window.pendingAnnotationMapTarget = null
        return true
    }

    function applyPendingAnnotationChapter() {
        var pending = window.pendingAnnotationChapter
        var workspace = writingWorkspaceLoader.item
        if (!pending || !workspace || workspace.workspaceProjectId !== pending.projectId) return false
        if (!pending.volumeId) {
            var tree = window.appState && window.appState.tree ? window.appState.tree : []
            for (var i = 0; i < tree.length; ++i) {
                var node = tree[i]
                if (node && node.type === "chapter"
                        && String(node.id) === pending.chapterId
                        && String(node.projectId) === pending.projectId
                        && node.volumeId) {
                    pending.volumeId = String(node.volumeId)
                    if (!pending.title) pending.title = String(node.title || "")
                    break
                }
            }
            if (!pending.volumeId) return false
        }
        workspace.openChapter(pending.projectId, pending.volumeId, pending.chapterId, pending.title)
        if (window.pendingAnnotationChapter === pending
                && pending.startByte !== undefined && pending.endByte !== undefined
                && workspace.requestSearchReveal
                && workspace.requestSearchReveal(pending.projectId, pending.volumeId,
                    pending.chapterId, pending.startByte, pending.endByte)) {
            window.pendingAnnotationChapter = null
        } else if (pending.startByte === undefined || pending.endByte === undefined) {
            window.pendingAnnotationChapter = null
        }
        return true
    }

    function handleStarmapSceneReady(starmapId, routeKey) {
        var annotationPending = window.pendingAnnotationMapTarget
        if (annotationPending && annotationPending.starmapId === String(starmapId || "")
                && window.applyPendingAnnotationMapTarget()) return
        var pending = window.pendingStarmapSearchTarget
        if (!pending || !pending.target) return
        if (pending.serial !== window.searchNavigationSerial) return
        if (pending.starmapId && starmapId && pending.starmapId !== starmapId) return
        if (window.applyPendingStarmapSearchTarget()) return
        window.cancelPendingSearchNavigation(qsTr("无法定位该星图目标，请重新搜索"))
        window.debugLog("search", "starmap_reveal_unavailable", String(routeKey || ""))
    }

    // Issue #984：窗口级统一离开星图入口。设置切工作区、同步冲突打开作品、
    // 全局搜索跳转等壳层动作都会让 starmapWorkspaceLoader 失活，必须先提交
    // 活动 Scene 的唯一节点编辑会话。失败时保留原图与草稿，由 Canvas 显示错误。
    function flushStarmapBeforeRouteChange() {
        if (!appController.inStarmap) return true
        var workspace = starmapWorkspaceLoader.item
        if (!workspace || typeof workspace.flushPendingNodeEdit !== "function")
            return true
        try {
            return workspace.flushPendingNodeEdit() === true
        } catch (e) {
            return false
        }
    }

    function navigateToSearchResult(result) {
        if (!result) return false
        if (!window.searchTargetExists(result)) {
            globalSearchOverlay.showMessage(qsTr("目标已不存在或已被删除"))
            return false
        }

        var target = result.target || ({})
        // Issue #984：从星图跳到章节、根图或工作区前，先提交当前节点编辑。
        // 失败时保留星图与编辑器，搜索浮层不关闭，也不能让 Loader 失活。
        if (!window.flushStarmapBeforeRouteChange()) {
            globalSearchOverlay.showMessage(qsTr("节点正文尚未保存，无法离开星图"))
            return false
        }
        // 离开或定位当前章节前先保存。保存失败时不跳转，搜索浮层保持打开。
        if (appController.inWriting) {
            var saved = true
            try {
                saved = window.preSyncBarrier()
            } catch (e) {
                saved = false
            }
            if (!saved) {
                var reason = window.manualSyncBarrierFailureReason
                    || qsTr("当前章节尚未保存")
                globalSearchOverlay.showMessage(qsTr("无法离开当前章节：%1").arg(reason))
                return false
            }
        }

        if (target.chapterId) {
            var projectId = target.projectId || ""
            var volumeId = target.volumeId || ""
            var chapterId = target.chapterId
            appController.openWriting(
                projectId,
                window.projectTitleById(projectId) || result.title || qsTr("作品")
            )
            // 只有正文命中能在编辑器里按字节范围选中；标题/备注命中只负责打开章节。
            if (result.scope === "chapterBody") {
                window.schedulePendingSearchReveal(
                    projectId,
                    volumeId,
                    chapterId,
                    result.byteStart,
                    result.byteEnd
                )
                // 正文定位由章节加载完成信号驱动；真正选中后才关闭搜索。
                return true
            }
            globalSearchOverlay.close()
            return true
        }

        if (target.starmapId) {
            appController.openRootStarmap(
                target.starmapId,
                window.searchPathTail(result.path) || qsTr("星图")
            )
            if ((target.nodeId || target.edgeId || target.embedId
                    || target.linkId || target.hyperlinkId)
                    && result.scope !== "starmapTitle") {
                // 不立即关闭浮层：等目标真正选中并居中后再关闭；
                // 链接/超链接无法解析 owner 时保留搜索并提示，而不是只打开星图。
                window.schedulePendingStarmapSearchTarget(target)
            } else {
                globalSearchOverlay.close()
            }
            return true
        }

        if (target.settingKey || target.settingSection) {
            window.openSettingsDialog(target.settingSection || "", target.settingKey || "")
            globalSearchOverlay.close()
            return true
        }

        if (target.projectId) {
            appController.openWriting(
                target.projectId,
                window.projectTitleById(target.projectId) || result.title || qsTr("作品")
            )
            globalSearchOverlay.close()
            return true
        }

        globalSearchOverlay.showMessage(qsTr("该结果没有可导航的目标"))
        return false
    }

    function openConflictInProject(targetKind, projectId, path) {
        // Issue #902 第二十轮：App 冲突在 SyncPage 内部处理，不走 openWriting。
        if (targetKind === "app") return
        if (!projectId) return  // project target 必须有 projectId
        // Issue #984：从星图打开冲突前必须先提交节点编辑；失败时不要关设置，
        // 也不要写 pendingConflict，避免污染下一次打开状态。
        if (!window.flushStarmapBeforeRouteChange()) return
        // 关闭设置页，让写作工作区可见。
        if (settingsDialogLoader.item) settingsDialogLoader.item.close()
        // Issue #770 评论 5842877986: 先写 pending（projectId + path）再切作品，
        // 等 WritingWorkspace projectReady 信号到来且 projectId 匹配时再消费。
        window.pendingConflictProjectId = projectId
        window.pendingConflictPath = path || ""
        appController.openWriting(projectId, window.projectTitleById(projectId))
        // 不要求这一轮同步先结束；打开作品与同步是否在跑互不影响。
        // 若 Loader 已存在且项目已切到位，立即消费；否则等 projectReady。
        window.applyPendingConflictPath()
    }

    function openSyncDialog() {
        // Issue #696 评论 5698002089: 不在此处 load_local_settings()。
        // 打开设置/同步界面只负责开窗；本地主题设置只能由工作区初始化
        // internal_open_data_root() -> load_local_settings() 或无工作区的
        // load_app_theme_mode() 加载。UI 打开动作重新加载全局本地设置会触发
        // settings_changed -> themeController.reload()，导致主题异常切换。
        // 同步配置刷新由 SettingsDialog.onOpened 的 syncBackendRef.load_sync_config() 负责。
        window.openSettingsDialog();
    }


    property alias appState: appController.appState
    readonly property bool rootHasWorkspace: workspaceBackend !== null && workspaceBackend.has_workspace === true
    // Issue #977：全窗口顶栏与左侧一级导航共用同一宽度，品牌区不再单独猜 160/64。
    readonly property int desktopNavigationWidth: 200
    readonly property string desktopTopBarMode: appController.inWriting
        ? "writing"
        : (appController.inStarmap ? "starmap" : "hub")

    property string previousEditorText: ""

    // Issue #982：全局搜索浮层与待定位结果。
    property bool globalSearchVisible: false
    property var globalSearchContext: ({})
    property var pendingSearchReveal: null
    property var pendingStarmapSearchTarget: null
    property var pendingAnnotationMapTarget: null
    property var pendingAnnotationChapter: null
    property int searchNavigationSerial: 0

    SystemPalette {
        id: systemPalette
        colorGroup: SystemPalette.Active
    }

    function colorLuminance(colorValue) {
        return 0.2126 * colorValue.r + 0.7152 * colorValue.g + 0.0722 * colorValue.b;
    }

    function systemPaletteIsDark() {
        return colorLuminance(systemPalette.window) < colorLuminance(systemPalette.windowText);
    }

    function systemThemeIsDark() {
        // Issue #912：以实际显示的 KDE/Breeze 调色板为准。Qt.styleHints.colorScheme
        // 在部分 KDE 环境拿不到或与 Breeze 实际渲染不一致，会让「跟随系统」与
        // 壳层/纸张显示脱节。这里直接用 Kirigami.Theme 底色与文字色亮度对比。
        return kdePaletteIsDark;
    }

    function syncVisibleSystemTheme() {
        // Issue #912：仅在「跟随系统」时把实际显示的 KDE/Breeze 调色板深浅回写为
        // system_is_dark。显式深色/浅色经 KDE ColorScheme 入口切换的是应用配色，
        // Kirigami.Theme 反映的是应用选择而非桌面系统，回写会污染真实系统深浅。
        if (themeController && themeController.appearance_mode === "system") {
            themeController.set_system_is_dark(window.kdePaletteIsDark);
        }
    }

    function logThemeDiagnostics(event, snapshot) {
        // Issue #709 评论 issue-body-709: 主题诊断只读 ThemeController runtime state
        // 和 designTokens，不再读旧 appState.settings.themeMode、Qt.styleHints.colorScheme、
        // systemPaletteIsDark()。这些不再是主题诊断的权威来源。同一条诊断写出
        // appearance_mode/is_dark/color_source/选中 theme id/最终
        // primary/surface/on_surface/on_surface_variant/editorText。
        // Issue #736 评论 5778543593 修改2: 当传入 snapshot 时（onThemeApplied 调用），
        // 直接用 snapshot 的字段记录诊断，不经过第二轮 QML binding（Qt 不保证 binding
        // 求值顺序，诊断可能读到上一份值）。未传入 snapshot 时（其他调用点如
        // "system_color_scheme_changed"/"startup"），继续用 designTokens 派生 property。
        if (appBackend === null || !appBackend.log_qml) return;
        var tc = themeController;
        var primaryVal, surfaceVal, onSurfaceVal, onSurfaceVariantVal, editorTextVal;
        if (snapshot) {
            primaryVal = snapshot.primary;
            surfaceVal = snapshot.surface;
            onSurfaceVal = snapshot.on_surface;
            onSurfaceVariantVal = snapshot.on_surface_variant;
            editorTextVal = snapshot.editor_text;
        } else {
            primaryVal = designTokens.primary;
            surfaceVal = designTokens.surface;
            onSurfaceVal = designTokens.onSurface;
            onSurfaceVariantVal = designTokens.onSurfaceVariant;
            editorTextVal = designTokens.editorText;
        }
        appBackend.log_qml("info", "theme", event,
                        "appearance_mode=" + (tc ? tc.appearance_mode : "<null>")
                        + " is_dark=" + (tc ? tc.is_dark : "<null>")
                        + " color_source=" + (tc ? tc.color_source : "<null>")
                        + " builtin_theme_id=" + (tc ? tc.selected_builtin_theme_id : "<null>")
                        // Issue #912：把实际显示来源也写进同一条诊断——snapshot 的
                        // is_dark/editor_background（theme_applied 时）与当前 Breeze
                        // 壳层底色，便于核对壳层与纸张是否闭合。仍是每次主题变化一行。
                        + " snapshot_is_dark=" + (snapshot ? snapshot.is_dark : "<none>")
                        + " snapshot_editor_background=" + (snapshot ? snapshot.editor_background : "<none>")
                        + " kde_background=" + Kirigami.Theme.backgroundColor
                        + " primary=" + primaryVal
                        + " surface=" + surfaceVal
                        + " on_surface=" + onSurfaceVal
                        + " on_surface_variant=" + onSurfaceVariantVal
                        + " editorText=" + editorTextVal);
    }

    // Design tokens
    DesignTokens {
        id: designTokens
        // Issue #721: 颜色直接读 themeControllerRef.*_hex，不再从 themeStateJson 解析。
        // Issue #721 评论 5747140241: 属性名用 themeControllerRef，右侧 themeController 是
        // QQmlContext 注入对象。若属性名也叫 themeController，QML 绑定作用域（接收对象自身）
        // 会让右侧裸 themeController 遮蔽 QQmlContext 注入对象，自绑定保持 null，颜色链全走
        // fallback。属性名与注入名不同名后右侧明确拿外部注入对象。
        // themeStateJson 保留做 DesignTokens 输入，诊断改监听 themeApplied 信号，不参与颜色计算。
        themeControllerRef: themeController
        // Issue #702: 根 DesignTokens 只绑定 themeStateJson 这一份完整主题状态。
        // themeController 把 is_dark 和最终 ThemeColorScheme 打包成
        // {"is_dark": bool, "scheme": <object>} 一次性发布，QML 侧从同一份
        // JSON 解析 isDark 和 scheme，彻底消除 isDark 已是 true 但 scheme
        // 还是上一套浅色值的中间状态。不再分开绑定 is_dark 和
        // resolved_scheme_json 两个可能不同步的属性。
        themeStateJson: themeController !== null ? themeController.theme_state_json : ""
    }

    Connections {
        target: designTokens
        function onIsDarkChanged() {
            // Issue #668 评论 5646458592 问题 2: 删除 effective isDark 反写
            // themeController.set_system_is_dark 的逻辑。isDark 现在直接读
            // themeController.is_dark，反写会形成循环依赖。system_is_dark
            // 只由真实系统 colorScheme 变化（onColorSchemeChanged）和启动时
            // （Component.onCompleted）写入。
            if (appBackend) {
                appBackend.apply_window_dark_mode(designTokens.isDark);
            }
        }
        // Issue #736 评论 5777408243 问题2 / 5778543593 修改2: 主题诊断改为监听
        // DesignTokens 的 themeApplied 信号。themeApplied 在 applyThemeState() 完成
        // resolvedTheme 整体替换后发出，携带这份已发布的最终 token snapshot。
        // Issue #736 评论 5778543593 修改2: onThemeApplied(snapshot) 直接用 snapshot
        // 记录诊断，不经过第二轮 QML binding（Qt 不保证 binding 求值顺序，诊断可能
        // 读到上一份值）。
        function onThemeApplied(snapshot) {
            window.logThemeDiagnostics("theme_applied", snapshot);
        }
    }

    Connections {
        target: syncBackend
        function onSync_action_completed() {
            window.refreshManualSyncFeedback()
            if (settingsBackend) settingsBackend.refresh_theme_data()
            // Issue #724 评论 5750911834 问题 3: 改用 reload_from_backend_if_changed()
            // 避免重复 resolve。同步完成后主题输入可能未变，跳过 resolve。
            if (themeController) themeController.reload_from_backend_if_changed()
        }
        // Issue #754 评论 5814866116 改动1: 同步真正修改/重新加载了当前工作区内容时，
        // 由 SyncBackend::sync_content_applied 触发正文/树刷新。
        // onSync_action_completed 只处理同步状态/主题相关刷新，不无条件重载正文。
        // Issue #754 评论 5815901258: 用 refreshStateImmediate 立即拿到最新 appState，
        // 再用 reconcileActiveChapter 以权威 appState.selected 收口编辑器——同步删除当前
        // 章节/分卷/作品时编辑器退回"请选择或新建章节"，不再残留已删除正文。
        function onSync_content_applied() {
            appController.refreshStateImmediate(qsTr("刷新工作区内容失败"));

            if (appController.inWriting && writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.reconcileActiveChapter(
                    appController.appState && appController.appState.selected
                        ? appController.appState.selected
                        : null
                );
            }
        }
        function onSync_status_changed() {
            window.refreshManualSyncFeedback()
        }
        // Issue #983：保存配置/重新加载配置后立即重新检查一次到期条件。
        // 用 callLater 等本轮配置通知结束再启动，避免与设置页保存流程重入。
        function onSync_config_changed() {
            Qt.callLater(function() {
                window.maybeStartAutoSync("config_changed")
            })
        }
    }

    // Issue #912：只有当实际显示的 KDE/Breeze 调色板深浅变化时才写入
    // system_is_dark，取代原来监听 Qt.styleHints.colorSchemeChanged 的做法
    // （该系统信号在本环境可能与 Breeze 实际渲染不一致，导致跟随系统模式
    // 与壳层/纸张显示脱节）。这是 system_is_dark 的两个写入点之一
    // （另一个是 Component.onCompleted 启动时）。
    //
    // 仅「跟随系统」时回写：显式深色/浅色会通过 KDE ColorScheme 入口切换应用
    // 配色方案，此时 Kirigami.Theme 反映的是应用选择而非桌面系统，回写会污染
    // 真实的系统深浅（例如手动深色后切回跟随系统时得到错误的深色）。
    //
    // 延迟到本轮属性通知结束后执行：set_appearance_mode() 里的 sync_kde_color_scheme()
    // 会先 activateScheme 再发 scheme_changed，色板可能在 QML 还持有旧的
    // appearance_mode 绑定值时先变化。立即回写会把刚切换的应用色板误记成系统色板。
    // Qt.callLater 在 QML 引擎返回事件循环后调用，多次排队自动合并。
    onKdePaletteIsDarkChanged: {
        Qt.callLater(window.syncVisibleSystemTheme)
        window.logThemeDiagnostics("kde_palette_changed");
    }

    AppController {
        id: appController
        workspaceBackendRef: workspaceBackend
        projectBackendRef: projectBackend
        appBackendRef: appBackend
        onErrorRaised: function(message) {
            errorDialog.message = message;
            errorDialog.open();
        }
    }

    ProjectController {
        id: projectController
        projectBackendRef: projectBackend
        appController: appController
    }

    StarMapController {
        id: globalStarMapController
        starmapBackendRef: starmapBackend
        appController: appController
    }

    Component.onCompleted: {
        // 启动守卫：确保 DesignTokens 已初始化
        if (!designTokens) {
            console.error("[STARTUP GUARD] designTokens is null!")
        }
        window.verifyBackendRuntime();
        window.debugLog("app", "qml_completed", "QML components fully loaded");
        // Issue #668 评论 5646458592 问题 2: 启动时把真实系统 colorScheme
        // 写给 ThemeController，作为 system_is_dark 的唯一写入点之一。
        // themeController 内部根据 appearance_mode（dark/light/system）决定
        // 是否使用 system_is_dark，QML 不再自算 isDark。
        if (themeController) {
            themeController.set_system_is_dark(window.systemThemeIsDark());
        }
        window.logThemeDiagnostics("startup");
        if (appBackend) {
            appBackend.apply_window_dark_mode(designTokens.isDark);
        }
        appController.restoreWorkspace();
        // 启动时同步一次真实前台状态，避免窗口已经 active 却不触发 onActiveChanged。
        window.syncWritingSessionLifecycle();
        // Issue #696 评论 5696993601: 无工作区时 load_app_theme_mode() 已在
        // try_restore_last_workspace 内部执行完毕，这里发布一次
        // themeController.reload() 作为无工作区 fallback 路径的主题收口点。
        // 有工作区时由 onWorkspace_opened 收口，此处不重复触发。
        Qt.callLater(function() {
            if (!window.rootHasWorkspace && themeController) {
                themeController.reload();
            }
        });
    }

    onClosing: {
        if (editorBackend) {
            // 先结算当前计时切片并等统计 writer 落盘，确认成功后才正式结束会话，
            // 避免 process::exit 丢掉最后不足 30 秒的切片。
            // 失败时保持窗口和计时会话打开，稳定 eventId 会在稍后心跳/下次关闭时重试。
            var statsCloseError = editorBackend.close_writing_session_and_wait("close")
            if (statsCloseError && statsCloseError.length > 0) {
                close.accepted = false
                window.showPassiveNotification(qsTr("写作统计尚未保存：%1").arg(statsCloseError), 8000)
                return
            }
        }
        // 只有确认统计已写入、真正要退出时才停自动同步；被拒绝关窗后继续写作不能失去自动同步。
        window.autoSyncShutdown = true
        if (autoSyncWakeTimer) autoSyncWakeTimer.stop()
        if (appController.inWriting) {
            editorBackend.flush_recent_edits();
        }
        // 应用关闭前 flush pending settings save
        if (settingsBackend) {
            settingsBackend.flush_pending_settings_save();
        }
    }

    // ── Issue #983：自动同步唤醒 Timer ──
    // 只做唤醒；后端按持久化配置、Core last_attempt_time 和本进程实际开始时间
    // 判断是否到期。设置关闭、无工作区、同步进行中或手动 pending 时后端返回 false。
    Timer {
        id: autoSyncWakeTimer
        interval: 30000
        repeat: true
        running: !window.autoSyncShutdown
        onTriggered: window.maybeStartAutoSync("timer")
    }

    // ── Debounced settings save Timer ──
    // settingsBackend.save_requested 信号触发后，延迟 300ms 执行实际保存
    Timer {
        id: settingsDebounceSaveTimer
        interval: 300
        repeat: false
        onTriggered: {
            if (settingsBackend) {
                settingsBackend.do_save_local_settings();
            }
        }
    }

    Connections {
        target: settingsBackend
        function onSave_requested() {
            settingsDebounceSaveTimer.restart();
        }
    }

    function applyState(state) {
        appController.applyState(state);
    }

    Connections {
        target: projectBackend
        function onProjects_reloaded() {
            appController.refreshState(qsTr("刷新作品列表失败"));
        }
        function onSelected_item_changed() {
            appController.refreshStateImmediate(qsTr("刷新当前选择失败"))
            if (appController.inWriting)
                appController.saveNavigationState()
        }
    }

    Connections {
        target: workspaceBackend
        function onWorkspace_state_changed() {
            appController.refreshState(qsTr("刷新工作区状态失败"));
        }
        function onWorkspace_opened() {
            // Issue #696 评论 5696993601: 工作区恢复完成、
            // AppBackend::load_local_settings() 已执行后，立即调用一次
            // themeController.reload() 发布首次有效 scheme。不再等设置页
            // onOpened 重新 load_local_settings 触发 settings_changed 才切换。
            if (themeController) {
                themeController.reload();
            }
            // Issue #983：工作区已成功打开且同步配置已加载，立即检查一次是否到期。
            Qt.callLater(function() {
                window.maybeStartAutoSync("workspace_opened")
            })
        }
        function onWorkspace_content_changed() {
            appController.refreshState(qsTr("刷新工作区内容失败"));
        }
    }

    Connections {
        target: editorBackend
        function onClear_editor() {
            if (writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.previousEditorText = "";
            }
        }
    }

    Connections {
        target: settingsBackend
        function onSettings_changed() {
            appController.refreshState(qsTr("刷新设置失败"));
            // Issue #668 评论 5646458592 问题 2: 设置变更（如切换主题/配色/
            // appearance_mode）后调用 themeController.reload()，让
            // themeController 重新解析主题并发出 scheme_changed，使
            // DesignTokens.isDark / resolvedSchemeJson 等跟随更新。
            // 不再只 refreshState 而漏掉主题重载，导致 light/dark、isDark、
            // editorText 不同步。
            // Issue #724 评论 5750911834 问题 3: 改用 reload_from_backend_if_changed()
            // 避免重复 resolve。设置变更可能不涉及主题输入（如字体大小），
            // 此时跳过 resolve，避免 574 次 theme.resolve 和 light/dark 混合状态。
            if (themeController) {
                themeController.reload_from_backend_if_changed();
            }
        }
    }

    // === Main Content ===
    // Issue #977：全窗口只剩这一条 DesktopTopBar。三个工作区 Loader 都放在
    // workspaceBody 里，只负责顶栏底线以下的内容和交互，不再各自画顶栏。
    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        DesktopTopBar {
            Layout.fillWidth: true
            visible: rootHasWorkspace
            mode: window.desktopTopBarMode
            navigationWidth: window.desktopNavigationWidth
            dt: designTokens
            appState: window.appState
            starmapTitle: appController.starmapTitle
            starmapFocusIsRoot: starmapWorkspaceLoader.item
                && starmapWorkspaceLoader.item.focusIsRoot !== undefined
                ? starmapWorkspaceLoader.item.focusIsRoot : true
            toolbarLeadingWidth: writingWorkspaceLoader.item
                ? writingWorkspaceLoader.item.toolbarLeadingWidth : 200
            toolbarTrailingWidth: writingWorkspaceLoader.item
                ? writingWorkspaceLoader.item.toolbarTrailingWidth : 200
            currentFontSize: settingsBackend ? settingsBackend.setting_font_size : 16
            currentLineSpacing: settingsBackend ? settingsBackend.setting_line_spacing : 1.5
            firstLineIndent: settingsBackend ? settingsBackend.setting_auto_indent_enabled : false
            saveStatus: writingWorkspaceLoader.item
                ? writingWorkspaceLoader.item.saveStatus : ""

            onBackRequested: {
                if (window.desktopTopBarMode === "writing") {
                    appController.openHub(0);
                    window.debugLog("workspace", "exit_writing_mode", "");
                } else if (window.desktopTopBarMode === "starmap"
                           && starmapWorkspaceLoader.item) {
                    starmapWorkspaceLoader.item.navigateBack();
                }
            }
            onUndoRequested: {
                var ws = writingWorkspaceLoader.item;
                if (ws) ws.undoActiveEditor();
            }
            onRedoRequested: {
                var ws = writingWorkspaceLoader.item;
                if (ws) ws.redoActiveEditor();
            }
            onFontSizeChanged: function(size) {
                var ws = writingWorkspaceLoader.item;
                if (ws) ws.setWritingFontSize(size);
            }
            onLineSpacingChanged: function(spacing) {
                var ws = writingWorkspaceLoader.item;
                if (ws) ws.setWritingLineSpacing(spacing);
            }
            onFirstLineIndentToggled: {
                var ws = writingWorkspaceLoader.item;
                if (ws) ws.toggleWritingFirstLineIndent();
            }
            onFormatOneClick: {
                var ws = writingWorkspaceLoader.item;
                if (ws) ws.formatActiveEditor();
            }
            onRequestSync: window.requestManualSync(window.desktopTopBarMode)
            onRequestSearch: window.openGlobalSearch({})
            onOpenSettings: window.openSettingsDialog()
        }

        Item {
            id: workspaceBody
            Layout.fillWidth: true
            Layout.fillHeight: true

        // StarMapWorkspace: shown when in starmap editor mode
        Loader {
            id: starmapWorkspaceLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.inStarmap
            sourceComponent: StarMapWorkspace {
                dt: designTokens
                starmapBackendRef: starmapBackend
                annotationBackendRef: annotationBackend
                searchBackendRef: searchBackend
                aiAvailable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled
                starmapId: appController.starmapId
                starmapTitle: appController.starmapTitle
                appState: window.appState
                onBackClicked: {
                    // Issue #801: Workspace 自己已处理层级返回，只有根星图时才退到 Hub
                    appController.openHub(1);
                }
                // Issue #790 评论 5875963057: 顶栏收口后的同步/搜索/设置入口
                onRequestSync: {
                    window.requestManualSync("starmap")
                }
                onRequestSearch: {
                    window.openGlobalSearch({});
                }
                onOpenSettings: {
                    window.openSettingsDialog();
                }
                onSceneGeometryReady: function(starmapId, routeKey) {
                    window.handleStarmapSceneReady(starmapId, routeKey)
                }
                onAnnotationTargetRequested: function(target) {
                    window.navigateAnnotationTarget(target)
                }
            }
        }

        // CreativeHub: shown when workspace open and not in writing mode
        Loader {
            id: creativeHubLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.route === "hub"
            sourceComponent: CreativeHub {
                dt: designTokens
                navigationWidth: window.desktopNavigationWidth
                projectBackendRef: projectBackend
            editorBackendRef: editorBackend
                starmapBackendRef: starmapBackend
                starMapController: globalStarMapController
                // Issue #796 评论 5886483653: 传 appControllerRef，Hub tab 由 hubTab 驱动。
                appControllerRef: appController
                appState: window.appState
                tree: window.appState.tree || []
                aiCapable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled

                onOpenStarmapWorkspace: function(smId, smTitle) {
                    // Issue #796 评论 5887280405: 从 Hub 星图列表点卡片进入根星图，清空栈。
                    appController.openRootStarmap(smId, smTitle);
                }

                onOpenProject: function(projectId, projectTitle) {
                    appController.openWriting(projectId, projectTitle);
                    window.debugLog("workspace", "enter_writing_mode", "projectId=" + projectId);
                }

                onCreateProject: {
                    window.debugLog("project", "create_project_dialog_open", "");
                    createProjectDialog.open();
                }

                onRenameProjectRequested: function(projectId, title) {
                    projectController.renameProject(projectId, title);
                }

                onDeleteProjectRequested: function(projectId, title) {
                    confirmDialog.actionType = "delete_project";
                    confirmDialog.contextData = { projectId: projectId, title: title };
                    confirmDialog.open();
                }

                onOpenSettings: {
                    window.debugLog("settings", "settings_dialog_open", "");
                    window.openSettingsDialog();
                }

                onRequestSync: {
                    window.requestManualSync("hub")
                }

                onRequestSearch: {
                    window.openGlobalSearch({});
                }
            }
        }

        // WritingWorkspace: shown when in writing mode
        Loader {
            id: writingWorkspaceLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.inWriting
            // Issue #762 评论 5826175490 第 4 点：作品从全局冲突入口打开时，
            // Loader 到这里才有 item，补交待选中的冲突路径。
            onLoaded: {
                window.applyPendingConflictPath()
            }
            sourceComponent: WritingWorkspace {
                dt: designTokens
                projectBackendRef: projectBackend
                editorBackendRef: editorBackend
                // Issue #933：设置/导航后端显式交给写作区，不再由子页面读全局 context property。
                settingsBackendRef: settingsBackend
                workspaceBackendRef: workspaceBackend
                // Issue #835：透传星图后端给 WritingWorkspace，再由它透传给 RightDrawer。
                starmapBackendRef: starmapBackend
                // Issue #835 评论 6019713847: 透传真实 StarMapController，RightDrawer 里
                // StarMapPage 据此调 listStarmaps() 读真实根星图，不用写死卡片占位。
                starMapControllerRef: globalStarMapController
                // Issue #709 评论 issue-body-709: 传入 themeController 使
                // EditorController.logRenderColorProbe 能读取 runtime state。
                themeController: themeController
                appState: window.appState
                annotationBackendRef: annotationBackend
                searchBackendRef: searchBackend
                aiAvailable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled
                tree: window.appState.tree || []
                workspaceProjectId: appController.writingProjectId
                // Issue #757 评论 5818193510 第 5 点：传入 syncBackend 给 WritingWorkspace，
                // 用于监听同步完成信号并在冲突产生时打开临时冲突侧栏。
                syncBackendRef: syncBackend
                // Issue #770 评论 5842877986: 作品切到位后发 projectReady，
                // main.qml 据此消费 pendingConflictPath（项目 id 匹配才消费）。
                onProjectReady: {
                    window.applyPendingConflictPath()
                    window.handleWritingWorkspaceReady()
                    window.applyPendingAnnotationChapter()
                }
                onChapterLoaded: function(projectId, volumeId, chapterId) {
                    window.handleSearchChapterLoaded(projectId, volumeId, chapterId)
                    var pendingAnnotation = window.pendingAnnotationChapter
                    if (pendingAnnotation
                            && pendingAnnotation.projectId === projectId
                            && pendingAnnotation.chapterId === chapterId) {
                        if (pendingAnnotation.startByte !== undefined
                                && pendingAnnotation.endByte !== undefined
                                && !writingWorkspaceLoader.item.requestSearchReveal(
                                    pendingAnnotation.projectId, pendingAnnotation.volumeId,
                                    pendingAnnotation.chapterId, pendingAnnotation.startByte,
                                    pendingAnnotation.endByte)) {
                            errorDialog.message = qsTr("无法定位正文锚点范围")
                            errorDialog.open()
                        }
                        window.pendingAnnotationChapter = null
                    }
                }
                onChapterLoadFailed: function(projectId, volumeId, chapterId, reason) {
                    window.handleSearchChapterLoadFailed(projectId, volumeId, chapterId, reason)
                    if (window.pendingAnnotationChapter
                            && window.pendingAnnotationChapter.projectId === projectId
                            && window.pendingAnnotationChapter.chapterId === chapterId) {
                        window.pendingAnnotationChapter = null
                        errorDialog.message = qsTr("无法打开链接目标章节：%1").arg(String(reason || ""))
                        errorDialog.open()
                    }
                }
                onAnnotationTargetRequested: function(target) {
                    window.navigateAnnotationTarget(target)
                }

                onBackToProjects: {
                    // Issue #796 评论 5886483653: 从写作区返回仍回 openHub(0)（作品页）。
                    appController.openHub(0);
                    window.debugLog("workspace", "exit_writing_mode", "");
                }

                onOpenSettings: {
                    window.openSettingsDialog();
                }

                // Issue #790 评论 5875963057: 顶栏收口后的同步/搜索入口
                onRequestSync: {
                    window.requestManualSync("writing")
                }
                onRequestSearch: {
                    window.openGlobalSearch({});
                }


                onCreateVolumeRequested: function(projectId) {
                    inputDialog.actionType = "volume";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = "";
                    inputDialog.dialogTitle = qsTr("新建卷");
                    inputDialog.defaultText = "";
                    inputDialog.open();
                }

                onCreateChapterRequested: function(projectId, volumeId) {
                    inputDialog.actionType = "chapter";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = volumeId;
                    inputDialog.dialogTitle = qsTr("新建章节");
                    // 从树数据中统计当前卷的章节数量，生成默认标题
                    var chapterCount = 0;
                    var treeData = window.appState ? (window.appState.tree || []) : [];
                    for (var i = 0; i < treeData.length; i++) {
                        if (treeData[i].type === "chapter" && treeData[i].volumeId === volumeId) {
                            chapterCount++;
                        }
                    }
                    inputDialog.defaultText = qsTr("第%1章").arg(chapterCount + 1);
                    inputDialog.open();
                }

                onRenameItemRequested: function(itemData) {
                    inputDialog.actionType = "rename_" + itemData.type;
                    inputDialog.projectId = itemData.projectId || "";
                    inputDialog.volumeId = itemData.volumeId || "";
                    inputDialog.chapterId = itemData.id || "";
                    inputDialog.dialogTitle = qsTr("重命名");
                    inputDialog.defaultText = itemData.title || "";
                    inputDialog.open();
                }

                onDeleteItemRequested: function(itemData) {
                    confirmDialog.actionType = "delete_" + itemData.type;
                    confirmDialog.contextData = {
                        projectId: itemData.projectId || "",
                        volumeId: itemData.volumeId || "",
                        chapterId: itemData.id || "",
                        title: itemData.title || ""
                    };
                    confirmDialog.open();
                }
            }
        }

        // EmptyWorkspace: loaded only when no workspace (Loader destroys on deactivate)
        Loader {
            id: emptyWorkspaceLoader
            anchors.fill: parent
            active: !rootHasWorkspace
            onActiveChanged: {
                window.debugLog("app", "empty_workspace_loader_active_changed", "active=" + active);
            }
            sourceComponent: EmptyWorkspace {
                workspaceBackendRef: workspaceBackend
                dt: designTokens
                onCreateWorkspaceWithPath: (path) => {
                    appController.createWorkspaceWithPath(path, false);
                }
                onOpenWorkspaceWithPath: (path) => {
                    appController.createWorkspaceWithPath(path, true);
                }
                onInitFromGithub: {
                    window.openSyncDialog()
                }

            }
        }
        }
    }

    // 非模态状态条：持续显示本轮同步的排队、运行或终态信息，不阻挡编辑区操作。
    Rectangle {
        id: manualSyncFeedbackBanner
        visible: window.manualSyncFeedbackVisible && window.manualSyncFeedbackText.length > 0
        z: 1000
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        width: Math.min(parent.width - 32, 620)
        height: feedbackColumn.implicitHeight + 24
        radius: 8
        color: Kirigami.Theme.backgroundColor
        border.width: 1
        border.color: window.manualSyncFeedbackIsError
            ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.highlightColor

        ColumnLayout {
            id: feedbackColumn
            anchors.fill: parent
            anchors.margins: 12
            spacing: 6

            RowLayout {
                Layout.fillWidth: true
                Label {
                    Layout.fillWidth: true
                    text: window.manualSyncFeedbackIsError
                        ? (window.manualSyncOperationId ? qsTr("同步未完成") : qsTr("同步未开始"))
                        : qsTr("同步状态")
                    font.bold: true
                    color: window.manualSyncFeedbackIsError
                        ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                }
                ToolButton {
                    text: qsTr("关闭")
                    onClicked: window.manualSyncFeedbackVisible = false
                }
            }

            Label {
                Layout.fillWidth: true
                Layout.maximumHeight: Math.max(80, Math.min(220, window.height * 0.32))
                text: window.manualSyncFeedbackText
                wrapMode: Text.Wrap
                maximumLineCount: 9
                elide: Text.ElideRight
                color: Kirigami.Theme.textColor
            }
        }
    }

    // === Dialogs ===
    CreateProjectDialog {
        id: createProjectDialog
        theme: designTokens
        onSubmitProject: function(title) {
            var trimmedTitle = title ? title.trim() : "";
            var isEmpty = (trimmedTitle === "");
            window.debugLog("project", "create_project_submit", "titleLength=" + (title ? title.length : 0) + ", isEmpty=" + isEmpty);
            if (projectController.createProject(title)) {
                createProjectDialog.close();
            }
        }
    }

    // Issue #900: 用 KDE Controls 标准 Dialog 替换手绘背景与按钮，交由 Breeze 样式渲染。
    Dialog {
        id: confirmDialog
        property string actionType: ""
        property var contextData: ({})

        title: qsTr("确认删除")
        modal: true
        anchors.centerIn: parent
        width: 420

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            text: {
                if (confirmDialog.actionType === "delete_project") return qsTr("您确定要删除作品「%1」及其所有分卷、章节吗？").arg(confirmDialog.contextData.title);
                if (confirmDialog.actionType === "delete_volume") return qsTr("您确定要删除分卷「%1」及包含的所有章节吗？").arg(confirmDialog.contextData.title);
                if (confirmDialog.actionType === "delete_chapter") return qsTr("您确定要删除章节「%1」吗？").arg(confirmDialog.contextData.title);
                return qsTr("确定要删除吗？");
            }
        }

        footer: DialogButtonBox {
            Button {
                text: qsTr("取消")
                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
            }
            Button {
                text: qsTr("删除")
                DialogButtonBox.buttonRole: DialogButtonBox.DestructiveRole
                onClicked: {
                    if (projectController.deleteItem(confirmDialog.actionType, confirmDialog.contextData)) {
                        confirmDialog.close();
                    }
                }
            }
        }
    }

    Dialog {
        id: errorDialog
        property string message: ""
        title: qsTr("提示")
        modal: true
        anchors.centerIn: parent
        width: 360
        standardButtons: Dialog.Ok

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            text: errorDialog.message
        }
    }

    Dialog {
        id: annotationEntryDetailsDialog
        property var entry: null
        title: entry ? String(entry.canonicalName || qsTr("登记名称")) : qsTr("登记名称")
        modal: true
        anchors.centerIn: parent
        width: Math.min(460, window.width - 40)
        standardButtons: Dialog.Ok
        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: qsTr("别名：%1").arg((annotationEntryDetailsDialog.entry
                    && annotationEntryDetailsDialog.entry.aliases
                    ? annotationEntryDetailsDialog.entry.aliases : []).join("、") || qsTr("无"))
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: qsTr("已确认误写：%1").arg((annotationEntryDetailsDialog.entry
                    && annotationEntryDetailsDialog.entry.confirmedMisspellings
                    ? annotationEntryDetailsDialog.entry.confirmedMisspellings : []).join("、") || qsTr("无"))
            }
            Label {
                Layout.fillWidth: true
                text: annotationEntryDetailsDialog.entry
                    && annotationEntryDetailsDialog.entry.autoLinkEnabled
                    ? qsTr("正文中自动链接已开启") : qsTr("正文中自动链接已关闭")
            }
        }
    }

    Loader {
        id: settingsDialogLoader
        // Issue #912：Loader 只是 Dialog 的惰性实例化宿主，本身不渲染。填满父级
        // 是为了不让它保持零尺寸——Dialog 的位置改由 Overlay.overlay 决定后，
        // 零尺寸宿主只是历史遗留，这里显式收口避免歧义。
        anchors.fill: parent
        active: false
        sourceComponent: SettingsDialog {
            theme: designTokens
            // Issue #920：设置面板是否为宽屏两列由面板自身按宽度断点决定，
            // 不再从 Core 布局契约注入 layoutPlan。
            settingsBackendRef: settingsBackend
            workspaceBackendRef: workspaceBackend
            syncBackendRef: syncBackend
            editorBackendRef: editorBackend
            themeControllerRef: themeController
            beforeSyncHook: function() { return window.preSyncBarrier() }
            onSettingsChanged: {
                appController.refreshState(qsTr("刷新设置失败"));
                // Issue #724 评论 5750911834 问题 3: 改用 reload_from_backend_if_changed()
                // 避免与 settingsBackend.onSettings_changed 重复 resolve。
                if (themeController) themeController.reload_from_backend_if_changed();
            }
            // Issue #762 评论 5826175490 第 4 点：处理 SyncPage 的 openConflict 信号。
            // 打开对应作品并把目标冲突路径交给 WritingWorkspace/SyncConflictPanel 选中，
            // 不要求这一轮同步先结束。
            onOpenConflict: function(targetKind, projectId, path) {
                window.openConflictInProject(targetKind, projectId, path)
            }
            // Issue #790 评论 5875963057: 切换工作区入口收口到设置页
            onSwitchWorkspaceRequested: {
                // Issue #984：切换工作区会让星图 Loader 失活，先统一提交节点编辑。
                if (!window.flushStarmapBeforeRouteChange()) return
                var dialog = settingsDialogLoader.item
                if (dialog) {
                    dialog.flushSave()
                    dialog.close()
                }
                appController.switchWorkspace();
            }
        }
    }


    Dialog {
        id: inputDialog
        property string actionType: ""
        property string projectId: ""
        property string volumeId: ""
        property string chapterId: ""
        property string defaultText: ""
        property string dialogTitle: qsTr("请输入")

        modal: true
        anchors.centerIn: parent
        width: 340
        title: inputDialog.dialogTitle

        ColumnLayout {
            width: parent.width
            spacing: Kirigami.Units.smallSpacing

            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: {
                    if (inputDialog.actionType === "volume") return qsTr("卷名称");
                    if (inputDialog.actionType === "chapter") return qsTr("章节名称");
                    return qsTr("新名称");
                }
            }

            TextField {
                id: inputField
                Layout.fillWidth: true
                placeholderText: {
                    if (inputDialog.actionType === "volume") return qsTr("例如：第一卷");
                    if (inputDialog.actionType === "chapter") return qsTr("例如：第一章");
                    return qsTr("请输入新名称");
                }
                onAccepted: confirmInputButton.clicked()
            }
        }

        footer: DialogButtonBox {
            Button {
                text: qsTr("取消")
                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
            }
            Button {
                id: confirmInputButton
                text: qsTr("确定")
                DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
                onClicked: {
                    var title = inputField.text.trim();
                    // 章节允许空标题，后端会兜底生成默认标题
                    var allowEmpty = (inputDialog.actionType === "chapter");
                    if (title !== "" || allowEmpty) {
                        if (inputDialog.actionType === "volume") {
                            projectController.createVolume(inputDialog.projectId, title);
                        } else if (inputDialog.actionType === "chapter") {
                            projectController.createChapter(inputDialog.projectId, inputDialog.volumeId, title);
                        } else if (inputDialog.actionType === "rename_project") {
                            projectController.renameProject(inputDialog.projectId, title);
                        } else if (inputDialog.actionType === "rename_volume") {
                            projectController.renameVolume(inputDialog.projectId, inputDialog.volumeId, title);
                        } else if (inputDialog.actionType === "rename_chapter") {
                            projectController.renameChapter(inputDialog.projectId, inputDialog.volumeId, inputDialog.chapterId, title);
                        }
                    }
                    inputDialog.close();
                }
            }
        }

        onOpened: {
            inputField.text = inputDialog.defaultText;
        }
    }

    // Issue #982：全局搜索浮层放在最上层；只有点击结果才触发导航。
    GlobalSearchOverlay {
        id: globalSearchOverlay
        searchBackendRef: searchBackend
        onResultActivated: function(result) {
            window.navigateToSearchResult(result)
        }
        onClosed: {
            window.globalSearchVisible = false
        }
    }
}
