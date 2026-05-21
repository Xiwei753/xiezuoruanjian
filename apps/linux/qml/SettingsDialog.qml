import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Popup {
    id: root
    property var backendRef: null
    property var editorPageRef: null
    property var theme: null
    property bool actionInProgress: false
    property bool hasExistingToken: false
    property int currentCategory: 0

    width: 720
    height: 580
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    x: Math.round((parent.width - width) / 2)
    y: Math.round((parent.height - height) / 2)
    background: Rectangle { color: theme.surface; radius: theme.radiusLg }

    function switchToCategory(index) { currentCategory = index }

    // ── Form helpers ──────────────────────────────────────────────

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
        if (tokenInput.text.length > 0) {
            root.backendRef.sync_token = tokenInput.text
        }
    }

    function applyEditorFormToBackend() {
        root.backendRef.setting_font_size = fontSizeSlider.value
        root.backendRef.setting_line_spacing = lineSpacingSlider.value / 100.0
        root.backendRef.setting_auto_save_enabled = autoSaveCheck.checked
        root.backendRef.setting_auto_save_delay_ms = autoSaveDelaySlider.value
        root.backendRef.setting_auto_indent_enabled = autoIndentCheck.checked
        root.backendRef.setting_auto_indent_width = autoIndentWidthSlider.value / 100.0
        root.backendRef.setting_theme_mode = themeCombo.currentText
        root.backendRef.setting_typing_animation_enabled = typingAnimCheck.checked
        root.backendRef.setting_smooth_cursor_enabled = smoothCursorCheck.checked
    }

    function loadFormFromBackend() {
        root.backendRef.load_local_settings()

        var themeMode = root.backendRef.setting_theme_mode
        var modes = ["system", "light", "dark"]
        themeCombo.currentIndex = modes.indexOf(themeMode)
        if (themeCombo.currentIndex === -1) themeCombo.currentIndex = 0

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
        root.hasExistingToken = (root.backendRef.sync_token.length > 0)
        tokenInput.text = ""
        tokenInput.placeholderText = root.hasExistingToken ? "已配置（输入新 Token 以覆盖）" : "未配置"
        syncResultLabel.text = root.backendRef.sync_action_result
        actionResultText.text = ""
    }

    onAboutToShow: loadFormFromBackend()

    Connections {
        target: root.backendRef
        function onSync_action_completed() {
            syncResultLabel.text = root.backendRef.sync_action_result
            root.actionInProgress = false
        }
    }

    // ═══════════════════════════════════════════════════════════════
    //  LAYOUT: Sidebar categories + Content panel
    // ═══════════════════════════════════════════════════════════════

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // ── Left Category Sidebar ──────────────────────────────────

        Rectangle {
            Layout.preferredWidth: 160
            Layout.fillHeight: true
            color: theme.surfaceAlt
            radius: theme.radiusLg

            ColumnLayout {
                anchors.fill: parent
                anchors.topMargin: theme.sp16
                spacing: theme.sp2

                Label {
                    text: "设置"
                    font.pixelSize: theme.fontXl
                    font.weight: Font.Bold
                    color: theme.text
                    Layout.leftMargin: theme.sp16
                    Layout.bottomMargin: theme.sp12
                }

                SettingsCategoryTab {
                    text: "编辑器"; icon: "✏️"; active: currentCategory === 0
                    onClicked: currentCategory = 0
                }
                SettingsCategoryTab {
                    text: "外观"; icon: "🎨"; active: currentCategory === 1
                    onClicked: currentCategory = 1
                }
                SettingsCategoryTab {
                    text: "同步"; icon: "🔄"; active: currentCategory === 2
                    onClicked: currentCategory = 2
                }
                SettingsCategoryTab {
                    text: "动效"; icon: "✨"; active: currentCategory === 3
                    onClicked: currentCategory = 3
                }
                SettingsCategoryTab {
                    text: "调试"; icon: "🐛"; active: currentCategory === 4
                    onClicked: currentCategory = 4
                }
                SettingsCategoryTab {
                    text: "关于"; icon: "ℹ️"; active: currentCategory === 5
                    onClicked: currentCategory = 5
                }

                Item { Layout.fillHeight: true }
            }
        }

        Rectangle { width: 1; color: theme.divider; Layout.fillHeight: true }

        // ── Right Content Panel ────────────────────────────────────

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: theme.surface

            StackLayout {
                anchors.fill: parent
                anchors.margins: theme.sp16
                currentIndex: currentCategory

                // ── Tab 0: Editor ──────────────────────────────────
                ScrollView {
                    clip: true
                    ScrollBar.vertical.policy: ScrollBar.AsNeeded

                    ColumnLayout {
                        width: parent.width
                        spacing: theme.sp16

                        SettingsSection { title: "编辑" }
                        SettingsCard {
                            FormSlider {
                                label: "字号"; valueLabel: fontSizeSlider.value.toFixed(0) + "px"
                                from: 10; to: 72; value: root.backendRef.setting_font_size > 0 ? root.backendRef.setting_font_size : 16
                                id: fontSizeSlider
                            }
                        }

                        SettingsCard {
                            FormSlider {
                                label: "行距"; valueLabel: (lineSpacingSlider.value / 100).toFixed(2) + "x"
                                from: 100; to: 300; value: 150; stepSize: 10
                                id: lineSpacingSlider
                            }
                        }

                        SettingsCard {
                            FormSwitch {
                                label: "自动缩进"; description: "自动在换行时添加缩进"
                                id: autoIndentCheck
                            }
                            RowLayout {
                                Layout.fillWidth: true; Layout.leftMargin: 24; spacing: theme.sp8
                                visible: autoIndentCheck.checked
                                Label { text: "缩进宽度:"; font.pixelSize: theme.fontSm; color: theme.textDim }
                                Slider {
                                    id: autoIndentWidthSlider
                                    from: 0; to: 800; value: 200; stepSize: 50
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 24
                                }
                                Label {
                                    text: (autoIndentWidthSlider.value / 100).toFixed(1) + " 字符"
                                    font.pixelSize: theme.fontSm; color: theme.textDim
                                }
                            }
                        }

                        SettingsSection { title: "保存" }
                        SettingsCard {
                            FormSwitch {
                                label: "自动保存"; description: "在编辑内容变化时自动保存到磁盘"
                                id: autoSaveCheck
                            }
                            RowLayout {
                                Layout.fillWidth: true; Layout.leftMargin: 24; spacing: theme.sp8
                                visible: autoSaveCheck.checked
                                Label { text: "延迟:"; font.pixelSize: theme.fontSm; color: theme.textDim }
                                Slider {
                                    id: autoSaveDelaySlider
                                    from: 500; to: 60000; value: 1500; stepSize: 500
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 24
                                }
                                Label {
                                    text: autoSaveDelaySlider.value.toFixed(0) + " ms"
                                    font.pixelSize: theme.fontSm; color: theme.textDim; Layout.preferredWidth: 60
                                }
                            }
                        }

                        SettingsSection { title: "主题" }
                        SettingsCard {
                            RowLayout {
                                Layout.fillWidth: true; spacing: theme.sp12
                                Label { text: "主题模式:"; font.pixelSize: theme.fontMd; color: theme.text }
                                ComboBox {
                                    id: themeCombo
                                    model: ["system", "light", "dark"]
                                    Layout.preferredWidth: 120
                                    contentItem: Text {
                                        text: parent.displayText; color: theme.text; font.pixelSize: theme.fontMd
                                        leftPadding: 8; verticalAlignment: Text.AlignVCenter
                                    }
                                    background: Rectangle {
                                        color: theme.surfaceAlt; border.color: parent.activeFocus ? theme.borderFocus : theme.border
                                        border.width: 1; radius: theme.radiusSm
                                    }
                                }
                            }
                        }

                        Item { height: theme.sp8 }
                        Button {
                            text: "保存设置"
                            Layout.alignment: Qt.AlignLeft
                            implicitHeight: 36; implicitWidth: 120
                            enabled: !root.actionInProgress
                            onClicked: {
                                applyEditorFormToBackend()
                                root.backendRef.save_local_settings()
                            }
                            contentItem: Text {
                                text: parent.text; color: theme.primaryText; font.pixelSize: theme.fontMd
                                horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                color: parent.enabled ? (parent.hovered ? theme.primaryHover : theme.primary) : theme.border
                                radius: theme.radiusSm
                            }
                        }

                        Item { height: theme.sp16 }
                    }
                }

                // ── Tab 1: Appearance ──────────────────────────────
                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: parent.width; spacing: theme.sp16

                        SettingsSection { title: "界面" }
                        SettingsCard {
                            FormSwitch {
                                label: "自动保存时显示保存状态"
                                description: "在底部状态栏显示当前保存状态指示"
                                checked: true
                                enabled: false
                            }
                        }

                        SettingsSection { title: "关于外观" }
                        SettingsCard {
                            Label {
                                text: "当前主题: " + (theme.mode === "light" ? "浅色" : "深色")
                                font.pixelSize: theme.fontMd; color: theme.text
                            }
                            Label {
                                text: "设计系统: 统一间距/圆角/色彩体系"
                                font.pixelSize: theme.fontSm; color: theme.textDim; wrapMode: Text.Wrap
                                Layout.fillWidth: true
                            }
                        }
                        Item { height: theme.sp16 }
                    }
                }

                // ── Tab 2: Sync ────────────────────────────────────
                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: parent.width; spacing: theme.sp16

                        SettingsSection { title: "同步设置" }

                        SettingsCard {
                            FormSwitch {
                                label: "启用同步"; description: "启用后可将作品同步到远程仓库"
                                id: syncEnabledCheck
                            }
                        }

                        SettingsCard {
                            RowLayout {
                                Layout.fillWidth: true; spacing: theme.sp12
                                Label { text: "后端:"; font.pixelSize: theme.fontMd; color: theme.text; Layout.preferredWidth: 80 }
                                Label { text: "GitHub API (推荐)"; font.pixelSize: theme.fontMd; color: theme.text; font.weight: Font.Medium }
                            }
                        }

                        SettingsCard {
                            ColumnLayout {
                                Layout.fillWidth: true; spacing: theme.sp8
                                FormField { label: "GitHub 仓库地址"; placeholder: "https://github.com/user/repo.git"; id: remoteUrlInput }
                                FormField { label: "分支"; placeholder: "main"; id: branchInput }
                                FormField { label: "GitHub 用户名（可选）"; placeholder: "留空则使用 x-access-token"; id: usernameInput }
                            }
                        }

                        SettingsCard {
                            ColumnLayout {
                                Layout.fillWidth: true; spacing: theme.sp8
                                Label { text: "Token"; font.pixelSize: theme.fontMd; color: theme.text; font.weight: Font.Medium }
                                TextField {
                                    id: tokenInput
                                    Layout.fillWidth: true
                                    echoMode: TextInput.Password
                                    placeholderText: root.hasExistingToken ? "已配置（输入新 Token 以覆盖）" : "未配置"
                                    color: theme.text
                                    background: Rectangle {
                                        color: theme.surfaceAlt; border.color: parent.activeFocus ? theme.borderFocus : theme.border
                                        border.width: 1; radius: theme.radiusSm
                                    }
                                    font.pixelSize: theme.fontMd
                                    leftPadding: theme.sp8; topPadding: theme.sp8; bottomPadding: theme.sp8
                                }
                                Label {
                                    text: root.hasExistingToken ? "✓ Token 已配置" : "⚠ Token 未配置，同步将无法进行"
                                    font.pixelSize: theme.fontXs
                                    color: root.hasExistingToken ? theme.success : theme.warning
                                }
                            }
                        }

                        SettingsCard {
                            ColumnLayout {
                                Layout.fillWidth: true; spacing: theme.sp8
                                FormSwitch { label: "自动同步"; description: "按间隔时间自动执行同步"; id: autoSyncCheck }
                                RowLayout {
                                    Layout.fillWidth: true; spacing: theme.sp8; visible: autoSyncCheck.checked
                                    Label { text: "同步间隔 (秒):"; font.pixelSize: theme.fontMd; color: theme.text }
                                    TextField {
                                        id: syncIntervalInput
                                        Layout.preferredWidth: 100
                                        validator: IntValidator { bottom: 60 }
                                        color: theme.text
                                        background: Rectangle {
                                            color: theme.surfaceAlt; border.color: parent.activeFocus ? theme.borderFocus : theme.border
                                            border.width: 1; radius: theme.radiusSm
                                        }
                                        font.pixelSize: theme.fontMd
                                        leftPadding: theme.sp8; topPadding: theme.sp6; bottomPadding: theme.sp6
                                    }
                                }
                            }
                        }

                        SettingsCard {
                            ColumnLayout {
                                Layout.fillWidth: true; spacing: theme.sp8
                                Label { text: "代理设置（可选）"; font.pixelSize: theme.fontMd; color: theme.text; font.weight: Font.Medium }
                                FormSwitch { label: "启用应用内代理"; description: "兜底代理，默认关闭"; id: proxyEnabledCheck }
                                RowLayout {
                                    Layout.fillWidth: true; spacing: theme.sp8; visible: proxyEnabledCheck.checked
                                    Label { text: "类型:"; font.pixelSize: theme.fontSm; color: theme.textDim }
                                    ComboBox {
                                        id: proxyTypeCombo
                                        model: ["none", "auto", "http", "socks5"]
                                        Layout.preferredWidth: 100
                                        onCurrentTextChanged: {
                                            if (currentText === "http" && proxyPortInput.text === "0") proxyPortInput.text = "7890"
                                            else if (currentText === "socks5" && proxyPortInput.text === "0") proxyPortInput.text = "7891"
                                        }
                                        contentItem: Text {
                                            text: parent.displayText; color: theme.text; font.pixelSize: theme.fontSm
                                            leftPadding: 6; verticalAlignment: Text.AlignVCenter
                                        }
                                        background: Rectangle {
                                            color: theme.surfaceAlt; border.color: parent.activeFocus ? theme.borderFocus : theme.border
                                            border.width: 1; radius: theme.radiusSm
                                        }
                                    }
                                }
                                RowLayout {
                                    visible: proxyEnabledCheck.checked
                                    spacing: theme.sp8
                                    Label { text: "Host:"; font.pixelSize: theme.fontSm; color: theme.textDim }
                                    TextField {
                                        id: proxyHostInput
                                        Layout.preferredWidth: 120
                                        placeholderText: "127.0.0.1"
                                        color: theme.text
                                        background: Rectangle {
                                            color: theme.surfaceAlt; border.color: parent.activeFocus ? theme.borderFocus : theme.border
                                            border.width: 1; radius: theme.radiusSm
                                        }
                                        font.pixelSize: theme.fontSm
                                        leftPadding: theme.sp6; topPadding: theme.sp4; bottomPadding: theme.sp4
                                    }
                                    Label { text: "Port:"; font.pixelSize: theme.fontSm; color: theme.textDim }
                                    TextField {
                                        id: proxyPortInput
                                        Layout.preferredWidth: 80
                                        validator: IntValidator { bottom: 0; top: 65535 }
                                        color: theme.text
                                        background: Rectangle {
                                            color: theme.surfaceAlt; border.color: parent.activeFocus ? theme.borderFocus : theme.border
                                            border.width: 1; radius: theme.radiusSm
                                        }
                                        font.pixelSize: theme.fontSm
                                        leftPadding: theme.sp6; topPadding: theme.sp4; bottomPadding: theme.sp4
                                    }
                                }
                            }
                        }

                        // Action buttons
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: theme.sp8

                            SyncButton { text: "保存配置"; onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (!root.backendRef.save_sync_config()) { root.actionInProgress = false } } }
                            SyncButton { text: "测试连接"; onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (root.backendRef.save_sync_config()) { root.backendRef.perform_sync_diagnostics() } else { root.actionInProgress = false } } }
                            SyncButton { text: "同步计划"; onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (root.backendRef.save_sync_config()) { root.backendRef.perform_sync_dry_run() } else { root.actionInProgress = false } } }
                            SyncButton { text: "立即同步"; onClicked: { applySyncFormToBackend(); root.actionInProgress = true; if (root.backendRef.save_sync_config()) { root.backendRef.perform_sync() } else { root.actionInProgress = false } } }
                        }

                        SettingsCard {
                            Label {
                                text: "提示：首次同步时如果远程分支不存在，系统将自动创建分支并推送初始内容。\nGitHub API 后端使用 REST API，支持双向同步、冲突检测、30 天回收站。"
                                font.pixelSize: theme.fontXs; color: theme.textDim; wrapMode: Text.Wrap
                                Layout.fillWidth: true
                            }
                        }

                        ScrollView {
                            Layout.fillWidth: true
                            Layout.preferredHeight: Math.min(syncResultLabel.implicitHeight + 20, 200)
                            clip: true
                            TextArea {
                                id: syncResultLabel
                                width: parent.width
                                readOnly: true
                                wrapMode: Text.Wrap
                                color: theme.text
                                font.pixelSize: theme.fontSm
                                background: Rectangle { color: theme.surfaceAlt; radius: theme.radiusSm; border.color: theme.border; border.width: 1 }
                                leftPadding: theme.sp8; topPadding: theme.sp8; rightPadding: theme.sp8; bottomPadding: theme.sp8
                            }
                        }

                        Item { height: theme.sp16 }
                    }
                }

                // ── Tab 3: Animations ──────────────────────────────
                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: parent.width; spacing: theme.sp16

                        SettingsSection { title: "输入动效" }
                        SettingsCard {
                            FormSwitch {
                                label: "输入动画"; description: "启用逐字输入动画效果"
                                id: typingAnimCheck
                            }
                        }
                        SettingsCard {
                            FormSwitch {
                                label: "平滑光标"; description: "启用光标平滑移动效果"
                                id: smoothCursorCheck
                            }
                        }
                        Item { height: theme.sp16 }
                    }
                }

                // ── Tab 4: Debug / Action Registry ─────────────────
                ActionRegistryPage {
                    backendRef: root.backendRef
                    theme: root.theme
                    fontSizeSpinValue: fontSizeSlider ? fontSizeSlider.value : 16
                    autoSaveCheckChecked: autoSaveCheck ? autoSaveCheck.checked : false
                    autoSaveDelaySpinValue: autoSaveDelaySlider ? autoSaveDelaySlider.value : 1500
                    lineSpacingSpinValue: lineSpacingSlider ? lineSpacingSlider.value : 150
                    autoIndentCheckChecked: autoIndentCheck ? autoIndentCheck.checked : false
                    autoIndentWidthSpinValue: autoIndentWidthSlider ? autoIndentWidthSlider.value : 200
                    typingAnimCheckChecked: typingAnimCheck ? typingAnimCheck.checked : false
                    smoothCursorCheckChecked: smoothCursorCheck ? smoothCursorCheck.checked : false
                }

                // ── Tab 5: About ───────────────────────────────────
                Item {
                    ColumnLayout {
                        anchors.centerIn: parent
                        spacing: theme.sp12

                        Label {
                            text: "Writer"
                            font.pixelSize: theme.fontXxl
                            font.weight: Font.Bold
                            color: theme.primary
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Label {
                            text: "版本 1.0.0"
                            font.pixelSize: theme.fontMd
                            color: theme.textDim
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Rectangle { width: 40; height: 1; color: theme.divider; Layout.alignment: Qt.AlignHCenter }

                        Label {
                            text: "技术栈: Rust + Qt/QML (qmetaobject)"
                            font.pixelSize: theme.fontSm
                            color: theme.textDim
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Label {
                            text: "GitHub API 同步 | 双向同步 | 30 天回收站"
                            font.pixelSize: theme.fontSm
                            color: theme.textDim
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Label {
                            text: "跨平台写作工具，专注长文创作体验"
                            font.pixelSize: theme.fontSm
                            color: theme.textDim
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                    }
                }
            }
        }
    }

    // ── Inline Components ────────────────────────────────────────

    component SettingsCategoryTab: Item {
        id: sctItem
        property string text: ""
        property string icon: ""
        property bool active: false
        signal clicked()
        height: 36
        Layout.fillWidth: true

        Rectangle {
            anchors.fill: parent
            anchors.leftMargin: theme.sp8
            anchors.rightMargin: theme.sp8
            radius: theme.radiusSm
            color: sctItem.active ? theme.selected : (maCat.containsMouse ? theme.hover : "transparent")
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: theme.sp12
            spacing: theme.sp8
            Text { text: sctItem.icon; font.pixelSize: theme.fontMd }
            Text {
                text: sctItem.text; color: sctItem.active ? theme.selectedText : theme.text
                font.pixelSize: theme.fontMd; font.weight: sctItem.active ? Font.Medium : Font.Normal
                Layout.fillWidth: true; elide: Text.ElideRight
            }
        }
        MouseArea {
            anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
            onClicked: sctItem.clicked()
        }
    }

    component SettingsSection: Item {
        property string title: ""
        height: 24
        Layout.fillWidth: true
        Label {
            text: title; font.pixelSize: theme.fontXl; font.weight: Font.Bold; color: theme.text
        }
    }

    component SettingsCard: Rectangle {
        Layout.fillWidth: true
        radius: theme.radiusMd
        color: theme.surfaceAlt
        border.color: theme.border; border.width: 1
        implicitHeight: col.implicitHeight + theme.sp16
        ColumnLayout {
            id: col
            anchors.fill: parent
            anchors.margins: theme.sp12
            spacing: theme.sp8
        }
        default property alias content: col.children
    }

    component FormField: Item {
        property string label: ""
        property string placeholder: ""
        id: ffItem
        height: inputField.height + 20
        Layout.fillWidth: true

        Label {
            text: ffItem.label
            font.pixelSize: theme.fontMd; color: theme.text; font.weight: Font.Medium
            anchors.top: parent.top
        }

        TextField {
            id: inputField
            anchors.top: parent.top; anchors.topMargin: 20
            anchors.left: parent.left; anchors.right: parent.right
            height: 32
            placeholderText: ffItem.placeholder
            color: theme.text
            background: Rectangle {
                color: theme.surface; border.color: parent.activeFocus ? theme.borderFocus : theme.border
                border.width: 1; radius: theme.radiusSm
            }
            font.pixelSize: theme.fontMd
            leftPadding: theme.sp8; topPadding: theme.sp6; bottomPadding: theme.sp6
        }
    }

    component FormSwitch: Item {
        property string label: ""
        property string description: ""
        property bool checked: false
        id: fsItem
        height: 40
        Layout.fillWidth: true

        RowLayout {
            anchors.fill: parent
            spacing: theme.sp12

            Switch {
                checked: fsItem.checked
                onCheckedChanged: fsItem.checked = checked
                indicator: Rectangle {
                    implicitWidth: 40; implicitHeight: 22
                    x: parent.leftPadding; y: parent.height / 2 - height / 2
                    radius: 11
                    color: parent.checked ? theme.primary : theme.border
                    Behavior on color { ColorAnimation { duration: 150 } }
                    Rectangle {
                        x: parent.checked ? 20 : 2
                        y: 2; width: 18; height: 18; radius: 9
                        color: "#ffffff"
                        Behavior on x { NumberAnimation { duration: 150 } }
                    }
                }
                background: Item {}
            }

            ColumnLayout {
                spacing: 2
                Label {
                    text: fsItem.label; font.pixelSize: theme.fontMd; color: theme.text
                }
                Label {
                    text: fsItem.description; font.pixelSize: theme.fontXs; color: theme.textDim
                    visible: fsItem.description.length > 0; wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }
        }
    }

    component FormSlider: Item {
        property string label: ""
        property string valueLabel: ""
        property real from: 0
        property real to: 100
        property real value: 50
        property real stepSize: 1
        id: fslItem
        height: 40
        Layout.fillWidth: true

        RowLayout {
            anchors.fill: parent
            spacing: theme.sp12

            Label {
                text: fslItem.label; font.pixelSize: theme.fontMd; color: theme.text
                Layout.preferredWidth: 60
            }
            Slider {
                id: fslSlider
                from: fslItem.from; to: fslItem.to; value: fslItem.value; stepSize: fslItem.stepSize
                Layout.fillWidth: true
                Layout.preferredHeight: 24
                onValueChanged: fslItem.value = value
            }
            Label {
                text: fslItem.valueLabel; font.pixelSize: theme.fontSm; color: theme.textDim
                Layout.preferredWidth: 56; horizontalAlignment: Text.AlignRight
            }
        }
    }

    component SyncButton: Button {
        id: syncBtn
        implicitHeight: 32
        implicitWidth: Math.max(textMetricsBtn.width + 20, 64)
        TextMetrics { id: textMetricsBtn; text: syncBtn.text; font.pixelSize: theme.fontSm }

        enabled: !root.actionInProgress
        contentItem: Text {
            text: syncBtn.text; color: theme.primaryText; font.pixelSize: theme.fontSm
            horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            color: syncBtn.enabled ? (syncBtn.hovered ? theme.primaryHover : theme.primary) : theme.border
            radius: theme.radiusSm
        }
    }
}
