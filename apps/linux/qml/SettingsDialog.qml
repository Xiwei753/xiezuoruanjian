// =============================================================================
// SettingsDialog.qml — 设置对话框
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
// 职责：编辑器设置展示与保存（字号、行距、自动保存、主题、AI 开关）
// 约束：
//   - 通过 backendRef 直接读写 AppBackend 属性
//   - 不直接操作文件系统，通过 backendRef.save_local_settings() 持久化
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Dialog {
    id: root
    modal: true
    width: 640
    height: 620
    x: (parent ? (parent.width - width) / 2 : 0)
    y: (parent ? (parent.height - height) / 2 : 0)
    property var theme: null
    property var backendRef: null
    property var dt: theme
    property bool updatingValues: false
    signal settingsChanged()

    background: Rectangle { color: dt ? dt.surface : "#1A1D23"; border.color: dt ? dt.border : "#2A2E36"; border.width: 1; radius: dt ? dt.radiusMd : 12 }

    function saveAndNotify() { if (!backendRef) return; backendRef.save_local_settings(); root.settingsChanged() }
    function setSwitchValue(control, key, value) {
        control.checked = value
        if (!backendRef || updatingValues) return
        backendRef[key] = value
        saveAndNotify()
    }
    function updateValues() {
        if (!backendRef) return
        updatingValues = true
        autoSave.checked = backendRef.setting_auto_save_enabled
        typingAnim.checked = backendRef.setting_typing_animation_enabled
        smoothCursor.checked = backendRef.setting_smooth_cursor_enabled
        aiSwitch.checked = backendRef.ai_enabled
        autoSaveDelay.value = backendRef.setting_auto_save_delay_ms / 1000.0
        fontSizeSlider.value = backendRef.setting_font_size || 16.0
        lineSpacingSlider.value = backendRef.setting_line_spacing || 1.5
        autoIndent.checked = backendRef.setting_auto_indent_enabled
        autoIndentWidth.value = backendRef.setting_auto_indent_width || 2.0
        typingAnimDuration.value = backendRef.setting_typing_animation_duration_ms || 100
        smoothCursorDuration.value = backendRef.setting_smooth_cursor_duration_ms || 80
        var mode = backendRef.setting_theme_mode
        themeCombo.currentIndex = mode === "light" ? 1 : (mode === "dark" ? 2 : 0)
        updatingValues = false
    }
    onOpened: updateValues()

    Rectangle {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 54
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt ? dt.sp24 : 24
            anchors.rightMargin: dt ? dt.sp16 : 16
            Text { text: "设置"; color: dt ? dt.textPrimary : "#E2E4E9"; font.pixelSize: dt ? dt.fontLg : 16; font.weight: Font.Bold; Layout.fillWidth: true }
            ToolButton { text: "x"; onClicked: root.close() }
        }
    }

    ScrollView {
        id: settingsScroll
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.top: topBar.bottom
        anchors.leftMargin: dt ? dt.sp20 : 20
        anchors.rightMargin: dt ? dt.sp20 : 20
        anchors.bottomMargin: dt ? dt.sp20 : 20
        anchors.topMargin: dt ? dt.sp8 : 8
        clip: true

        ColumnLayout {
            width: settingsScroll.availableWidth
            spacing: dt ? dt.cardGap : 16

            SettingsSection {
                dt: root.dt
                title: "外观"
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: "字体大小"
                    description: Math.round(fontSizeSlider.value) + " px"
                    Slider {
                        id: fontSizeSlider
                        from: 12.0
                        to: 32.0
                        stepSize: 1.0
                        onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_font_size = value; root.saveAndNotify() }
                    }
                }
                SettingsRow {
                    dt: root.dt
                    title: "行距倍数"
                    description: Number(lineSpacingSlider.value).toFixed(1) + "x"
                    Slider {
                        id: lineSpacingSlider
                        from: 1.0
                        to: 3.0
                        stepSize: 0.1
                        onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_line_spacing = value; root.saveAndNotify() }
                    }
                }
                SettingsRow {
                    dt: root.dt
                    title: "主题模式"
                    description: "切换系统、浅色或深色"
                    ModernComboBox {
                        id: themeCombo
                        dt: root.dt
                        model: ["跟随系统", "浅色", "深色"]
                        onActivated: function(index) {
                            if (!backendRef || root.updatingValues) return
                            backendRef.setting_theme_mode = ["system", "light", "dark"][index]
                            root.saveAndNotify()
                        }
                    }
                }
            }

            SettingsSection {
                dt: root.dt
                title: "编辑器行为"
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: "打字动画"
                    description: "输入时显示动态效果"
                    clickable: true
                    onClicked: root.setSwitchValue(typingAnim, "setting_typing_animation_enabled", !typingAnim.checked)
                    ModernSwitch { id: typingAnim; dt: root.dt; onToggled: function(v) { root.setSwitchValue(typingAnim, "setting_typing_animation_enabled", v) } }
                }
                SettingsRow {
                    dt: root.dt
                    title: "打字动画持续时间"
                    description: Math.round(typingAnimDuration.value) + " ms"
                    Slider {
                        id: typingAnimDuration
                        from: 0
                        to: 240
                        stepSize: 10
                        onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_typing_animation_duration_ms = value; root.saveAndNotify() }
                    }
                }
                SettingsRow {
                    dt: root.dt
                    title: "平滑光标"
                    description: "光标移动更顺滑"
                    clickable: true
                    onClicked: root.setSwitchValue(smoothCursor, "setting_smooth_cursor_enabled", !smoothCursor.checked)
                    ModernSwitch { id: smoothCursor; dt: root.dt; onToggled: function(v) { root.setSwitchValue(smoothCursor, "setting_smooth_cursor_enabled", v) } }
                }
                SettingsRow {
                    dt: root.dt
                    title: "平滑光标持续时间"
                    description: Math.round(smoothCursorDuration.value) + " ms"
                    Slider {
                        id: smoothCursorDuration
                        from: 0
                        to: 240
                        stepSize: 10
                        onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_smooth_cursor_duration_ms = value; root.saveAndNotify() }
                    }
                }
                SettingsRow {
                    dt: root.dt
                    title: "自动首行缩进"
                    description: "回车时自动添加缩进"
                    clickable: true
                    onClicked: root.setSwitchValue(autoIndent, "setting_auto_indent_enabled", !autoIndent.checked)
                    ModernSwitch { id: autoIndent; dt: root.dt; onToggled: function(v) { root.setSwitchValue(autoIndent, "setting_auto_indent_enabled", v) } }
                }
                SettingsRow {
                    dt: root.dt
                    title: "首行缩进宽度"
                    description: Number(autoIndentWidth.value).toFixed(1) + " 字符"
                    Slider {
                        id: autoIndentWidth
                        from: 0.0
                        to: 8.0
                        stepSize: 0.5
                        onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_indent_width = value; root.saveAndNotify() }
                    }
                }
            }

            SettingsSection {
                dt: root.dt
                title: "保存"
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: "自动保存"
                    description: "编辑时自动保存到本地"
                    clickable: true
                    onClicked: root.setSwitchValue(autoSave, "setting_auto_save_enabled", !autoSave.checked)
                    ModernSwitch { id: autoSave; dt: root.dt; onToggled: function(v) { root.setSwitchValue(autoSave, "setting_auto_save_enabled", v) } }
                }
                SettingsRow {
                    dt: root.dt
                    title: "自动保存延迟"
                    description: Math.round(autoSaveDelay.value) + " 秒"
                    Slider {
                        id: autoSaveDelay
                        from: 1
                        to: 10
                        stepSize: 1
                        onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_save_delay_ms = value * 1000; root.saveAndNotify() }
                    }
                }
            }

            SettingsSection {
                dt: root.dt
                title: "同步"
                Layout.fillWidth: true
                SettingsRow { dt: root.dt; title: "同步配置"; description: "在同步页面管理仓库与鉴权" }
            }

            SettingsSection {
                dt: root.dt
                title: "统计"
                Layout.fillWidth: true
                SettingsRow { dt: root.dt; title: "统计开关"; description: "统计功能入口（占位）" }
                SettingsRow { dt: root.dt; title: "清理本地统计"; description: "清理入口（占位）" }
            }

            SettingsSection {
                dt: root.dt
                title: "AI"
                Layout.fillWidth: true
                visible: root.backendRef ? root.backendRef.ai_available : false
                SettingsRow {
                    dt: root.dt
                    title: "启用 AI 功能"
                    description: "控制 AI 功能入口显示"
                    clickable: true
                    onClicked: root.setSwitchValue(aiSwitch, "ai_enabled", !aiSwitch.checked)
                    ModernSwitch { id: aiSwitch; dt: root.dt; onToggled: function(v) { root.setSwitchValue(aiSwitch, "ai_enabled", v) } }
                }
            }
        }
    }
}
