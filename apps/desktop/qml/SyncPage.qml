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
    implicitHeight: mainCol.implicitHeight + (theme ? theme.sp32 : 32)
    property int lastSyncResultLen: -1
    property double lastSyncStatusLogTime: 0
    property var theme: null
    property var backendRef: null
    property var beforeSyncHook: null
    signal settingsChanged()

    property string activeOperationId: ""
    property string activeOperationKind: ""

    // ── SystemPalette 推断：theme 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

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
        if (s === "conflict") return qsTr("存在冲突")
        if (s === "partial_conflict") return qsTr("部分同步，存在正文冲突")
        if (s === "dry_run_success") return qsTr("检查成功")
        if (s === "diagnostics_success") return qsTr("诊断成功")
        if (s === "token_missing") return qsTr("未设置 Token")
        if (s === "token_invalid") return qsTr("Token 无效")
        if (s === "token_permission_denied") return qsTr("Token 权限不足")
        if (s === "repo_not_found_or_no_permission") return qsTr("仓库不存在或无权限")
        if (s === "remote_branch_missing" || s === "branch_missing") return qsTr("远程分支不存在")
        if (s === "network_failed") return qsTr("网络连接失败")
        if (root.isFailureStatus(s)) return qsTr("同步失败")
        if (root.backendRef && root.backendRef.sync_enabled) return qsTr("已配置")
        return qsTr("未配置")
    }

    Connections {
        target: appBackend
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
    }

    ColumnLayout {
        id: mainCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: theme ? theme.sp16 : 16
        spacing: theme ? theme.sp16 : 16

        RowLayout {
            Layout.fillWidth: true
            spacing: theme ? theme.sp12 : 12

            ColumnLayout {
                Layout.fillWidth: true
                spacing: theme ? theme.sp4 : 4
                AppText {
                    text: qsTr("同步设置")
                    color: theme ? theme.onBackground : (_inferDark ? "#E2E2E5" : "#1A1C1E")
                    font.pixelSize: theme ? theme.title : 24
                    font.family: theme ? theme.fontFamily : "sans-serif"
                    font.weight: Font.Bold
                }
                AppText {
                    text: qsTr("配置远端仓库并查看同步状态")
                    color: theme ? theme.onSurfaceVariant : (_inferDark ? "#C3C6CF" : "#42474E")
                    font.pixelSize: theme ? theme.body : 14
                    font.family: theme ? theme.fontFamily : "sans-serif"
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
            spacing: theme ? theme.sp16 : 16

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
            spacing: theme ? theme.sp8 : 8

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
                    if (success) root.settingsChanged()
                }
            }

            AppButton {
                text: qsTr("执行同步")
                dt: root.theme
                variant: "secondary"
                enabled: !(appBackend && appBackend.sync_in_progress)
                onClicked: {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") window.debugLog("sync", "perform_sync_clicked", "")
                    syncResultArea.text = qsTr("正在同步...\n正在拉取远端清单\n正在比较本地和远端\n正在下载远端较新文件\n正在上传本地较新文件")
                    if (typeof root.beforeSyncHook === "function") root.beforeSyncHook()
                    if (root.backendRef) {
                        var opId = root.backendRef.perform_sync()
                        root.activeOperationId = opId
                        root.activeOperationKind = "sync"
                    }
                }
            }

            AppButton {
                text: qsTr("运行诊断")
                dt: root.theme
                variant: "secondary"
                enabled: !(appBackend && appBackend.sync_in_progress)
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
            color: root.isFailureStatus(root.currentSyncStatus) ? (theme ? theme.dangerContainer : (_inferDark ? "#93000A" : "#FFDAD6")) : (theme ? theme.surfaceContainerLow : (_inferDark ? "#1F2229" : "#F6F8FB"))
            border.color: root.isFailureStatus(root.currentSyncStatus) ? (theme ? theme.error : (_inferDark ? "#FFB4AB" : "#BA1A1A")) : (theme ? theme.border : (_inferDark ? "#2A2E36" : "#CBD5E1"))
            border.width: root.isFailureStatus(root.currentSyncStatus) ? 2 : 1
            radius: theme ? theme.radiusLg : 16
            clip: true
            visible: (root.currentSyncOperationState !== "" || root.currentSyncStatus === "syncing" || root.isFailureStatus(root.currentSyncStatus))

            ScrollView {
                id: logScroll
                anchors.fill: parent
                anchors.margins: theme ? theme.sp12 : 12
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                TextArea {
                    id: syncResultArea
                    width: logScroll.availableWidth
                    text: ""
                    color: root.isFailureStatus(root.currentSyncStatus) ? (theme ? theme.dangerContainer : (_inferDark ? "#93000A" : "#FFDAD6")) : (theme ? theme.onSurfaceVariant : (_inferDark ? "#C3C6CF" : "#42474E"))
                    font.family: "monospace"
                    font.pixelSize: theme ? theme.caption : 12
                    readOnly: true
                    background: null
                    wrapMode: TextEdit.Wrap
                }
            }
        }
    }

    Component.onCompleted: {
        root.refreshLocalSyncState();
    }
}
