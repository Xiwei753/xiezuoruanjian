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
        autoSaveDelay.currentIndex = Math.max(0, [1, 2, 3, 5, 10].indexOf(Number(backendRef.setting_auto_save_delay_seconds || 3)))
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
                    title: "平滑光标"
                    description: "光标移动更顺滑"
                    clickable: true
                    onClicked: root.setSwitchValue(smoothCursor, "setting_smooth_cursor_enabled", !smoothCursor.checked)
                    ModernSwitch { id: smoothCursor; dt: root.dt; onToggled: function(v) { root.setSwitchValue(smoothCursor, "setting_smooth_cursor_enabled", v) } }
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
                    description: "停止输入后多久触发保存"
                    ModernComboBox {
                        id: autoSaveDelay
                        dt: root.dt
                        model: ["1 秒", "2 秒", "3 秒", "5 秒", "10 秒"]
                        onActivated: function(index) { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_save_delay_seconds = [1,2,3,5,10][index]; root.saveAndNotify() }
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
