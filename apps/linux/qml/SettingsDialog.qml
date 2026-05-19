import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Popup {
    id: root
    property var backendRef: null
    property var editorPageRef: null
    property bool actionInProgress: false
    property bool hasExistingToken: false

    width: 650
    height: 700
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    anchors.centerIn: parent

    function applySyncFormToBackend() {
        root.backendRef.sync_enabled = syncEnabledCheck.checked
        root.backendRef.sync_remote_url = remoteUrlInput.text
        root.backendRef.sync_branch = branchInput.text
        root.backendRef.sync_auto_sync = autoSyncCheck.checked
        root.backendRef.sync_interval = parseInt(syncIntervalInput.text) || 300
        root.backendRef.sync_proxy_type = proxyTypeCombo.currentText
        root.backendRef.sync_proxy_host = proxyHostInput.text
        root.backendRef.sync_proxy_port = parseInt(proxyPortInput.text) || 0
        if (tokenInput.text.length > 0) {
            root.backendRef.sync_token = tokenInput.text
        }
    }

    function applyEditorFormToBackend() {
        root.backendRef.setting_font_size = fontSizeSpin.value
        root.backendRef.setting_line_spacing = lineSpacingSpin.value / 100.0
        root.backendRef.setting_auto_save_enabled = autoSaveCheck.checked
        root.backendRef.setting_auto_save_delay_ms = autoSaveDelaySpin.value
        root.backendRef.setting_auto_indent_enabled = autoIndentCheck.checked
        root.backendRef.setting_auto_indent_width = autoIndentWidthSpin.value / 100.0
        root.backendRef.setting_theme_mode = themeModeCombo.currentText
        root.backendRef.setting_typing_animation_enabled = typingAnimCheck.checked
        root.backendRef.setting_smooth_cursor_enabled = smoothCursorCheck.checked
    }

    onAboutToShow: {
        root.backendRef.load_local_settings()
        fontSizeSpin.value = root.backendRef.setting_font_size > 0 ? root.backendRef.setting_font_size : 16
        lineSpacingSpin.value = root.backendRef.setting_line_spacing > 0 ? root.backendRef.setting_line_spacing * 100 : 150
        autoSaveCheck.checked = root.backendRef.setting_auto_save_enabled
        autoSaveDelaySpin.value = root.backendRef.setting_auto_save_delay_ms > 0 ? root.backendRef.setting_auto_save_delay_ms : 1500
        autoIndentCheck.checked = root.backendRef.setting_auto_indent_enabled
        autoIndentWidthSpin.value = root.backendRef.setting_auto_indent_width > 0 ? root.backendRef.setting_auto_indent_width * 100 : 200
        typingAnimCheck.checked = root.backendRef.setting_typing_animation_enabled
        smoothCursorCheck.checked = root.backendRef.setting_smooth_cursor_enabled

        var modes = ["system", "light", "dark"]
        themeModeCombo.currentIndex = modes.indexOf(root.backendRef.setting_theme_mode)
        if (themeModeCombo.currentIndex === -1) themeModeCombo.currentIndex = 0

        root.backendRef.load_sync_config()
        syncEnabledCheck.checked = root.backendRef.sync_enabled
        remoteUrlInput.text = root.backendRef.sync_remote_url
        branchInput.text = root.backendRef.sync_branch
        autoSyncCheck.checked = root.backendRef.sync_auto_sync
        syncIntervalInput.text = root.backendRef.sync_interval.toString()
        proxyTypeCombo.currentIndex = proxyTypeCombo.indexOfText(root.backendRef.sync_proxy_type)
        if (proxyTypeCombo.currentIndex === -1) proxyTypeCombo.currentIndex = 0
        proxyHostInput.text = root.backendRef.sync_proxy_host
        proxyPortInput.text = root.backendRef.sync_proxy_port.toString()
        root.hasExistingToken = (root.backendRef.sync_token.length > 0)
        tokenInput.text = ""
        tokenInput.placeholderText = root.hasExistingToken ? "已配置（输入新 Token 以覆盖）" : "未配置"
        syncResultLabel.text = root.backendRef.sync_action_result
        actionResultText.text = ""
    }

    Connections {
        target: root.backendRef
        function onSync_action_completed() {
            syncResultLabel.text = root.backendRef.sync_action_result
            root.actionInProgress = false
        }
    }

    ColumnLayout {
        anchors.fill: parent

        TabBar {
            id: settingsTabBar
            Layout.fillWidth: true
            TabButton { text: "编辑器设置" }
            TabButton { text: "同步设置" }
            TabButton { text: "Action 调试" }
            TabButton { text: "关于" }
        }

        StackLayout {
            currentIndex: settingsTabBar.currentIndex
            Layout.fillWidth: true
            Layout.fillHeight: true

            // Tab 1: Editor Settings
            ScrollView {
                clip: true
                ColumnLayout {
                    width: parent.width - 20
                    spacing: 12

                    Label { text: "字号:" }
                    SpinBox {
                        id: fontSizeSpin
                        from: 10
                        to: 72
                        value: 16
                    }

                    Label { text: "行距: " + (lineSpacingSpin.value / 100).toFixed(2) + "x" }
                    SpinBox {
                        id: lineSpacingSpin
                        from: 100
                        to: 300
                        value: 150
                        stepSize: 10
                    }

                    CheckBox {
                        id: autoSaveCheck
                        text: "自动保存"
                    }

                    Label { text: "自动保存延迟 (毫秒):" }
                    SpinBox {
                        id: autoSaveDelaySpin
                        from: 500
                        to: 60000
                        stepSize: 500
                        value: 1500
                    }

                    CheckBox {
                        id: autoIndentCheck
                        text: "自动缩进"
                    }

                    Label { text: "自动缩进宽度 (字符数): " + (autoIndentWidthSpin.value / 100).toFixed(1) }
                    SpinBox {
                        id: autoIndentWidthSpin
                        from: 0
                        to: 800
                        value: 200
                        stepSize: 50
                    }

                    CheckBox {
                        id: typingAnimCheck
                        text: "输入动画"
                    }

                    CheckBox {
                        id: smoothCursorCheck
                        text: "平滑光标"
                    }

                    Label { text: "主题模式:" }
                    ComboBox {
                        id: themeModeCombo
                        Layout.fillWidth: true
                        model: ["system", "light", "dark"]
                    }
                    Label {
                        text: "Linux 端当前仅支持暗色主题，设置值将同步至其他平台"
                        color: "gray"
                        font.pixelSize: 12
                        wrapMode: Text.Wrap
                    }

                    Button {
                        text: "保存设置"
                        enabled: !root.actionInProgress
                        onClicked: {
                            applyEditorFormToBackend()
                            if (!root.backendRef.save_local_settings()) {
                                // Error dialog is in main.qml, signal it
                            }
                        }
                    }
                }
            }

            // Tab 2: Sync Settings
            ScrollView {
                clip: true
                ColumnLayout {
                    width: parent.width - 20
                    spacing: 10

                    CheckBox {
                        id: syncEnabledCheck
                        text: "启用同步"
                    }

                    Label { text: "GitHub 仓库地址:" }
                    TextField {
                        id: remoteUrlInput
                        Layout.fillWidth: true
                        placeholderText: "https://github.com/user/repo.git"
                    }

                    Label { text: "分支:" }
                    TextField {
                        id: branchInput
                        Layout.fillWidth: true
                        placeholderText: "main"
                    }

                    CheckBox {
                        id: autoSyncCheck
                        text: "自动同步"
                    }

                    Label { text: "同步间隔 (秒):" }
                    TextField {
                        id: syncIntervalInput
                        Layout.fillWidth: true
                        validator: IntValidator { bottom: 60 }
                    }

                    Label { text: "代理类型:" }
                    ComboBox {
                        id: proxyTypeCombo
                        Layout.fillWidth: true
                        model: ["none", "auto", "http", "socks5"]
                        onCurrentTextChanged: {
                            if (currentText === "http" && proxyPortInput.text === "0") {
                                proxyPortInput.text = "7890"
                            } else if (currentText === "socks5" && proxyPortInput.text === "0") {
                                proxyPortInput.text = "7891"
                            }
                        }
                    }

                    Label { text: "代理 Host:" }
                    TextField {
                        id: proxyHostInput
                        Layout.fillWidth: true
                        placeholderText: "127.0.0.1"
                    }

                    Label { text: "代理 Port:" }
                    TextField {
                        id: proxyPortInput
                        Layout.fillWidth: true
                        validator: IntValidator { bottom: 0; top: 65535 }
                    }

                    Label { text: "Token (Personal Access Token):" }
                    TextField {
                        id: tokenInput
                        Layout.fillWidth: true
                        echoMode: TextInput.Password
                    }
                    Label {
                        id: tokenStatusLabel
                        color: root.hasExistingToken ? "#4caf50" : "#ff9800"
                        font.pixelSize: 12
                        text: root.hasExistingToken ? "Token 已配置" : "Token 未配置，同步将无法进行"
                    }

                    RowLayout {
                        Layout.fillWidth: true

                        Button {
                            text: "保存"
                            enabled: !root.actionInProgress
                            onClicked: {
                                applySyncFormToBackend()
                                root.actionInProgress = true
                                if (root.backendRef.save_sync_config()) {
                                    root.actionInProgress = false
                                    syncResultLabel.text = "配置已保存"
                                } else {
                                    root.actionInProgress = false
                                }
                            }
                        }

                        Button {
                            text: "测试连接"
                            enabled: !root.actionInProgress
                            onClicked: {
                                applySyncFormToBackend()
                                root.actionInProgress = true
                                if (root.backendRef.save_sync_config()) {
                                    root.backendRef.perform_sync_diagnostics()
                                } else {
                                    root.actionInProgress = false
                                }
                            }
                        }

                        Button {
                            text: "同步计划"
                            enabled: !root.actionInProgress
                            onClicked: {
                                applySyncFormToBackend()
                                root.actionInProgress = true
                                if (root.backendRef.save_sync_config()) {
                                    root.backendRef.perform_sync_dry_run()
                                } else {
                                    root.actionInProgress = false
                                }
                            }
                        }
                        Button {
                            text: "立即同步"
                            enabled: !root.actionInProgress
                            onClicked: {
                                applySyncFormToBackend()
                                root.actionInProgress = true
                                if (root.backendRef.save_sync_config()) {
                                    root.backendRef.perform_sync()
                                } else {
                                    root.actionInProgress = false
                                }
                            }
                        }
                    }

                    Label {
                        text: "提示：首次同步时如果远程分支不存在，系统将自动创建分支并推送初始内容。"
                        color: "#90a4ae"
                        font.pixelSize: 11
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }

                    TextArea {
                        id: syncResultLabel
                        Layout.fillWidth: true
                        readOnly: true
                        wrapMode: Text.Wrap
                        background: Rectangle { color: "#2a2a2a"; radius: 4 }
                        color: "#e0e0e0"
                        font.pixelSize: 12
                    }
                }
            }

            // Tab 3: Action Registry Debug
            ActionRegistryPage {
                backendRef: root.backendRef
                fontSizeSpinValue: fontSizeSpin.value
                autoSaveCheckChecked: autoSaveCheck.checked
                autoSaveDelaySpinValue: autoSaveDelaySpin.value
                lineSpacingSpinValue: lineSpacingSpin.value
                autoIndentCheckChecked: autoIndentCheck.checked
                autoIndentWidthSpinValue: autoIndentWidthSpin.value
                typingAnimCheckChecked: typingAnimCheck.checked
                smoothCursorCheckChecked: smoothCursorCheck.checked
            }

            // Tab 4: About
            Item {
                ColumnLayout {
                    anchors.centerIn: parent
                    Label {
                        text: "Writer Application (Linux)"
                        font.pixelSize: 20
                        font.bold: true
                        horizontalAlignment: Text.AlignHCenter
                    }
                    Label {
                        text: "Version 1.0.0"
                        horizontalAlignment: Text.AlignHCenter
                    }
                    Label {
                        text: "技术栈: Rust Core + qmetaobject + Qt/QML"
                        color: "gray"
                        horizontalAlignment: Text.AlignHCenter
                    }
                }
            }
        }
    }
}
