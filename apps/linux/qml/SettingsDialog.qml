import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Popup {
    id: root
    property var backendRef: null
    property var editorPageRef: null
    property var theme: null
    property bool actionInProgress: false
    property int currentCategory: 0

    width: 720; height: 580
    modal: true; focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    x: Math.round((parent.width - width) / 2)
    y: Math.round((parent.height - height) / 2)
    background: Rectangle { color: theme.surface; radius: theme.radiusLg; border.color: theme.border; border.width: 1 }

    function switchToCategory(index) { currentCategory = index }

    onAboutToShow: {
        backendRef.load_local_settings()
        backendRef.load_sync_config()
    }

    RowLayout {
        anchors.fill: parent; spacing: 0

        // ── Left Category Sidebar ──────────────────────────────────

        Rectangle {
            Layout.preferredWidth: 160; Layout.fillHeight: true
            color: theme.surfaceAlt

            ColumnLayout {
                anchors.fill: parent; anchors.topMargin: theme.sp16; spacing: theme.sp2
                Label {
                    text: "设置"; font.pixelSize: theme.fontXl; font.weight: Font.Bold; color: theme.text
                    Layout.leftMargin: theme.sp16; Layout.bottomMargin: theme.sp12
                }

                SidebarItem { text: "编辑器"; icon: "✏"; active: currentCategory === 0; onClicked: currentCategory = 0; theme: theme }
                SidebarItem { text: "外观";   icon: "🎨"; active: currentCategory === 1; onClicked: currentCategory = 1; theme: theme }
                SidebarItem { text: "同步";   icon: "🔄"; active: currentCategory === 2; onClicked: currentCategory = 2; theme: theme }
                SidebarItem { text: "动效";   icon: "✨"; active: currentCategory === 3; onClicked: currentCategory = 3; theme: theme }
                SidebarItem { text: "调试";   icon: "🐛"; active: currentCategory === 4; onClicked: currentCategory = 4; theme: theme }
                SidebarItem { text: "关于";   icon: "ℹ"; active: currentCategory === 5; onClicked: currentCategory = 5; theme: theme }

                Item { Layout.fillHeight: true }
            }
        }

        Rectangle { width: 1; color: theme.divider; Layout.fillHeight: true }

        // ── Right Content Panel ────────────────────────────────────

        Rectangle {
            Layout.fillWidth: true; Layout.fillHeight: true; color: theme.surface

            StackLayout {
                anchors.fill: parent; anchors.margins: theme.sp16
                currentIndex: currentCategory

                // Tab 0: Editor
                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: Math.min(parent.width, 560); spacing: theme.sp16
                        Layout.alignment: Qt.AlignTop
                        SectionHeader { theme: theme; text: "编辑" }
                        AppCard { theme: theme; Layout.fillWidth: true
                            SettingsRow {
                                theme: theme; isSwitch: false; label: "字号"
                                sliderValue: backendRef.setting_font_size > 0 ? backendRef.setting_font_size : 16
                                sliderFrom: 10; sliderTo: 72
                                valueLabel: sliderValue.toFixed(0) + "px"
                                id: fontSizeRow
                            }
                        }
                        AppCard { theme: theme; Layout.fillWidth: true
                            SettingsRow {
                                theme: theme; isSwitch: false; label: "行距"
                                sliderValue: backendRef.setting_line_spacing > 0 ? backendRef.setting_line_spacing * 100 : 150
                                sliderFrom: 100; sliderTo: 300; sliderStep: 10
                                valueLabel: (sliderValue / 100).toFixed(2) + "x"
                                id: lineSpacingRow
                            }
                        }
                        AppCard { theme: theme; Layout.fillWidth: true
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                SettingsRow {
                                    theme: theme; isSwitch: true; label: "自动缩进"; description: "自动在换行时添加缩进"
                                    id: autoIndentCheck
                                    checked: backendRef.setting_auto_indent_enabled
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: 44
                                    Layout.rightMargin: theme.sp12
                                    spacing: theme.sp4
                                    visible: autoIndentCheck.checked
                                    Label { text: "缩进宽度:"; font.pixelSize: theme.fontSm; color: theme.textDim }
                                    Slider {
                                        id: autoIndentWidthSlider
                                        from: 0; to: 800; value: backendRef.setting_auto_indent_width > 0 ? backendRef.setting_auto_indent_width * 100 : 200; stepSize: 50
                                        Layout.fillWidth: true; Layout.preferredHeight: 28
                                        Layout.bottomMargin: 4
                                    }
                                    Label { text: (autoIndentWidthSlider.value / 100).toFixed(1) + " 字符"; font.pixelSize: theme.fontSm; color: theme.textDim; Layout.bottomMargin: 4 }
                                }
                            }
                        }

                        SectionHeader { theme: theme; text: "保存" }
                        AppCard { theme: theme; Layout.fillWidth: true
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                SettingsRow {
                                    theme: theme; isSwitch: true; label: "自动保存"; description: "在编辑内容变化时自动保存到磁盘"
                                    id: autoSaveCheck
                                    checked: backendRef.setting_auto_save_enabled
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: 44
                                    Layout.rightMargin: theme.sp12
                                    spacing: theme.sp4
                                    visible: autoSaveCheck.checked
                                    Label { text: "延迟:"; font.pixelSize: theme.fontSm; color: theme.textDim }
                                    Slider {
                                        id: autoSaveDelaySlider
                                        from: 500; to: 60000; value: backendRef.setting_auto_save_delay_ms > 0 ? backendRef.setting_auto_save_delay_ms : 1500; stepSize: 500
                                        Layout.fillWidth: true; Layout.preferredHeight: 28
                                        Layout.bottomMargin: 4
                                    }
                                    Label { text: autoSaveDelaySlider.value.toFixed(0) + " ms"; font.pixelSize: theme.fontSm; color: theme.textDim; Layout.bottomMargin: 4 }
                                }
                            }
                        }

                        SectionHeader { theme: theme; text: "主题" }
                        AppCard { theme: theme; Layout.fillWidth: true
                            RowLayout {
                                Layout.fillWidth: true; spacing: theme.sp12
                                Label { text: "主题模式:"; font.pixelSize: theme.fontMd; color: theme.text }
                                ComboBox {
                                    id: themeCombo
                                    model: ["system", "light", "dark"]
                                    Layout.preferredWidth: 120
                                    currentIndex: {
                                        var modes = ["system", "light", "dark"]
                                        var idx = modes.indexOf(backendRef.setting_theme_mode)
                                        return idx >= 0 ? idx : 0
                                    }
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
                        AppButton {
                            text: "保存设置"; theme: theme
                            Layout.alignment: Qt.AlignLeft
                            onClicked: {
                                backendRef.setting_font_size = fontSizeRow.sliderValue
                                backendRef.setting_line_spacing = lineSpacingRow.sliderValue / 100.0
                                backendRef.setting_auto_save_enabled = autoSaveCheck.checked
                                backendRef.setting_auto_save_delay_ms = autoSaveDelaySlider.value
                                backendRef.setting_auto_indent_enabled = autoIndentCheck.checked
                                backendRef.setting_auto_indent_width = autoIndentWidthSlider.value / 100.0
                                backendRef.setting_theme_mode = themeCombo.currentText
                                backendRef.save_local_settings()
                            }
                        }
                        Item { height: theme.sp16 }
                    }
                }

                // Tab 1: Appearance
                ScrollView {
                    clip: true
                    ColumnLayout { width: Math.min(parent.width, 560); spacing: theme.sp16
                        Layout.alignment: Qt.AlignTop
                        SectionHeader { theme: theme; text: "界面" }
                        AppCard { theme: theme; Layout.fillWidth: true
                            SettingsRow { theme: theme; isSwitch: true; label: "自动保存时显示保存状态"; description: "在底部状态栏显示当前保存状态指示"; checked: true; enabled: false }
                        }
                        SectionHeader { theme: theme; text: "关于外观" }
                        AppCard { theme: theme; Layout.fillWidth: true
                            Label { text: "当前主题: " + (theme.mode === "light" ? "浅色" : "深色"); font.pixelSize: theme.fontMd; color: theme.text }
                            Label { text: "设计系统: 统一间距/圆角/色彩体系"; font.pixelSize: theme.fontSm; color: theme.textDim; wrapMode: Text.Wrap; Layout.fillWidth: true }
                        }
                        Item { height: theme.sp16 }
                    }
                }

                // Tab 2: Sync
                SyncPage {
                    backendRef: root.backendRef; theme: root.theme
                    actionInProgress: root.actionInProgress
                    onActionInProgressChanged: root.actionInProgress = actionInProgress
                }

                // Tab 3: Animations
                ScrollView {
                    clip: true
                    ColumnLayout { width: Math.min(parent.width, 560); spacing: theme.sp16
                        Layout.alignment: Qt.AlignTop
                        SectionHeader { theme: theme; text: "输入动效" }
                        AppCard { theme: theme; Layout.fillWidth: true
                            SettingsRow { theme: theme; isSwitch: true; label: "输入动画"; description: "启用逐字输入动画效果"; id: typingAnimCheck; checked: backendRef.setting_typing_animation_enabled }
                        }
                        AppCard { theme: theme; Layout.fillWidth: true
                            SettingsRow { theme: theme; isSwitch: true; label: "平滑光标"; description: "启用光标平滑移动效果"; id: smoothCursorCheck; checked: backendRef.setting_smooth_cursor_enabled }
                        }
                        Item { height: theme.sp16 }
                    }
                }

                // Tab 4: Debug
                ActionRegistryPage {
                    backendRef: root.backendRef; theme: root.theme
                    fontSizeSpinValue: fontSizeRow ? fontSizeRow.sliderValue : 16
                    autoSaveCheckChecked: autoSaveCheck ? autoSaveCheck.checked : false
                    autoSaveDelaySpinValue: autoSaveDelaySlider ? autoSaveDelaySlider.value : 1500
                    lineSpacingSpinValue: lineSpacingRow ? lineSpacingRow.sliderValue : 150
                    autoIndentCheckChecked: autoIndentCheck ? autoIndentCheck.checked : false
                    autoIndentWidthSpinValue: autoIndentWidthSlider ? autoIndentWidthSlider.value : 200
                    typingAnimCheckChecked: typingAnimCheck ? typingAnimCheck.checked : false
                    smoothCursorCheckChecked: smoothCursorCheck ? smoothCursorCheck.checked : false
                }

                // Tab 5: About
                Item {
                    ColumnLayout {
                        anchors.centerIn: parent; spacing: theme.sp12
                        Label { text: "Writer"; font.pixelSize: theme.fontXxl; font.weight: Font.Bold; color: theme.primary; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
                        Label { text: "版本 1.0.0"; font.pixelSize: theme.fontMd; color: theme.textDim; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
                        Rectangle { width: 40; height: 1; color: theme.divider; Layout.alignment: Qt.AlignHCenter }
                        Label { text: "技术栈: Rust + Qt/QML (qmetaobject)"; font.pixelSize: theme.fontSm; color: theme.textDim; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
                        Label { text: "GitHub API 同步 | 双向同步 | 30 天回收站"; font.pixelSize: theme.fontSm; color: theme.textDim; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }
                        Label { text: "跨平台写作工具，专注长文创作体验"; font.pixelSize: theme.fontSm; color: theme.textDim; horizontalAlignment: Text.AlignHCenter; Layout.fillWidth: true }

                        Item { height: theme.sp16 }

                        // Workspace switcher (only visible when workspace is open)
                        AppButton {
                            text: "切换工作区"; theme: theme
                            Layout.alignment: Qt.AlignHCenter
                            visible: backendRef && backendRef.has_workspace
                            onClicked: {
                                backendRef.switch_workspace()
                                root.close()
                            }
                        }
                    }
                }
            }
        }
    }
}
