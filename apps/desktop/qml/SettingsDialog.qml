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
//     外观 → 编辑器 → 保存 → 同步 → AI → 诊断与日志 → 关于/高级
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
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var dt: theme
    property bool updatingValues: false
    property bool settingsDirty: false
    property var _saveTimer: null
    signal settingsChanged()

    background: Rectangle { color: dt.surface; border.color: dt.border; border.width: 1; radius: dt.radiusXl }
    header: null

    function saveAndNotify() { if (!backendRef || !root.settingsDirty) return; backendRef.save_local_settings(); root.settingsDirty = false; root.settingsChanged() }
    // Debounced save: 使用 SettingsBackend 统一的 debounced_save_local_settings
    // 所有设置入口共用同一个保存事务
    function debouncedSave() {
        if (!backendRef) return
        root.settingsDirty = true
        backendRef.debounced_save_local_settings()
    }
    // Force-save: called when dialog closes.
    // Only saves if settingsDirty is true.
    function flushSave() {
        if (!root.settingsDirty) return
        if (backendRef) backendRef.flush_pending_settings_save()
        root.settingsDirty = false
        root.settingsChanged()
    }
    function setSwitchValue(control, key, value) {
        control.checked = value
        if (!backendRef || updatingValues) return
        backendRef[key] = value
        root.settingsDirty = true
        debouncedSave()
    }
    function updateValues() {
        if (!backendRef) return
        updatingValues = true
        autoSave.checked = backendRef.setting_auto_save_enabled
        typingAnim.checked = backendRef.setting_typing_animation_enabled
        smoothCursor.checked = backendRef.setting_smooth_cursor_enabled
        coordinatedCursorAnim.checked = backendRef.setting_coordinated_text_cursor_animation_enabled
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
        diagnosticsEnabled.checked = backendRef.setting_diagnostics_enabled
        diagnosticsVerbose.checked = backendRef.setting_diagnostics_verbose
        diagnosticsVerbose.enabled = backendRef.setting_diagnostics_enabled
        updatingValues = false
    }
    onOpened: {
        if (backendRef) backendRef.load_local_settings()
        if (syncBackendRef) syncBackendRef.load_sync_config()
        updateValues()
    }
    onClosed: {
        // Force-write all slider current values back to backendRef before saving
        if (backendRef && !root.updatingValues) {
            backendRef.setting_font_size = fontSizeSlider.value
            backendRef.setting_line_spacing = lineSpacingSlider.value
            backendRef.setting_auto_indent_width = autoIndentWidth.value
            backendRef.setting_auto_save_delay_ms = autoSaveDelay.value * 1000
            backendRef.setting_typing_animation_duration_ms = typingAnimDuration.value
            backendRef.setting_smooth_cursor_duration_ms = smoothCursorDuration.value
            root.settingsDirty = true
        }
        flushSave()
    }

    Rectangle {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 64
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt.sp24
            anchors.rightMargin: dt.sp16
            AppText { text: qsTr("设置"); color: dt.textPrimary; font.pixelSize: dt.subtitle; font.family: dt.fontFamily; font.weight: Font.Bold; Layout.fillWidth: true }
            ToolbarButton { text: qsTr("关闭"); dt: root.dt; onClicked: root.close() }
        }
    }

    ScrollView {
        id: settingsScroll
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.top: topBar.bottom
        anchors.leftMargin: dt.sp20
        anchors.rightMargin: dt.sp20
        anchors.bottomMargin: dt.sp20
        anchors.topMargin: dt.sp8
        clip: true
        contentWidth: availableWidth
        contentHeight: settingsColumn.implicitHeight

        ColumnLayout {
            id: settingsColumn
            width: settingsScroll.availableWidth
            spacing: dt.cardGap

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
                            root.settingsDirty = true
                            root.saveAndNotify()
                        }
                    }
                }
                AppSlider {
                    id: fontSizeSlider
                    Layout.fillWidth: true
                    dt: root.dt
                    label: qsTr("字体大小")
                    valueText: Math.round(value) + " px"
                    // range from Core settings_presentation: min=12, max=72, step=1
                    from: 12.0
                    to: 72.0
                    stepSize: 1.0
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_font_size = value }
                    onCommitted: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_font_size = value; root.debouncedSave() }
                }
                AppSlider {
                    id: lineSpacingSlider
                    Layout.fillWidth: true
                    dt: root.dt
                    label: qsTr("行距倍数")
                    valueText: Number(value).toFixed(1) + "x"
                    // range from Core settings_presentation: min=1.0, max=3.0, step=0.1
                    from: 1.0
                    to: 3.0
                    stepSize: 0.1
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_line_spacing = value }
                    onCommitted: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_line_spacing = value; root.debouncedSave() }
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
                    dt: root.dt
                    label: qsTr("首行缩进宽度")
                    valueText: Number(value).toFixed(1) + qsTr(" 字符")
                    // range from Core settings_presentation: min=0.0, max=8.0, step=0.5
                    from: 0.0
                    to: 8.0
                    stepSize: 0.5
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_indent_width = value }
                    onCommitted: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_indent_width = value; root.debouncedSave() }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("打字动画")
                    description: qsTr("输入时字符从光标处吐出")
                    clickable: true
                    onClicked: root.setSwitchValue(typingAnim, "setting_typing_animation_enabled", !typingAnim.checked)
                    ModernSwitch { id: typingAnim; dt: root.dt; onToggled: function(v) { root.setSwitchValue(typingAnim, "setting_typing_animation_enabled", v) } }
                }
                AppSlider {
                    id: typingAnimDuration
                    Layout.fillWidth: true
                    dt: root.dt
                    label: qsTr("打字动画持续时间")
                    valueText: Math.round(value) + " ms"
                    // range from Core settings_presentation: min=30, max=1000, step=10
                    from: 30
                    to: 1000
                    stepSize: 10
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_typing_animation_duration_ms = value }
                    onCommitted: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_typing_animation_duration_ms = value; root.debouncedSave() }
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
                    dt: root.dt
                    label: qsTr("平滑光标持续时间")
                    valueText: Math.round(value) + " ms"
                    // range from Core settings_presentation: min=30, max=1000, step=10
                    from: 30
                    to: 1000
                    stepSize: 10
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_smooth_cursor_duration_ms = value }
                    onCommitted: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_smooth_cursor_duration_ms = value; root.debouncedSave() }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("协同光标动画")
                    description: qsTr("光标与吐字动画协同移动")
                    clickable: true
                    onClicked: root.setSwitchValue(coordinatedCursorAnim, "setting_coordinated_text_cursor_animation_enabled", !coordinatedCursorAnim.checked)
                    ModernSwitch { id: coordinatedCursorAnim; dt: root.dt; onToggled: function(v) { root.setSwitchValue(coordinatedCursorAnim, "setting_coordinated_text_cursor_animation_enabled", v) } }
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
                    dt: root.dt
                    label: qsTr("自动保存延迟")
                    valueText: Math.round(value) + qsTr(" 秒")
                    // range from Core settings_presentation: min=1, max=10, step=1
                    from: 1
                    to: 10
                    stepSize: 1
                    onMoved: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_save_delay_ms = value * 1000 }
                    onCommitted: function() { if (!backendRef || root.updatingValues) return; backendRef.setting_auto_save_delay_ms = value * 1000; root.debouncedSave() }
                }
            }

            // ── 4. 同步 (sync) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("同步")
                Layout.fillWidth: true
                SyncPage {
                    Layout.fillWidth: true
                    theme: root.dt
                    backendRef: root.syncBackendRef
                    beforeSyncHook: function() {
                        if (root.editorBackendRef) root.editorBackendRef.flush_writing_stats();
                    }
                    onSettingsChanged: root.settingsChanged()
                }
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

            // ── 6. 诊断与日志 (diagnostics) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("诊断与日志")
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: qsTr("启用诊断日志")
                    description: qsTr("记录应用运行日志，用于问题排查")
                    clickable: true
                    onClicked: root.setSwitchValue(diagnosticsEnabled, "setting_diagnostics_enabled", !diagnosticsEnabled.checked)
                    ModernSwitch { id: diagnosticsEnabled; dt: root.dt; onToggled: function(v) {
                        root.setSwitchValue(diagnosticsEnabled, "setting_diagnostics_enabled", v)
                        diagnosticsVerbose.enabled = v
                        if (!v) {
                            diagnosticsVerbose.checked = false
                            root.setSwitchValue(diagnosticsVerbose, "setting_diagnostics_verbose", false)
                        }
                    }}
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("详细日志")
                    description: qsTr("记录更详细的调试信息")
                    clickable: true
                    onClicked: root.setSwitchValue(diagnosticsVerbose, "setting_diagnostics_verbose", !diagnosticsVerbose.checked)
                    ModernSwitch { id: diagnosticsVerbose; dt: root.dt; onToggled: function(v) { root.setSwitchValue(diagnosticsVerbose, "setting_diagnostics_verbose", v) } }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("清空日志")
                    description: qsTr("删除所有日志文件")
                    clickable: true
                    onClicked: {
                        if (!root.backendRef) return
                        root.backendRef.clear_logs()
                    }
                }
                // 导出诊断包：独立行，明确按钮
                RowLayout {
                    Layout.fillWidth: true
                    spacing: dt.sp12
                    AppText {
                        Layout.fillWidth: true
                        text: qsTr("导出诊断包")
                        color: dt.textSecondary
                        font.pixelSize: dt.caption
                        font.family: dt.fontFamily
                    }
                    AppButton {
                        text: qsTr("导出")
                        dt: root.dt
                        variant: "secondary"
                        small: true
                        onClicked: {
                            if (!root.backendRef) return
                            var result = root.backendRef.export_diagnostics_pack()
                            // parse JSON envelope
                            try {
                                var obj = JSON.parse(result)
                                if (obj.success) {
                                    // 使用 nativePath 显示路径（原生分隔符），使用 fileUrl 打开目录
                                    diagnosticsFeedback.message = qsTr("已导出到: ") + (obj.nativePath || obj.path)
                                    diagnosticsFeedback.isError = false
                                    // 如果有 fileUrl，用 Qt.openUrlExternally 打开目录
                                    if (obj.fileUrl) {
                                        Qt.openUrlExternally(obj.fileUrl)
                                    }
                                } else {
                                    diagnosticsFeedback.message = qsTr("导出失败：") + (obj.error || qsTr("未知错误"))
                                    diagnosticsFeedback.isError = true
                                }
                            } catch(e) {
                                // 兼容旧格式：纯路径字符串
                                if (result && result.length > 0 && !result.startsWith("error.")) {
                                    diagnosticsFeedback.message = qsTr("已导出到: ") + result
                                    diagnosticsFeedback.isError = false
                                } else {
                                    diagnosticsFeedback.message = qsTr("导出失败")
                                    diagnosticsFeedback.isError = true
                                }
                            }
                        }
                    }
                }
                AppText {
                    id: diagnosticsFeedback
                    property string message: ""
                    property bool isError: false
                    visible: message.length > 0
                    text: diagnosticsFeedback.message
                    color: isError ? dt.error : dt.textSecondary
                    font.pixelSize: dt.caption
                    font.family: dt.fontFamily
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("复制设备信息")
                    description: qsTr("将设备信息复制到剪贴板")
                    clickable: true
                    onClicked: {
                        if (!root.backendRef) return
                        var info = root.backendRef.copy_device_info()
                        if (info && info.length > 0) {
                            var result = root.backendRef.copy_text_to_clipboard(info)
                            // result 是 JSON envelope：success=true 表示成功
                            try {
                                var obj = JSON.parse(result)
                                if (obj.success) {
                                    deviceInfoFeedback.message = qsTr("已复制")
                                } else {
                                    deviceInfoFeedback.message = qsTr("复制失败：") + (obj.messageKey || obj.rawError || qsTr("未知错误"))
                                }
                            } catch(e) {
                                if (result === "ok" || result.length > 0) {
                                    deviceInfoFeedback.message = qsTr("已复制")
                                } else {
                                    deviceInfoFeedback.message = qsTr("复制失败")
                                }
                            }
                        } else {
                            deviceInfoFeedback.message = qsTr("获取设备信息失败")
                        }
                    }
                    AppText {
                        id: deviceInfoFeedback
                        property string message: ""
                        visible: message.length > 0
                        text: deviceInfoFeedback.message
                        color: dt.textSecondary
                        font.pixelSize: dt.caption
                        font.family: dt.fontFamily
                    }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("打开日志目录")
                    description: qsTr("在文件管理器中打开日志目录")
                    clickable: true
                    onClicked: {
                        if (!root.backendRef) return
                        root.backendRef.open_log_directory()
                    }
                }
            }

            // ── 7. 关于 (about) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("关于")
                Layout.fillWidth: true
                SettingsRow {
                    dt: root.dt
                    title: qsTr("应用名")
                    description: qsTr("素笺写作")
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("作者")
                    description: "Xiwei753"
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("项目地址")
                    description: qsTr("GitHub: github.com/Xiwei753/xiezuoruanjian\nGitee 镜像: gitee.com/wei-xi753/xiezuoruanjian")
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("开源协议")
                    description: "GPLv3"
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("镜像说明")
                    description: qsTr("GitHub 为主仓库。Gitee 为镜像同步仓库，方便国内访问。")
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("版本信息")
                    description: qsTr("Desktop Qt 客户端")
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("工作区路径")
                    description: root.workspaceBackendRef ? root.workspaceBackendRef.workspace_path : qsTr("未加载")
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

    SmoothWheelScroller {
        anchors.fill: settingsScroll
        scrollView: settingsScroll
        lineHeight: 24
        fontPixelSize: 14
    }
}
