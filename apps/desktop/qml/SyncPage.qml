// =============================================================================
// SyncPage.qml — 同步页面
// =============================================================================
//
// 层级：Desktop UI 层（QML 页面）
// 职责：同步配置展示、手动同步触发、同步诊断、错误展示
// 约束：不直接操作 Git 或文件系统，业务动作全部委托 syncBackend 兼容入口
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window

Item {
    id: root
    implicitHeight: mainCol.implicitHeight + theme.sp32
    property int lastSyncResultLen: -1
    property double lastSyncStatusLogTime: 0
    property var theme: null
    property var backendRef: null
    property var beforeSyncHook: null
    signal settingsChanged()

    property string activeOperationId: ""
    property string activeOperationKind: ""

    // Local reactive sync state
    property string currentSyncStatus: "not_configured"
    property bool currentSyncInProgress: false
    property string currentSyncOperationState: ""

    function updateSyncResultText() {
        if (root.backendRef) {
            try {
                var obj = JSON.parse(root.currentSyncOperationState);
                if (root.activeOperationId === "" || obj.operation_id === root.activeOperationId) {
                    syncResultArea.text = obj.summary || "";
                } else {
                    syncResultArea.text = root.currentSyncOperationState;
                }
            } catch(e) {
                syncResultArea.text = root.currentSyncOperationState;
            }
        }
    }

    function refreshLocalSyncState() {
        if (root.backendRef) {
            root.currentSyncStatus = root.backendRef.sync_status || "not_configured";
            root.currentSyncInProgress = root.backendRef.sync_in_progress || false;
            root.currentSyncOperationState = root.backendRef.sync_operation_state || "";
            root.updateSyncResultText();
            // 同步失败时，如果结果区为空，则显示错误信息
            if (root.isFailureStatus(root.currentSyncStatus) && syncResultArea.text.trim() === "") {
                var opState = root.currentSyncOperationState.trim();
                if (opState !== "") {
                    syncResultArea.text = opState;
                } else {
                    syncResultArea.text = qsTr("同步失败");
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
        anchors.margins: theme.sp16
        spacing: theme.sp16

        RowLayout {
            Layout.fillWidth: true
            spacing: theme.sp12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: theme.sp4
                AppText {
                    dt: root.theme
                    text: qsTr("同步设置")
                    color: theme.onBackground
                    font.pixelSize: theme.title
                    font.family: theme.fontFamily
                    font.weight: Font.Bold
                }
                AppText {
                    dt: root.theme
                    text: qsTr("配置远端仓库并查看同步状态")
                    color: theme.onSurfaceVariant
                    font.pixelSize: theme.body
                    font.family: theme.fontFamily
                }
            }

            StatusPill {
                dt: root.theme
                status: root.statusKind()
                text: root.statusText()
            }
        }

        AppCard {
            Layout.fillWidth: true
            dt: root.theme
            variant: "surface"
            spacing: theme.sp16

            AppTextField {
                id: urlField
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("远程仓库地址")
                text: (root.backendRef ? root.backendRef.sync_remote_url : "")
                placeholderText: "https://github.com/user/repo"
            }

            AppTextField {
                id: branchField
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("分支名")
                text: (root.backendRef ? root.backendRef.sync_branch : "")
                placeholderText: "main"
            }

            AppTextField {
                id: tokenField
                Layout.fillWidth: true
                theme: root.theme
                label: qsTr("访问 Token")
                placeholderText: (root.backendRef ? root.backendRef.has_sync_token : false) ? qsTr("已设置（输入新 Token 以覆盖）") : qsTr("请输入 GitHub Personal Access Token")
                echoMode: TextInput.Password
            }
        }

        Flow {
            Layout.fillWidth: true
            spacing: theme.sp8

            AppButton {
                text: qsTr("保存配置")
                dt: root.theme
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
                    }
                }
            }

            AppButton {
                text: qsTr("执行同步")
                dt: root.theme
                variant: "secondary"
                enabled: !root.currentSyncInProgress
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
                    syncResultArea.text = qsTr("正在同步...\n正在拉取远端清单\n正在比较本地和远端\n正在下载远端较新文件\n正在上传本地较新文件")
                    if (typeof root.beforeSyncHook === "function") root.beforeSyncHook()
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
                dt: root.theme
                variant: "secondary"
                enabled: !root.currentSyncInProgress
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") window.debugLog("sync", "perform_diagnostics_clicked", "")
                    syncResultArea.text = qsTr("正在诊断...")
                    if (root.backendRef) {
                        var opId = root.backendRef.perform_sync_diagnostics()
                        root.activeOperationId = opId
                        root.activeOperationKind = "dry_run"
                    }
                }
            }

            AppButton {
                text: qsTr("打开工作区目录")
                dt: root.theme
                variant: "text"
                visible: root.backendRef && root.backendRef.has_workspace
                onClicked: if (root.backendRef) root.backendRef.open_workspace_dir()
            }

            AppButton {
                text: qsTr("复制冲突信息")
                dt: root.theme
                variant: "danger"
                visible: root.backendRef && root.isConflictStatus(root.currentSyncStatus)
                onClicked: if (root.backendRef) root.backendRef.copy_text_to_clipboard(syncResultArea.text)
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 180
            color: root.isFailureStatus(root.currentSyncStatus) ? theme.dangerContainer : theme.surfaceContainerLow
            border.color: root.isFailureStatus(root.currentSyncStatus) ? theme.error : theme.border
            border.width: root.isFailureStatus(root.currentSyncStatus) ? 2 : 1
            radius: theme.radiusLg
            clip: true
            visible: (root.currentSyncOperationState !== "" || root.currentSyncStatus === "syncing" || root.isFailureStatus(root.currentSyncStatus))

            ScrollView {
                id: logScroll
                anchors.fill: parent
                anchors.margins: theme.sp12
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea {
                    id: syncResultArea
                    width: logScroll.availableWidth
                    text: ""
                    color: root.isFailureStatus(root.currentSyncStatus) ? theme.dangerContainer : theme.onSurfaceVariant
                    font.family: "monospace"
                    font.pixelSize: theme.caption
                    readOnly: true
                    background: null
                    wrapMode: TextEdit.Wrap
                }
            }
        }
    }

    Component.onCompleted: {
        if (root.backendRef) {
            root.backendRef.load_sync_config()
        }
        root.refreshLocalSyncState();
    }
}
