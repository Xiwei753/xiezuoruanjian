// =============================================================================
// SettingsDialog.qml — 设置对话框
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
// 职责：编辑器设置展示与保存（字号、行距、自动保存、主题、AI 开关）
// 约束：
//   - 通过 settingsBackend 兼容入口读写设置属性
//   - 不直接操作文件系统，通过 backendRef.save_local_settings() 持久化
//   - 使用 DesignTokens 统一样式
//   - Section 顺序按 Core settings_presentation 契约：
//     外观 → 编辑器 → 保存 → 同步 → AI → 关于/高级
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    modal: true
    width: 640
    height: Math.max(480, Math.min(720, settingsScroll.contentHeight + 120))
    parent: Overlay.overlay
    x: Math.round((parent.width - width) / 2)
    y: Math.round((parent.height - height) / 2)
    property var theme: null
    property var backendRef: null
    property var workspaceBackendRef: null
    property var dt: theme
    property bool updatingValues: false
    signal settingsChanged()

    background: Rectangle { color: dt ? dt.surface : "#1A1D23"; border.color: dt ? dt.border : "#2A2E36"; border.width: 1; radius: dt ? dt.radiusXl : 24 }
    header: null

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
        typingAnim.checked = false
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
        height: 64
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt ? dt.sp24 : 24
            anchors.rightMargin: dt ? dt.sp16 : 16
            AppText { text: qsTr("设置"); color: dt ? dt.textPrimary : "#E2E2E5"; font.pixelSize: dt ? dt.subtitle : 18; font.family: dt ? dt.fontFamily : "sans-serif"; font.weight: Font.Bold; Layout.fillWidth: true }
            ToolbarButton { text: qsTr("关闭"); theme: root.dt; onClicked: root.close() }
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

            // ── 1. 外观 (appearance) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("外观")
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: qsTr("主题模式")
                    description: qsTr("切换系统、浅色或深色")
                    ModernComboBox {
                        id: themeCombo
                        dt: root.dt
                        model: [qsTr("跟随系统"), qsTr("浅色"), qsTr("深色")]
                        onActivated: function(index) {
                            if (!backendRef || root.updatingValues) return
                            backendRef.setting_theme_mode = ["system", "light", "dark"][index]
                            root.saveAndNotify()
                        }
                    }
                }
                AppSlider {
                    id: fontSizeSlider
                    Layout.fillWidth: true
                    theme: root.dt
                    label: qsTr("字体大小")
                    valueText: Math.round(value) + " px"
                    from: 12.0
                    to: 120.0
                    stepSize: 1.0
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_font_size = value; root.saveAndNotify() }
                }
                AppSlider {
                    id: lineSpacingSlider
                    Layout.fillWidth: true
                    theme: root.dt
                    label: qsTr("行距倍数")
                    valueText: Number(value).toFixed(1) + "x"
                    from: 1.0
                    to: 3.0
                    stepSize: 0.1
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_line_spacing = value; root.saveAndNotify() }
                }
            }

            // ── 2. 编辑器 (editor) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("编辑器")
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: qsTr("自动首行缩进")
                    description: qsTr("回车时自动添加缩进")
                    clickable: true
                    onClicked: root.setSwitchValue(autoIndent, "setting_auto_indent_enabled", !autoIndent.checked)
                    ModernSwitch { id: autoIndent; dt: root.dt; onToggled: function(v) { root.setSwitchValue(autoIndent, "setting_auto_indent_enabled", v) } }
                }
                AppSlider {
                    id: autoIndentWidth
                    Layout.fillWidth: true
                    theme: root.dt
                    label: qsTr("首行缩进宽度")
                    valueText: Number(value).toFixed(1) + qsTr(" 字符")
                    from: 0.0
                    to: 8.0
                    stepSize: 0.5
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_indent_width = value; root.saveAndNotify() }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("打字动画（重构中）")
                    description: qsTr("旧 TextArea hidden-range 动画已停用，等待 SujianEditorItem")
                    clickable: false
                    ModernSwitch { id: typingAnim; dt: root.dt; enabled: false }
                }
                AppSlider {
                    id: typingAnimDuration
                    Layout.fillWidth: true
                    theme: root.dt
                    label: qsTr("打字动画持续时间（当前自研写作区下不可用）")
                    enabled: false
                    valueText: Math.round(value) + " ms"
                    from: 0
                    to: 1000
                    stepSize: 10
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_typing_animation_duration_ms = value; root.saveAndNotify() }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("平滑光标")
                    description: qsTr("光标移动更顺滑")
                    clickable: true
                    onClicked: root.setSwitchValue(smoothCursor, "setting_smooth_cursor_enabled", !smoothCursor.checked)
                    ModernSwitch { id: smoothCursor; dt: root.dt; onToggled: function(v) { root.setSwitchValue(smoothCursor, "setting_smooth_cursor_enabled", v) } }
                }
                AppSlider {
                    id: smoothCursorDuration
                    Layout.fillWidth: true
                    theme: root.dt
                    label: qsTr("平滑光标持续时间")
                    valueText: Math.round(value) + " ms"
                    from: 0
                    to: 1000
                    stepSize: 10
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_smooth_cursor_duration_ms = value; root.saveAndNotify() }
                }
            }

            // ── 3. 保存 (save) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("保存")
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: qsTr("自动保存")
                    description: qsTr("编辑时自动保存到本地")
                    clickable: true
                    onClicked: root.setSwitchValue(autoSave, "setting_auto_save_enabled", !autoSave.checked)
                    ModernSwitch { id: autoSave; dt: root.dt; onToggled: function(v) { root.setSwitchValue(autoSave, "setting_auto_save_enabled", v) } }
                }
                AppSlider {
                    id: autoSaveDelay
                    Layout.fillWidth: true
                    theme: root.dt
                    label: qsTr("自动保存延迟")
                    valueText: Math.round(value) + qsTr(" 秒")
                    from: 1
                    to: 10
                    stepSize: 1
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_save_delay_ms = value * 1000; root.saveAndNotify() }
                }
            }

            // ── 4. 同步 (sync) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("同步")
                Layout.fillWidth: true
                SettingsRow { dt: root.dt; title: qsTr("同步配置"); description: qsTr("在同步页面管理仓库与鉴权") }
            }

            // ── 5. AI (ai) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("AI")
                Layout.fillWidth: true
                visible: root.backendRef ? root.backendRef.ai_available : false
                SettingsRow {
                    dt: root.dt
                    title: qsTr("启用 AI 功能")
                    description: qsTr("控制 AI 功能入口显示")
                    clickable: true
                    onClicked: root.setSwitchValue(aiSwitch, "ai_enabled", !aiSwitch.checked)
                    ModernSwitch { id: aiSwitch; dt: root.dt; onToggled: function(v) { root.setSwitchValue(aiSwitch, "ai_enabled", v) } }
                }
            }

            // ── 6. 关于/高级 (about) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("关于/高级")
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: qsTr("工作区路径")
                    description: root.workspaceBackendRef ? root.workspaceBackendRef.workspace_path : qsTr("未加载")
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("版本信息")
                    description: qsTr("Qt 桌面客户端")
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("动作注册表")
                    description: qsTr("查看已注册的动作")
                    clickable: true
                    onClicked: { /* Navigate to ActionRegistryPage later */ }
                }
            }
        }
    }
}
