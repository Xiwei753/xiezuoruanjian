import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

ScrollView {
    id: root
    clip: true
    property var backendRef: null
    property var theme: null
    property bool actionInProgress: false
    property bool hasExistingToken: false

    function applySyncFormToBackend() {
        root.backendRef.sync_enabled = syncEnabledCheck.checked
        root.backendRef.sync_remote_url = remoteUrlInput.text
        root.backendRef.sync_branch = branchInput.text
        root.backendRef.sync_auto_sync = autoSyncCheck.checked
        root.backendRef.sync_interval = parseInt(syncIntervalInput.text) || 300
        root.backendRef.sync_proxy_enabled = proxyEnabledCheck.checked
        root.backendRef.sync_proxy_type = proxyTypeCombo.currentText
        root.backendRef.sync_proxy_host = proxyHostInput.text
        root.backendRef.sync_proxy_port = parseInt(proxyPortInput.text) || 0
        root.backendRef.sync_username = usernameInput.text
        if (tokenInput.text.length > 0)
            root.backendRef.set_sync_token(tokenInput.text)
    }

    function loadForm() {
        root.backendRef.load_sync_config()
        syncEnabledCheck.checked = root.backendRef.sync_enabled
        remoteUrlInput.text = root.backendRef.sync_remote_url
        branchInput.text = root.backendRef.sync_branch
        autoSyncCheck.checked = root.backendRef.sync_auto_sync
        syncIntervalInput.text = root.backendRef.sync_interval.toString()
        proxyEnabledCheck.checked = root.backendRef.sync_proxy_enabled
        proxyTypeCombo.currentIndex = proxyTypeCombo.indexOfText(root.backendRef.sync_proxy_type)
        if (proxyTypeCombo.currentIndex === -1) proxyTypeCombo.currentIndex = 0
        proxyHostInput.text = root.backendRef.sync_proxy_host
        proxyPortInput.text = root.backendRef.sync_proxy_port.toString()
        usernameInput.text = root.backendRef.sync_username
        root.hasExistingToken = root.backendRef.has_sync_token
        tokenInput.text = ""
        tokenInput.placeholderText = root.hasExistingToken ? "已配置（输入新 Token 以覆盖）" : "未配置"
        syncResultLabel.text = root.backendRef.sync_action_result
    }

    function startPollTimer() {
        pollTimer.start()
    }

    Connections {
        target: root.backendRef
        function onSync_action_completed() {
            syncResultLabel.text = root.backendRef.sync_action_result
            if (!pollTimer.running) {
                root.actionInProgress = false
            }
        }
        function onSync_status_changed() {
            // Keep the status card updated
        }
    }

    Timer {
        id: pollTimer
        interval: 300
        repeat: true
        onTriggered: {
            if (root.backendRef) {
                root.backendRef.poll_sync_result()
                syncResultLabel.text = root.backendRef.sync_action_result
                if (root.backendRef.sync_status !== "syncing") {
                    pollTimer.stop()
                    root.actionInProgress = false
                }
            }
        }
    }

    property bool githubInitMode: false
    property string githubInitPath: ""

    function startGithubInit() {
        // Called from EmptyWorkspace - prepare for init flow
        root.githubInitMode = true
        root.githubInitPath = ""
        // User selects directory
    }

    Component.onCompleted: loadForm()

    ColumnLayout {
        width: Math.min(parent.width, 560)
        spacing: root.theme ? root.theme.sp16 : 16

        // GitHub Init section (visible when no workspace open)
        ColumnLayout {
            Layout.fillWidth: true
            spacing: root.theme ? root.theme.sp12 : 12
            visible: !root.backendRef || !root.backendRef.has_workspace

            AppCard {
                theme: root.theme; Layout.fillWidth: true
                ColumnLayout {
                    Layout.fillWidth: true; spacing: root.theme ? root.theme.sp8 : 8
                    Label {
                        text: "从 GitHub 初始化工作区"
                        font.pixelSize: root.theme ? root.theme.fontXl : 18
                        font.weight: Font.Bold; color: root.theme ? root.theme.text : "#e2e8f0"
                    }
                    Label {
                        text: "选择本地空目录（或已有工作区目录），填写远程仓库地址和 Token 后初始化。非空非工作区目录将被阻止。"
                        font.pixelSize: root.theme ? root.theme.fontSm : 12
                        color: root.theme ? root.theme.textDim : "#94a3b8"
                        wrapMode: Text.Wrap; Layout.fillWidth: true
                    }

                    RowLayout {
                        Layout.fillWidth: true; spacing: root.theme ? root.theme.sp8 : 8
                        Label {
                            text: "目录:"
                            font.pixelSize: root.theme ? root.theme.fontMd : 13
                            color: root.theme ? root.theme.text : "#e2e8f0"
                        }
                        Label {
                            text: root.backendRef && root.backendRef.pending_github_init_path
                                  ? root.backendRef.pending_github_init_path
                                  : "未选择"
                            font.pixelSize: root.theme ? root.theme.fontMd : 13
                            color: root.backendRef && root.backendRef.pending_github_init_path
                                   ? (root.theme ? root.theme.text : "#e2e8f0")
                                   : (root.theme ? root.theme.textDim : "#94a3b8")
                            Layout.fillWidth: true; elide: Text.ElideRight
                        }
                    }
                    AppButton {
                        theme: root.theme; small: true; text: "选择目录"
                        onClicked: {
                            root.backendRef.init_workspace_from_github()
                        }
                    }

                    AppTextField {
                        theme: root.theme; Layout.fillWidth: true
                        label: "远程仓库地址"
                        placeholder: "https://github.com/user/repo.git"
                        id: initUrlInput
                    }
                    RowLayout {
                        Layout.fillWidth: true; spacing: root.theme ? root.theme.sp8 : 8
                        AppTextField {
                            theme: root.theme; Layout.fillWidth: true
                            label: "分支"
                            placeholder: "main"
                            id: initBranchInput
                        }
                        AppTextField {
                            theme: root.theme; Layout.fillWidth: true
                            label: "Token (GitHub PAT)"
                            placeholder: "输入 Token"
                            id: initTokenInput
                            echoMode: TextInput.Password
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true; spacing: root.theme ? root.theme.sp8 : 8
                        AppTextField {
                            theme: root.theme; Layout.fillWidth: true
                            label: "代理主机 (可选)"
                            placeholder: "127.0.0.1"
                            id: initProxyHostInput
                        }
                        AppTextField {
                            theme: root.theme; Layout.preferredWidth: 100
                            label: "端口"
                            placeholder: "7890"
                            id: initProxyPortInput
                            validator: IntValidator { bottom: 0; top: 65535 }
                        }
                    }

                    AppButton {
                        theme: root.theme; text: "初始化/克隆"
                        enabled: !root.actionInProgress && root.backendRef && root.backendRef.pending_github_init_path.length > 0
                        onClicked: {
                            root.actionInProgress = true
                            root.backendRef.execute_github_init(
                                root.backendRef.pending_github_init_path,
                                initUrlInput.text,
                                initBranchInput.text,
                                initTokenInput.text,
                                initProxyHostInput.text.length > 0 ? "http" : "none",
                                initProxyHostInput.text,
                                parseInt(initProxyPortInput.text) || 0
                            )
                            startPollTimer()
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true; height: 1
                color: root.theme ? root.theme.divider : "#334155"
            }
        }

        // Sync status card
        AppCard {
            theme: root.theme; Layout.fillWidth: true
            RowLayout {
                Layout.fillWidth: true
                spacing: root.theme ? root.theme.sp12 : 12

                Rectangle {
                    width: 10; height: 10; radius: 5
                    color: {
                        var ss = root.backendRef ? root.backendRef.sync_status : "not_configured"
                        if (ss === "success") return root.theme ? root.theme.success : "#22c55e"
                        if (ss === "syncing") return root.theme ? root.theme.warning : "#f59e0b"
                        if (ss === "auth_failed") return root.theme ? root.theme.danger : "#ef4444"
                        if (ss === "network_failed") return root.theme ? root.theme.danger : "#ef4444"
                        if (ss === "conflict") return root.theme ? root.theme.danger : "#ef4444"
                        if (ss === "configured_untested") return root.theme ? root.theme.warning : "#f59e0b"
                        return root.theme ? root.theme.textDim : "#94a3b8"
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Label {
                        text: {
                            var ss = root.backendRef ? root.backendRef.sync_status : "not_configured"
                            if (ss === "not_configured") return "未配置"
                            if (ss === "configured_untested") return "已配置，未测试"
                            if (ss === "syncing") return "正在同步"
                            if (ss === "success") return "同步成功"
                            if (ss === "auth_failed") return "认证失败"
                            if (ss === "network_failed") return "网络/代理失败"
                            if (ss === "conflict") return "冲突需要处理"
                            return "未知状态"
                        }
                        font.pixelSize: root.theme ? root.theme.fontMd : 13
                        font.weight: Font.Medium
                        color: root.theme ? root.theme.text : "#e2e8f0"
                    }
                    Label {
                        text: {
                            var ss = root.backendRef ? root.backendRef.sync_status : "not_configured"
                            if (ss === "not_configured") return "请先配置同步后启用"
                            if (ss === "configured_untested") return "配置已保存，点击「测试连接」验证"
                            return ""
                        }
                        font.pixelSize: root.theme ? root.theme.fontSm : 12
                        color: root.theme ? root.theme.textDim : "#94a3b8"
                        visible: text.length > 0
                    }
                }
            }
        }

        SectionHeader { theme: root.theme; text: "同步设置" }

        AppCard {
            theme: root.theme; Layout.fillWidth: true
            SettingsRow {
                theme: root.theme; isSwitch: true
                label: "启用同步"
                description: "启用后可将作品同步到远程仓库"
                id: syncEnabledCheck
            }
        }

        AppCard {
            theme: root.theme; Layout.fillWidth: true
            RowLayout {
                Layout.fillWidth: true
                spacing: root.theme ? root.theme.sp12 : 12
                Label {
                    text: "后端:"
                    font.pixelSize: root.theme ? root.theme.fontMd : 13
                    color: root.theme ? root.theme.text : "#e2e8f0"
                    Layout.preferredWidth: 80
                }
                Label {
                    text: "Git / GitHub 仓库"
                    font.pixelSize: root.theme ? root.theme.fontMd : 13
                    color: root.theme ? root.theme.text : "#e2e8f0"
                    font.weight: Font.Medium
                }
            }
        }

        AppCard {
            theme: root.theme; Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: root.theme ? root.theme.sp8 : 8
                AppTextField {
                    theme: root.theme; Layout.fillWidth: true
                    label: "GitHub 仓库地址"
                    placeholder: "https://github.com/user/repo.git"
                    id: remoteUrlInput
                }
                AppTextField {
                    theme: root.theme; Layout.fillWidth: true
                    label: "分支"
                    placeholder: "main"
                    id: branchInput
                }
                AppTextField {
                    theme: root.theme; Layout.fillWidth: true
                    label: "GitHub 用户名（可选）"
                    placeholder: "留空则使用 x-access-token"
                    id: usernameInput
                }
            }
        }

        AppCard {
            theme: root.theme; Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: root.theme ? root.theme.sp8 : 8
                Label {
                    text: "Token"
                    font.pixelSize: root.theme ? root.theme.fontMd : 13
                    color: root.theme ? root.theme.text : "#e2e8f0"
                    font.weight: Font.Medium
                }
                TextField {
                    id: tokenInput
                    Layout.fillWidth: true; implicitHeight: 32
                    echoMode: TextInput.Password
                    placeholderText: root.hasExistingToken ? "已配置（输入新 Token 以覆盖）" : "未配置"
                    color: root.theme ? root.theme.text : "#e2e8f0"
                    background: Rectangle {
                        color: root.theme ? root.theme.surface : "#1a1a2e"
                        border.color: parent.activeFocus ? (root.theme ? root.theme.borderFocus : "#0ea5e9") : (root.theme ? root.theme.border : "#334155")
                        border.width: 1
                        radius: root.theme ? root.theme.radiusSm : 4
                    }
                    font.pixelSize: root.theme ? root.theme.fontMd : 13
                    leftPadding: 8; topPadding: 6; bottomPadding: 6
                }
                Label {
                    text: root.hasExistingToken ? "Token 已配置" : "Token 未配置，同步将无法进行"
                    font.pixelSize: root.theme ? root.theme.fontXs : 11
                    color: root.hasExistingToken ? (root.theme ? root.theme.success : "#22c55e") : (root.theme ? root.theme.warning : "#f59e0b")
                }
            }
        }

        AppCard {
            theme: root.theme; Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: root.theme ? root.theme.sp8 : 8
                SettingsRow {
                    theme: root.theme; isSwitch: true
                    label: "自动同步"
                    description: "按间隔时间自动执行同步"
                    id: autoSyncCheck
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: root.theme ? root.theme.sp8 : 8
                    visible: autoSyncCheck.checked
                    Label {
                        text: "同步间隔 (秒):"
                        font.pixelSize: root.theme ? root.theme.fontMd : 13
                        color: root.theme ? root.theme.text : "#e2e8f0"
                    }
                    TextField {
                        id: syncIntervalInput
                        Layout.preferredWidth: 100; implicitHeight: 30
                        validator: IntValidator { bottom: 60 }
                        color: root.theme ? root.theme.text : "#e2e8f0"
                        background: Rectangle {
                            color: root.theme ? root.theme.surface : "#1a1a2e"
                            border.color: parent.activeFocus ? (root.theme ? root.theme.borderFocus : "#0ea5e9") : (root.theme ? root.theme.border : "#334155")
                            border.width: 1
                            radius: root.theme ? root.theme.radiusSm : 4
                        }
                        font.pixelSize: root.theme ? root.theme.fontMd : 13
                        leftPadding: 8; topPadding: 6; bottomPadding: 6
                    }
                }
            }
        }

        AppCard {
            theme: root.theme; Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: root.theme ? root.theme.sp8 : 8
                Label {
                    text: "代理设置（可选）"
                    font.pixelSize: root.theme ? root.theme.fontMd : 13
                    color: root.theme ? root.theme.text : "#e2e8f0"
                    font.weight: Font.Medium
                }
                SettingsRow {
                    theme: root.theme; isSwitch: true
                    label: "启用应用内代理"
                    description: "兜底代理，默认关闭"
                    id: proxyEnabledCheck
                }
                RowLayout {
                    Layout.fillWidth: true; spacing: 8
                    visible: proxyEnabledCheck.checked
                    Label { text: "类型:"; font.pixelSize: 12; color: root.theme ? root.theme.textDim : "#94a3b8" }
                    ComboBox {
                        id: proxyTypeCombo; model: ["none", "auto", "http", "socks5"]
                        Layout.preferredWidth: 100
                        onCurrentTextChanged: {
                            if (currentText === "http" && proxyPortInput.text === "0") proxyPortInput.text = "7890"
                            else if (currentText === "socks5" && proxyPortInput.text === "0") proxyPortInput.text = "7891"
                        }
                        contentItem: Text {
                            text: parent.displayText; color: root.theme ? root.theme.text : "#e2e8f0"
                            font.pixelSize: 12; leftPadding: 6; verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            color: root.theme ? root.theme.surface : "#1a1a2e"
                            border.color: parent.activeFocus ? (root.theme ? root.theme.borderFocus : "#0ea5e9") : (root.theme ? root.theme.border : "#334155")
                            border.width: 1; radius: root.theme ? root.theme.radiusSm : 4
                        }
                    }
                }
                RowLayout {
                    visible: proxyEnabledCheck.checked; spacing: 8
                    Label { text: "Host:"; font.pixelSize: 12; color: root.theme ? root.theme.textDim : "#94a3b8" }
                    TextField {
                        id: proxyHostInput; Layout.preferredWidth: 120; implicitHeight: 28
                        placeholderText: "127.0.0.1"
                        color: root.theme ? root.theme.text : "#e2e8f0"
                        background: Rectangle {
                            color: root.theme ? root.theme.surface : "#1a1a2e"
                            border.color: parent.activeFocus ? (root.theme ? root.theme.borderFocus : "#0ea5e9") : (root.theme ? root.theme.border : "#334155")
                            border.width: 1; radius: root.theme ? root.theme.radiusSm : 4
                        }
                        font.pixelSize: 12; leftPadding: 6; topPadding: 4; bottomPadding: 4
                    }
                    Label { text: "Port:"; font.pixelSize: 12; color: root.theme ? root.theme.textDim : "#94a3b8" }
                    TextField {
                        id: proxyPortInput; Layout.preferredWidth: 80; implicitHeight: 28
                        validator: IntValidator { bottom: 0; top: 65535 }
                        color: root.theme ? root.theme.text : "#e2e8f0"
                        background: Rectangle {
                            color: root.theme ? root.theme.surface : "#1a1a2e"
                            border.color: parent.activeFocus ? (root.theme ? root.theme.borderFocus : "#0ea5e9") : (root.theme ? root.theme.border : "#334155")
                            border.width: 1; radius: root.theme ? root.theme.radiusSm : 4
                        }
                        font.pixelSize: 12; leftPadding: 6; topPadding: 4; bottomPadding: 4
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: root.theme ? root.theme.sp8 : 8
            AppButton { theme: root.theme; small: true; text: "保存配置"; enabled: !root.actionInProgress
                onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (!root.backendRef.save_sync_config()) { root.actionInProgress = false } }
            }
            AppButton { theme: root.theme; small: true; text: "测试连接"; enabled: !root.actionInProgress
                onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (root.backendRef.save_sync_config()) { root.backendRef.perform_sync_diagnostics(); startPollTimer() } else { root.actionInProgress = false } }
            }
            AppButton { theme: root.theme; small: true; text: "同步计划"; enabled: !root.actionInProgress
                onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (root.backendRef.save_sync_config()) { root.backendRef.perform_sync_dry_run() } else { root.actionInProgress = false } }
            }
            AppButton { theme: root.theme; small: true; text: "立即同步"; enabled: !root.actionInProgress
                onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (root.backendRef.save_sync_config()) { root.backendRef.perform_sync(); startPollTimer() } else { root.actionInProgress = false } }
            }
        }

        AppCard {
            theme: root.theme; Layout.fillWidth: true
            Label {
                text: "提示：首次同步时如果远程分支不存在，系统将自动创建分支并推送初始内容。\n当前使用 Git / libgit2 后端，由本地 Git 仓库直接操作。"
                font.pixelSize: root.theme ? root.theme.fontXs : 11
                color: root.theme ? root.theme.textDim : "#94a3b8"
                wrapMode: Text.Wrap; Layout.fillWidth: true
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(syncResultLabel.implicitHeight + 20, 200)
            clip: true
            TextArea {
                id: syncResultLabel; width: parent.width
                readOnly: true; wrapMode: Text.Wrap
                color: root.theme ? root.theme.text : "#e2e8f0"
                font.pixelSize: root.theme ? root.theme.fontSm : 12
                background: Rectangle {
                    color: root.theme ? root.theme.surfaceAlt : "#16213e"
                    radius: root.theme ? root.theme.radiusSm : 4
                    border.color: root.theme ? root.theme.border : "#334155"
                    border.width: 1
                }
                leftPadding: 8; topPadding: 8; rightPadding: 8; bottomPadding: 8
            }
        }

        Item { Layout.fillHeight: true; height: root.theme ? root.theme.sp16 : 16 }
    }
}
