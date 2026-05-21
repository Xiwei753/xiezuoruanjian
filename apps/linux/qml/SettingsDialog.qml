import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Popup {
    id: root
    property var backendRef: null
    property var editorPageRef: null
    property var appTheme: null
    property bool actionInProgress: false
    property int currentCategory: 0

    width: 720; height: 580
    modal: true; focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    x: Math.round((parent.width - width) / 2)
    y: Math.round((parent.height - height) / 2)
    background: Rectangle { color: root.appTheme ? root.appTheme.surface : "#ffffff"; radius: root.appTheme ? root.appTheme.radiusLg : 12; border.color: root.appTheme ? root.appTheme.border : "#e2e8f0"; border.width: 1 }

    function switchToCategory(index) { currentCategory = index }

    onAboutToShow: {
        if (backendRef) {
            backendRef.load_local_settings()
            backendRef.load_sync_config()
        }
    }

    RowLayout {
        anchors.fill: parent; spacing: 0

        // ── Left Category Sidebar ──────────────────────────────────

        Rectangle {
            Layout.preferredWidth: 160; Layout.fillHeight: true
            color: root.appTheme ? root.appTheme.surfaceAlt : "#f1f5f9"

            ColumnLayout {
                anchors.fill: parent; anchors.topMargin: root.appTheme ? root.appTheme.sp16 : 16; spacing: root.appTheme ? root.appTheme.sp2 : 2
                Label {
                    text: "设置"; font.pixelSize: root.appTheme ? root.appTheme.fontXl : 18; font.weight: Font.Bold; color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
                    Layout.leftMargin: root.appTheme ? root.appTheme.sp16 : 16; Layout.bottomMargin: root.appTheme ? root.appTheme.sp12 : 12
                }

                SidebarItem { text: "编辑器"; icon: "✏"; active: currentCategory === 0; onClicked: currentCategory = 0; theme: root.appTheme }
                SidebarItem { text: "外观";   icon: "🎨"; active: currentCategory === 1; onClicked: currentCategory = 1; theme: root.appTheme }
                SidebarItem { text: "同步";   icon: "🔄"; active: currentCategory === 2; onClicked: currentCategory = 2; theme: root.appTheme }
                SidebarItem { text: "动效";   icon: "✨"; active: currentCategory === 3; onClicked: currentCategory = 3; theme: root.appTheme }
                SidebarItem { text: "调试";   icon: "🐛"; active: currentCategory === 4; onClicked: currentCategory = 4; theme: root.appTheme }
                SidebarItem { text: "关于";   icon: "ℹ"; active: currentCategory === 5; onClicked: currentCategory = 5; theme: root.appTheme }

                Item { Layout.fillHeight: true }
            }
        }

        Rectangle { width: 1; color: root.appTheme ? root.appTheme.divider : "#e2e8f0"; Layout.fillHeight: true }

        // ── Right Content Panel ────────────────────────────────────

        Rectangle {
            Layout.fillWidth: true; Layout.fillHeight: true; color: root.appTheme ? root.appTheme.surface : "#ffffff"

            StackLayout {
                anchors.fill: parent; anchors.margins: root.appTheme ? root.appTheme.sp16 : 16
                currentIndex: currentCategory

                // Tab 0: Editor
                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: 560; spacing: root.appTheme ? root.appTheme.sp16 : 16
                        Layout.alignment: Qt.AlignTop
                        SectionHeader { theme: root.appTheme; text: "编辑" }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            SettingsRow {
                                theme: root.appTheme; isSwitch: false; label: "字号"
                                sliderValue: backendRef && backendRef.setting_font_size > 0 ? backendRef.setting_font_size : 16
                                sliderFrom: 10; sliderTo: 72
                                valueLabel: sliderValue.toFixed(0) + "px"
                                id: fontSizeRow
                            }
                        }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            SettingsRow {
                                theme: root.appTheme; isSwitch: false; label: "行距"
                                sliderValue: backendRef && backendRef.setting_line_spacing > 0 ? backendRef.setting_line_spacing * 100 : 150
                                sliderFrom: 100; sliderTo: 300; sliderStep: 10
                                valueLabel: (sliderValue / 100).toFixed(2) + "x"
                                id: lineSpacingRow
                            }
                        }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: root.appTheme ? root.appTheme.sp8 : 8
                                SettingsRow {
                                    theme: root.appTheme; isSwitch: true; label: "自动缩进"; description: "自动在换行时添加缩进"
                                    id: autoIndentCheck
                                    checked: backendRef ? backendRef.setting_auto_indent_enabled : false
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: root.appTheme ? root.appTheme.sp16 : 16
                                    Layout.rightMargin: root.appTheme ? root.appTheme.sp12 : 12
                                    spacing: root.appTheme ? root.appTheme.sp4 : 4
                                    visible: autoIndentCheck.checked
                                    Label {
                                        text: "缩进宽度:"
                                        font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                                        color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                                    }
                                    Slider {
                                        id: autoIndentWidthSlider
                                        from: 0; to: 800; value: backendRef && backendRef.setting_auto_indent_width > 0 ? backendRef.setting_auto_indent_width * 100 : 200; stepSize: 50
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 28
                                    }
                                    Label {
                                        text: (autoIndentWidthSlider.value / 100).toFixed(1) + " 字符"
                                        font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                                        color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                                    }
                                }
                            }
                        }

                        SectionHeader { theme: root.appTheme; text: "保存" }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: root.appTheme ? root.appTheme.sp8 : 8
                                SettingsRow {
                                    theme: root.appTheme; isSwitch: true; label: "自动保存"; description: "在编辑内容变化时自动保存到磁盘"
                                    id: autoSaveCheck
                                    checked: backendRef ? backendRef.setting_auto_save_enabled : false
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: root.appTheme ? root.appTheme.sp16 : 16
                                    Layout.rightMargin: root.appTheme ? root.appTheme.sp12 : 12
                                    spacing: root.appTheme ? root.appTheme.sp4 : 4
                                    visible: autoSaveCheck.checked
                                    Label {
                                        text: "延迟:"
                                        font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                                        color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                                    }
                                    Slider {
                                        id: autoSaveDelaySlider
                                        from: 500; to: 60000; value: backendRef && backendRef.setting_auto_save_delay_ms > 0 ? backendRef.setting_auto_save_delay_ms : 1500; stepSize: 500
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 28
                                    }
                                    Label {
                                        text: autoSaveDelaySlider.value.toFixed(0) + " ms"
                                        font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                                        color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                                    }
                                }
                            }
                        }

                        SectionHeader { theme: root.appTheme; text: "主题" }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            RowLayout {
                                Layout.fillWidth: true; spacing: root.appTheme ? root.appTheme.sp12 : 12
                                Label {
                                    text: "主题模式:"
                                    font.pixelSize: root.appTheme ? root.appTheme.fontMd : 13
                                    color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
                                }
                                AppComboBox {
                                    id: themeCombo
                                    model: ["system", "light", "dark"]
                                    theme: root.appTheme
                                    Layout.preferredWidth: 140
                                    currentIndex: {
                                        if (!backendRef) return 0
                                        var modes = ["system", "light", "dark"]
                                        var idx = modes.indexOf(backendRef.setting_theme_mode)
                                        return idx >= 0 ? idx : 0
                                    }
                                }
                            }
                        }

                        Item { height: root.appTheme ? root.appTheme.sp8 : 8 }
                        AppButton {
                            text: "保存设置"; theme: root.appTheme
                            Layout.alignment: Qt.AlignLeft
                            onClicked: {
                                if (!backendRef) return
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
                        Item { height: root.appTheme ? root.appTheme.sp16 : 16 }
                    }
                }

                // Tab 1: Appearance
                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: 560; spacing: root.appTheme ? root.appTheme.sp16 : 16
                        Layout.alignment: Qt.AlignTop
                        SectionHeader { theme: root.appTheme; text: "界面" }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            SettingsRow {
                                theme: root.appTheme; isSwitch: true
                                label: "自动保存时显示保存状态"
                                description: "在底部状态栏显示当前保存状态指示"
                                checked: true
                                enabled: false
                            }
                        }
                        SectionHeader { theme: root.appTheme; text: "关于外观" }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: root.appTheme ? root.appTheme.sp4 : 4
                                Label {
                                    text: "当前主题: " + (root.appTheme && root.appTheme.mode === "light" ? "浅色" : "深色")
                                    font.pixelSize: root.appTheme ? root.appTheme.fontMd : 13
                                    color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
                                }
                                Label {
                                    text: "设计系统: 统一间距/圆角/色彩体系"
                                    font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                                    color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                                    wrapMode: Text.WordWrap
                                    Layout.fillWidth: true
                                }
                            }
                        }
                        Item { height: root.appTheme ? root.appTheme.sp16 : 16 }
                    }
                }

                // Tab 2: Sync
                SyncPage {
                    backendRef: root.backendRef; appTheme: root.appTheme
                    actionInProgress: root.actionInProgress
                    onActionInProgressChanged: root.actionInProgress = actionInProgress
                }

                // Tab 3: Animations
                ScrollView {
                    clip: true
                    ColumnLayout {
                        width: 560; spacing: root.appTheme ? root.appTheme.sp16 : 16
                        Layout.alignment: Qt.AlignTop
                        SectionHeader { theme: root.appTheme; text: "输入动效" }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            SettingsRow {
                                theme: root.appTheme; isSwitch: true
                                label: "输入动画"
                                description: "启用逐字输入动画效果"
                                id: typingAnimCheck
                                checked: backendRef ? backendRef.setting_typing_animation_enabled : false
                            }
                        }
                        AppCard { theme: root.appTheme; Layout.fillWidth: true
                            SettingsRow {
                                theme: root.appTheme; isSwitch: true
                                label: "平滑光标"
                                description: "启用光标平滑移动效果"
                                id: smoothCursorCheck
                                checked: backendRef ? backendRef.setting_smooth_cursor_enabled : false
                            }
                        }
                        Item { height: root.appTheme ? root.appTheme.sp16 : 16 }
                    }
                }

                // Tab 4: Debug
                ActionRegistryPage {
                    backendRef: root.backendRef; appTheme: root.appTheme
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
                        anchors.centerIn: parent; spacing: root.appTheme ? root.appTheme.sp12 : 12
                        Label {
                            text: "Writer"
                            font.pixelSize: root.appTheme ? root.appTheme.fontXxl : 22
                            font.weight: Font.Bold
                            color: root.appTheme ? root.appTheme.primary : "#3b82f6"
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Label {
                            text: "版本 1.0.0"
                            font.pixelSize: root.appTheme ? root.appTheme.fontMd : 13
                            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Rectangle { width: 40; height: 1; color: root.appTheme ? root.appTheme.divider : "#e2e8f0"; Layout.alignment: Qt.AlignHCenter }
                        Label {
                            text: "技术栈: Rust + Qt/QML (qmetaobject)"
                            font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Label {
                            text: "GitHub API 同步 | 双向同步 | 30 天回收站"
                            font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }
                        Label {
                            text: "跨平台写作工具，专注长文创作体验"
                            font.pixelSize: root.appTheme ? root.appTheme.fontSm : 12
                            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                            horizontalAlignment: Text.AlignHCenter
                            Layout.fillWidth: true
                        }

                        Item { height: root.appTheme ? root.appTheme.sp16 : 16 }

                        // Workspace switcher (only visible when workspace is open)
                        AppButton {
                            text: "切换工作区"; theme: root.appTheme
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
