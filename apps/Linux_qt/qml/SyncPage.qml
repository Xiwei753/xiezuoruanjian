// =============================================================================
// SyncPage.qml — 同步页面
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：同步配置展示、手动同步触发、同步诊断、错误展示
// 约束：不直接操作 Git 或文件系统，业务动作全部委托 syncBackend 兼容入口
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window

Item {
    id: root
    implicitHeight: mainCol.implicitHeight + resolvedDt.sp32
    property int lastSyncResultLen: -1
    property double lastSyncStatusLogTime: 0
    property var dt: null
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt
    property var backendRef: null
    property var beforeSyncHook: null
    property var textCoordinator: null
    signal settingsChanged()

    property string activeOperationId: ""
    property string activeOperationKind: ""

    // Local reactive sync state
    property string currentSyncStatus: "not_configured"
    property bool currentSyncInProgress: false
    property string currentSyncOperationState: ""

    function syncMessageForKey(key, args) {
        if (!key) return ""
        var msg = ""
        switch (key) {
        case "sync.block.no_workspace": msg = qsTr("未打开工作区"); break;
        case "sync.block.disabled": msg = qsTr("同步已禁用"); break;
        case "sync.block.remote_url_missing": msg = qsTr("未设置远程仓库地址"); break;
        case "sync.block.token_missing": msg = qsTr("未设置访问 Token"); break;
        case "sync.block.invalid_directory": msg = qsTr("工作区目录无效"); break;
        case "sync.phase.diagnose": msg = qsTr("正在诊断..."); break;
        case "sync.phase.dry_run": msg = qsTr("正在试运行..."); break;
        case "sync.phase.syncing": msg = qsTr("正在同步..."); break;
        case "sync.phase.background_syncing": msg = qsTr("正在后台同步..."); break;
        case "sync.phase.github_init": msg = qsTr("正在初始化 GitHub 连接..."); break;
        case "sync.result.diagnose_success": msg = qsTr("诊断完成"); break;
        case "sync.result.diagnose_failed": msg = qsTr("诊断失败"); break;
        case "sync.result.dry_run_summary": msg = qsTr("试运行完成"); break;
        case "sync.result.dry_run_failed": msg = qsTr("试运行失败"); break;
        case "sync.result.success_summary": msg = qsTr("同步完成"); break;
        case "sync.result.latest_wins_summary": msg = qsTr("同步完成（以最新版本为准）"); break;
        case "sync.result.no_changes_summary": msg = qsTr("无需同步，本地与远端一致"); break;
        case "sync.result.conflict_summary": msg = qsTr("同步完成，存在冲突文件"); break;
        case "sync.result.partial_conflict_summary": msg = qsTr("部分同步，存在正文冲突"); break;
        case "sync.result.dirty_repo_blocked": msg = qsTr("同步被阻止：本地有未提交的更改"); break;
        case "sync.result.branch_recovered_summary": msg = qsTr("分支已恢复并同步完成"); break;
        case "sync.result.generic_error": msg = qsTr("同步出错"); break;
        case "sync.result.save_config_success": msg = qsTr("配置已保存"); break;
        case "sync.result.save_config_failed": msg = qsTr("保存配置失败"); break;
        case "sync.result.clone_success_init_failed": msg = qsTr("克隆成功但初始化失败"); break;
        case "sync.result.push_failed_save_config_failed": msg = qsTr("推送失败且保存配置失败"); break;
        case "sync.result.push_failed": msg = qsTr("推送失败"); break;
        case "sync.result.clone_init_success": msg = qsTr("克隆并初始化成功"); break;
        case "sync.result.clone_failed": msg = qsTr("克隆失败"); break;
        case "sync.result.remote_configured_sync_success": msg = qsTr("远端已配置，同步成功"); break;
        case "sync.result.no_conflict_files": msg = qsTr("无冲突文件"); break;
        case "sync.result.more_files_count": msg = qsTr("还有更多文件"); break;
        case "sync.result.git_repo_not_workspace": msg = qsTr("该目录已是 Git 仓库但不是工作区"); break;
        case "sync.result.directory_not_empty_not_workspace": msg = qsTr("目录非空且不是工作区"); break;
        case "sync.result.configured_not_tested": msg = qsTr("已配置，尚未测试"); break;
        case "sync.status.already_running": msg = qsTr("同步正在运行中"); break;
        case "error.io": msg = qsTr("文件读写失败"); break;
        case "error.json": msg = qsTr("数据格式异常"); break;
        case "error.invalid_workspace": msg = qsTr("不是有效的工作区"); break;
        case "error.project_not_found": msg = qsTr("作品不存在"); break;
        case "error.volume_not_found": msg = qsTr("卷不存在"); break;
        case "error.chapter_not_found": msg = qsTr("章节不存在"); break;
        case "error.empty_overwrite_blocked": msg = qsTr("已阻止空内容覆盖"); break;
        case "error.not_implemented": msg = qsTr("功能尚未实现"); break;
        case "error.refuse_delete_workspace_root": msg = qsTr("拒绝删除工作区根目录"); break;
        case "error.invalid_delete_target": msg = qsTr("删除目标无效"); break;
        case "error.sync_conflict": msg = qsTr("同步冲突，请手动处理冲突文件后重试"); break;
        case "error.sync_failed": msg = qsTr("同步失败，请检查网络和配置"); break;
        case "error.other": msg = qsTr("操作失败"); break;
        case "error.core_error": msg = qsTr("核心模块错误"); break;
        case "error.clipboard_unavailable": msg = qsTr("剪贴板不可用"); break;
        case "error.json_parse": msg = qsTr("数据解析失败"); break;
        case "error.empty_title": msg = qsTr("标题不能为空"); break;
        case "error.sync_diagnose_panic": msg = qsTr("诊断过程异常终止"); break;
        case "error.sync_dry_run_panic": msg = qsTr("试运行过程异常终止"); break;
        case "error.sync_panic": msg = qsTr("同步过程异常终止"); break;
        case "error.load_sync_config_failed": msg = qsTr("加载同步配置失败"); break;
        case "error.core_not_initialized": msg = qsTr("核心模块未初始化"); break;
        case "error.parse_json_failed": msg = qsTr("解析数据失败"); break;
        case "error.save_sync_config_failed": msg = qsTr("保存同步配置失败"); break;
        case "error.save_sync_secrets_failed": msg = qsTr("保存同步密钥失败"); break;
        case "chapter.deleted_remotely_refreshed": msg = qsTr("章节已在远端被删除，已刷新列表"); break;
        default: msg = qsTr("同步出错"); window.debugWarn("sync", "unmapped_message_key", key); break;
        }
        if (args) {
            for (var k in args) {
                msg = msg.replace("{" + k + "}", args[k])
            }
        }
        return msg
    }

    function updateSyncResultText() {
        if (root.backendRef) {
            try {
                var obj = JSON.parse(root.currentSyncOperationState);
                if (root.activeOperationId === "" || obj.operation_id === root.activeOperationId) {
                    var text = ""
                    if (obj.phase_key) {
                        text += syncMessageForKey(obj.phase_key, obj.summary_args) + "\n"
                    }
                    if (obj.summary_key) {
                        text += syncMessageForKey(obj.summary_key, obj.summary_args)
                    }

                    // Display counts if it's a summary
                    if (obj.counts && (obj.counts.uploaded > 0 || obj.counts.downloaded > 0 || obj.counts.local_deleted > 0 || obj.counts.remote_deleted > 0 || obj.counts.conflicts > 0)) {
                        text += "\n\n" + qsTr("上传: ") + obj.counts.uploaded
                        text += "\n" + qsTr("下载: ") + obj.counts.downloaded
                        text += "\n" + qsTr("本地删除: ") + obj.counts.local_deleted
                        text += "\n" + qsTr("远端删除: ") + obj.counts.remote_deleted
                        if (obj.counts.overwritten > 0) text += "\n" + qsTr("覆盖: ") + obj.counts.overwritten
                        if (obj.counts.conflicts > 0) text += "\n" + qsTr("冲突: ") + obj.counts.conflicts
                    }

                    if (obj.summary_args && obj.summary_args.conflict_files) {
                        text += "\n\n" + qsTr("冲突文件:") + "\n  - " + obj.summary_args.conflict_files
                    }

                    syncResultArea.text = text;

                    if (obj.raw_error) {
                        syncRawErrorArea.text = obj.raw_error
                        syncRawErrorRow.visible = true
                    } else {
                        syncRawErrorArea.text = ""
                        syncRawErrorRow.visible = false
                    }
                } else {
                    syncResultArea.text = qsTr("同步状态解析失败，请复制诊断信息反馈");
                    window.debugWarn("sync", "operation_state_parse_failed", root.currentSyncOperationState);
                }
            } catch(e) {
                syncResultArea.text = qsTr("同步状态解析失败，请复制诊断信息反馈");
                window.debugWarn("sync", "operation_state_json_error", e.toString());
            }
        }
    }

    function showConfigFeedback(message, isError) {
        syncConfigFeedback.message = message
        syncConfigFeedback.isError = isError
        syncConfigFeedback.visible = true
        configFeedbackTimer.restart()
    }

    function refreshLocalSyncState() {
        if (root.backendRef) {
            root.currentSyncStatus = root.backendRef.sync_status || "not_configured";
            root.currentSyncInProgress = root.backendRef.sync_in_progress || false;
            root.currentSyncOperationState = root.backendRef.sync_operation_state || "";
            autoSyncSwitch.checked = root.backendRef.sync_auto_sync || false;
            syncIntervalSlider.value = (root.backendRef.sync_interval || 300) / 60;
            root.updateSyncResultText();
            // 同步失败时，如果结果区为空，则显示错误信息
            if (root.isFailureStatus(root.currentSyncStatus) && syncResultArea.text.trim() === "") {
                var opState = root.currentSyncOperationState.trim();
                var parsed = null;
                try { parsed = JSON.parse(opState); } catch(e) {}
                if (parsed && parsed.summary_key) {
                    syncResultArea.text = syncMessageForKey(parsed.summary_key, parsed.summary_args);
                } else if (parsed && parsed.phase_key) {
                    syncResultArea.text = syncMessageForKey(parsed.phase_key, parsed.summary_args);
                } else {
                    syncResultArea.text = qsTr("同步失败");
                }
                if (parsed && parsed.raw_error) {
                    syncRawErrorArea.text = parsed.raw_error;
                    syncRawErrorRow.visible = true;
                }
            }
        }
    }

    // Remove color since root is now an Item
    function statusKind() {
        var s = root.currentSyncStatus
        if (s === "success") return "success"
        if (s === "syncing") return "warning"
        if (s === "partial_conflict") return "warning"
        if (root.isFailureStatus(s)) return "error"
        return "info"
    }

    function isFailureStatus(s) {
        return s === "error" || s === "conflict" || s === "partial_conflict" || s === "recoverable_error" || s === "fatal_error" || s === "auth_failed" || s === "network_failed" || s === "token_missing" || s === "token_invalid" || s === "token_permission_denied" || s === "repo_not_found_or_no_permission" || s === "branch_missing"
    }

    function isConflictStatus(s) {
        return s === "conflict" || s === "partial_conflict"
    }

    function statusText() {
        var s = root.currentSyncStatus
        if (s === "success") return qsTr("已同步")
        if (s === "syncing") return qsTr("同步中")
        if (s === "conflict") return qsTr("同步冲突")
        if (s === "partial_conflict") return qsTr("部分同步，存在正文冲突")
        if (s === "dry_run_success") return qsTr("检查成功")
        if (s === "diagnostics_success") return qsTr("诊断成功")
        if (s === "token_missing") return qsTr("未设置 Token")
        if (s === "token_invalid") return qsTr("GitHub token 无效或已过期。请检查 token 是否正确。")
        if (s === "token_permission_denied") return qsTr("GitHub token 权限不足。请给该 token 勾选目标仓库，并授予 Contents: Read and write。")
        if (s === "repo_not_found_or_no_permission") return qsTr("仓库不存在或无权限")
        if (s === "remote_branch_missing" || s === "branch_missing") return qsTr("远程分支不存在")
        if (s === "network_failed") return qsTr("网络连接失败")
        if (root.isFailureStatus(s)) return qsTr("同步失败")
        // 同步状态枚举显示：未配置、已配置未测试
        if (s === "not_configured") return qsTr("未配置")
        if (s === "no_workspace") return qsTr("未打开工作区")
        if (s === "configured_not_tested") return qsTr("已配置")
        if (root.backendRef && root.backendRef.sync_enabled) return qsTr("同步")
        return qsTr("配置同步")
    }

    Connections {
        target: root.backendRef
        function onSync_action_completed() {
            var stateStr = (root.backendRef && root.backendRef.sync_operation_state) || "";
            if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                window.debugLog("sync", "action_completed_callback", "resultLength=" + stateStr.length)
            }
            root.refreshLocalSyncState();
        }
        function onSync_status_changed() {
            var stateStr = (root.backendRef && root.backendRef.sync_operation_state) || "";
            var resLen = stateStr.length
            var now = Date.now()
            var shouldLog = true
            if (resLen === root.lastSyncResultLen && now - root.lastSyncStatusLogTime < 5000) shouldLog = false
            if (shouldLog) {
                root.lastSyncResultLen = resLen
                root.lastSyncStatusLogTime = now
                if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                    window.debugLog("sync", "status_changed_callback", "resultLength=" + resLen)
                }
            }
            root.refreshLocalSyncState();
        }
        function onSync_config_changed() {
            root.refreshLocalSyncState();
        }
    }

    ColumnLayout {
        id: mainCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: resolvedDt.sp16
        spacing: resolvedDt.sp16

        RowLayout {
            Layout.fillWidth: true
            spacing: resolvedDt.sp12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: resolvedDt.sp4
                AppText {
                    dt: root.resolvedDt
                    text: qsTr("同步设置")
                    color: resolvedDt.onBackground
                    font.pixelSize: resolvedDt.title
                    font.family: resolvedDt.fontFamily
                    font.weight: Font.Bold
                }
                AppText {
                    dt: root.resolvedDt
                    text: qsTr("配置远端仓库并查看同步状态")
                    color: resolvedDt.onSurfaceVariant
                    font.pixelSize: resolvedDt.body
                    font.family: resolvedDt.fontFamily
                }
            }

            StatusPill {
                dt: root.resolvedDt
                status: root.statusKind()
                text: root.statusText()
            }
        }

        AppCard {
            Layout.fillWidth: true
            dt: root.resolvedDt
            variant: "surface"
            spacing: resolvedDt.sp16

            CoordinatorTextField {
                id: urlField
                Layout.fillWidth: true
                dt: root.resolvedDt
                label: qsTr("远程仓库地址")
                text: (root.backendRef ? root.backendRef.sync_remote_url : "")
                placeholderText: "https://github.com/user/repo"
                coordinator: root.textCoordinator
                targetId: "sync-repo-url"
                isUrl: true
            }

            CoordinatorTextField {
                id: branchField
                Layout.fillWidth: true
                dt: root.resolvedDt
                label: qsTr("分支名")
                text: (root.backendRef ? root.backendRef.sync_branch : "")
                placeholderText: "main"
                coordinator: root.textCoordinator
                targetId: "sync-branch"
            }

            CoordinatorTextField {
                id: tokenField
                Layout.fillWidth: true
                dt: root.resolvedDt
                label: qsTr("访问 Token")
                placeholderText: (root.backendRef ? root.backendRef.has_sync_token : false) ? qsTr("已设置（输入新 Token 以覆盖）") : qsTr("请输入 GitHub Personal Access Token")
                echoMode: TextInput.Password
                coordinator: root.textCoordinator
                targetId: "sync-token"
                isSecret: true
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: resolvedDt.sp12

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: resolvedDt.sp2
                    AppText {
                        dt: root.resolvedDt
                        text: qsTr("自动同步")
                        color: resolvedDt.onBackground
                        font.pixelSize: resolvedDt.body
                        font.family: resolvedDt.fontFamily
                    }
                    AppText {
                        dt: root.resolvedDt
                        text: qsTr("启用后按设定间隔自动执行同步")
                        color: resolvedDt.onSurfaceVariant
                        font.pixelSize: resolvedDt.caption
                        font.family: resolvedDt.fontFamily
                    }
                }

                Switch {
                    id: autoSyncSwitch
                    onToggled: {
                        if (root.backendRef) {
                            root.backendRef.sync_auto_sync = autoSyncSwitch.checked
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: resolvedDt.sp12
                visible: autoSyncSwitch.checked

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: resolvedDt.sp2
                    AppText {
                        dt: root.resolvedDt
                        text: qsTr("同步间隔")
                        color: resolvedDt.onBackground
                        font.pixelSize: resolvedDt.body
                        font.family: resolvedDt.fontFamily
                    }
                    AppText {
                        dt: root.resolvedDt
                        text: Math.round(syncIntervalSlider.value) + qsTr(" 分钟")
                        color: resolvedDt.onSurfaceVariant
                        font.pixelSize: resolvedDt.caption
                        font.family: resolvedDt.fontFamily
                    }
                }

                Slider {
                    id: syncIntervalSlider
                    Layout.fillWidth: true
                    from: 1
                    to: 60
                    stepSize: 1
                    onMoved: {
                        if (root.backendRef) {
                            root.backendRef.sync_interval = Math.round(value) * 60
                        }
                    }
                }
            }
        }

        Flow {
            Layout.fillWidth: true
            spacing: resolvedDt.sp8

            AppButton {
                text: qsTr("保存配置")
                dt: root.resolvedDt
                variant: "primary"
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        var hasNewToken = tokenField.text.trim().length > 0
                        window.debugLog("sync", "save_config_clicked", "url=" + urlField.text + ", branch=" + branchField.text + ", hasNewToken=" + hasNewToken)
                    }
                    if (!root.backendRef) return
                    root.backendRef.sync_remote_url = urlField.text
                    root.backendRef.sync_branch = branchField.text.length > 0 ? branchField.text : "main"
                    if (tokenField.text.trim().length > 0) {
                        root.backendRef.set_sync_token(tokenField.text.trim())
                        tokenField.text = ""
                    }
                    root.backendRef.sync_enabled = true
                    root.backendRef.sync_backend_type = "github_api"

                    var success = root.backendRef.save_sync_config()
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("sync", "save_config_finished", "success=" + success)
                    }
                    if (success) {
                        root.backendRef.load_sync_config()
                        root.refreshLocalSyncState()
                        tokenField.text = ""
                        root.settingsChanged()
                        root.showConfigFeedback(qsTr("配置已保存"), false)
                    } else {
                        syncResultArea.text = root.backendRef.sync_operation_state || qsTr("保存配置失败")
                        root.showConfigFeedback(qsTr("保存配置失败"), true)
                    }
                }
            }

            AppButton {
                text: qsTr("执行同步")
                dt: root.resolvedDt
                variant: "secondary"
                enabled: !root.currentSyncInProgress && root.backendRef && root.backendRef.sync_can_run
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") window.debugLog("sync", "perform_sync_clicked", "")
                    // 先保存当前 UI 配置
                    if (root.backendRef) {
                        root.backendRef.sync_remote_url = urlField.text
                        root.backendRef.sync_branch = branchField.text.length > 0 ? branchField.text : "main"
                        if (tokenField.text.trim().length > 0) {
                            root.backendRef.set_sync_token(tokenField.text.trim())
                            tokenField.text = ""
                        }
                        root.backendRef.save_sync_config()
                        root.backendRef.load_sync_config()
                    }
                    // 再执行同步
                    if (typeof root.beforeSyncHook === "function") {
                        if (!root.beforeSyncHook()) {
                            syncResultArea.text = qsTr("同步已取消：保存当前章节失败，请检查编辑器内容后重试")
                            return
                        }
                    }
                    syncResultArea.text = qsTr("正在同步...\n正在拉取远端清单\n正在比较本地和远端\n正在下载远端较新文件\n正在上传本地较新文件")
                    if (root.backendRef) {
                        var opId = root.backendRef.perform_sync()
                        root.activeOperationId = opId
                        root.activeOperationKind = "sync"
                        root.refreshLocalSyncState()
                        // 如果 opId 为空，显示即时错误
                        if (!opId || opId.length === 0) {
                            syncResultArea.text = root.backendRef.sync_operation_state || qsTr("同步启动失败")
                        }
                    }
                }
            }

            AppButton {
                text: qsTr("运行诊断")
                dt: root.resolvedDt
                variant: "secondary"
                enabled: !root.currentSyncInProgress && root.backendRef && root.backendRef.has_workspace
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") window.debugLog("sync", "perform_diagnostics_clicked", "")
                    syncResultArea.text = qsTr("正在诊断...")
                    if (root.backendRef) {
                        var opId = root.backendRef.perform_sync_diagnostics()
                        root.activeOperationId = opId
                        root.activeOperationKind = "dry_run"
                        // 如果 opId 为空，显示即时错误
                        if (!opId || opId.length === 0) {
                            syncResultArea.text = root.backendRef.sync_operation_state || qsTr("诊断启动失败")
                        }
                    }
                }
            }

            AppButton {
                text: qsTr("打开工作区目录")
                dt: root.resolvedDt
                variant: "text"
                visible: root.backendRef && root.backendRef.has_workspace
                onClicked: if (root.backendRef) root.backendRef.open_workspace_dir()
            }

            AppButton {
                text: qsTr("复制冲突信息")
                dt: root.resolvedDt
                variant: "danger"
                visible: root.backendRef && root.isConflictStatus(root.currentSyncStatus)
                onClicked: if (root.backendRef) root.backendRef.copy_text_to_clipboard(syncResultArea.text)
            }
        }

        AppText {
            id: syncBlockReason
            dt: root.resolvedDt
            visible: root.backendRef && !root.backendRef.sync_can_run && root.backendRef.sync_block_reason.length > 0
            text: root.backendRef ? root.backendRef.sync_block_reason : ""
            color: resolvedDt.onSurfaceVariant
            font.pixelSize: resolvedDt.caption
            font.family: resolvedDt.fontFamily
            Layout.fillWidth: true
        }

        AppText {
            id: syncConfigFeedback
            dt: root.resolvedDt
            property string message: ""
            property bool isError: false
            visible: message.length > 0
            text: syncConfigFeedback.message
            color: isError ? resolvedDt.error : resolvedDt.onSurfaceVariant
            font.pixelSize: resolvedDt.caption
            font.family: resolvedDt.fontFamily
            Layout.fillWidth: true
        }

        Timer {
            id: configFeedbackTimer
            interval: 3000
            onTriggered: {
                syncConfigFeedback.message = ""
                syncConfigFeedback.visible = false
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 180
            color: root.isFailureStatus(root.currentSyncStatus) ? resolvedDt.dangerContainer : resolvedDt.surfaceContainerLow
            border.color: root.isFailureStatus(root.currentSyncStatus) ? resolvedDt.error : resolvedDt.border
            border.width: root.isFailureStatus(root.currentSyncStatus) ? 2 : 1
            radius: resolvedDt.radiusLg
            clip: true
            visible: (root.currentSyncOperationState !== "" || root.currentSyncStatus === "syncing" || root.isFailureStatus(root.currentSyncStatus))

            ScrollView {
                id: logScroll
                anchors.fill: parent
                anchors.margins: resolvedDt.sp12
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea {
                    id: syncResultArea
                    width: logScroll.availableWidth
                    text: ""
                    color: root.isFailureStatus(root.currentSyncStatus) ? resolvedDt.error : resolvedDt.onSurfaceVariant
                    font.family: "monospace"
                    font.pixelSize: resolvedDt.caption
                    readOnly: true
                    background: null
                    wrapMode: TextEdit.Wrap
                }
            }
        }

        RowLayout {
            id: syncRawErrorRow
            Layout.fillWidth: true
            visible: false
            spacing: resolvedDt.sp8

            AppText {
                dt: root.resolvedDt
                text: qsTr("诊断信息可复制")
                color: resolvedDt.onSurfaceVariant
                font.pixelSize: resolvedDt.caption
                font.family: resolvedDt.fontFamily
            }

            AppButton {
                text: qsTr("复制")
                dt: root.resolvedDt
                variant: "text"
                onClicked: if (root.backendRef) root.backendRef.copy_text_to_clipboard(syncRawErrorArea.text)
            }
        }
    }

    Component.onCompleted: {
        if (root.backendRef) {
            root.backendRef.load_sync_config()
            autoSyncSwitch.checked = root.backendRef.sync_auto_sync || false
            syncIntervalSlider.value = (root.backendRef.sync_interval || 300) / 60
        }
        root.refreshLocalSyncState();
        }

        TextEdit {
            id: syncRawErrorArea
            visible: false
            text: ""
            readOnly: true
        }
    }
