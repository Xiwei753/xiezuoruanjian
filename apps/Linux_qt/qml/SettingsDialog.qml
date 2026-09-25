// =============================================================================
// SettingsDialog.qml — 设置对话框
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：编辑器设置展示与保存（字号、行距、自动保存、主题、AI 开关）
// 约束：
//   - 通过 settingsBackendRef 兼容入口读写设置属性
//   - 不直接操作文件系统，通过 settingsBackendRef.save_local_settings() 持久化
//   - 使用 DesignTokens 统一样式
//   - Section 顺序按 Core settings_presentation 契约：
//     外观 → 编辑器和动画 → 保存和同步 → AI → 诊断与日志 → 关于
//   - Issue #701 评论 5702214893: 主题设置（appearance_mode、color_source、
//     selected_builtin_theme_id、selected_palette_id）的读写统一收口到
//     themeControllerRef（LinuxThemeController），不再走 settingsBackendRef.setting_*
//     fallback。themeControllerRef 是必需依赖。主题列表数据
//     （list_builtin_themes_json / list_palette_records_json）仍由
//     settingsBackendRef 提供。字号、行距、动画、保存等非主题设置仍走 settingsBackendRef。
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
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var themeControllerRef: null
    property var beforeSyncHook: null
    property var dt: theme
    property bool updatingValues: false
    property bool settingsDirty: false
    property var _saveTimer: null
    signal settingsChanged()
    // Issue #762 评论 5826175490 第 4 点：转发 SyncPage 的 openConflict 信号给主窗口
    signal openConflict(string projectId, string path)

    background: Rectangle { color: dt.surface; border.color: dt.border; border.width: 1; radius: dt.radiusXl }
    header: null

    function saveAndNotify() { if (!settingsBackendRef || !root.settingsDirty) return; settingsBackendRef.save_local_settings(); root.settingsDirty = false; root.settingsChanged() }
    // Debounced save: 使用 SettingsBackend 统一的 debounced_save_local_settings
    // 所有设置入口共用同一个保存事务
    function debouncedSave() {
        if (!settingsBackendRef) return
        root.settingsDirty = true
        settingsBackendRef.debounced_save_local_settings()
    }
    // Force-save: called when dialog closes.
    // Only saves if settingsDirty is true.
    function flushSave() {
        if (!root.settingsDirty) return
        if (settingsBackendRef) settingsBackendRef.flush_pending_settings_save()
        root.settingsDirty = false
        root.settingsChanged()
    }
    function setSwitchValue(control, key, value) {
        control.checked = value
        if (!settingsBackendRef || updatingValues) return
        settingsBackendRef[key] = value
        root.settingsDirty = true
        debouncedSave()
    }
    // Issue #756: 协同动画开关。协同模式下"协同动画持续时间"复用
    // setting_typing_animation_duration_ms 作为共享 timeline 时长，因此两个 duration
    // 滑块（协同 / 打字）必须同时刷新到共享值，否则关闭协同时 onClosed 会把陈旧的
    // typingAnimDuration.value 写回去，覆盖用户刚设的共享时长。
    function setCoordinatedAnimation(value) {
        coordinatedAnim.checked = value
        if (!settingsBackendRef || updatingValues) return
        settingsBackendRef.setting_coordinated_text_cursor_animation_enabled = value
        var shared = settingsBackendRef.setting_typing_animation_duration_ms || 100
        typingAnimDuration.value = shared
        coordinatedAnimDuration.value = shared
        root.settingsDirty = true
        debouncedSave()
    }
    function updateValues() {
        if (!settingsBackendRef) return
        updatingValues = true
        autoSave.checked = settingsBackendRef.setting_auto_save_enabled
        typingAnim.checked = settingsBackendRef.setting_typing_animation_enabled
        smoothCursor.checked = settingsBackendRef.setting_smooth_cursor_enabled
        // Issue #756: 恢复协同动画显式模式开关。
        coordinatedAnim.checked = settingsBackendRef.setting_coordinated_text_cursor_animation_enabled
        aiSwitch.checked = settingsBackendRef.ai_enabled
        autoSaveDelay.value = settingsBackendRef.setting_auto_save_delay_ms / 1000.0
        fontSizeSlider.value = settingsBackendRef.setting_font_size || 16.0
        lineSpacingSlider.value = settingsBackendRef.setting_line_spacing || 1.5
        autoIndent.checked = settingsBackendRef.setting_auto_indent_enabled
        autoIndentWidth.value = settingsBackendRef.setting_auto_indent_width || 2.0
        typingAnimDuration.value = settingsBackendRef.setting_typing_animation_duration_ms || 100
        smoothCursorDuration.value = settingsBackendRef.setting_smooth_cursor_duration_ms || 80
        // Issue #756: 协同模式 duration 滑块复用 typing_animation_duration_ms 作为共享 timeline 时长。
        coordinatedAnimDuration.value = settingsBackendRef.setting_typing_animation_duration_ms || 100
        var mode = themeControllerRef ? themeControllerRef.appearance_mode : "system"
        themeCombo.currentIndex = mode === "light" ? 1 : (mode === "dark" ? 2 : 0)
        // Issue #701 评论 5702675971: colorSourceCombo / builtinThemeCombo /
        // paletteRecordCombo 的 currentIndex 收口到 updateValues()，统一从
        // themeControllerRef 读取。原来这三个 combo 只靠各自
        // Component.onCompleted 初始化，当设置页用 Loader 加载且关闭后不销毁
        // 时，重新打开不会再次触发 Component.onCompleted，导致显示旧选择。
        var src = themeControllerRef ? themeControllerRef.color_source : "built_in"
        colorSourceCombo.currentIndex = src === "saved_palette" ? 1 : 0
        var builtinSelId = themeControllerRef ? themeControllerRef.selected_builtin_theme_id : ""
        var builtinThemes = builtinThemeCombo._themes
        var builtinFound = -1
        for (var bi = 0; bi < builtinThemes.length; bi++) {
            if (builtinThemes[bi].theme_id === builtinSelId) { builtinFound = bi; break }
        }
        builtinThemeCombo.currentIndex = builtinFound >= 0 ? builtinFound : 0
        var paletteSelId = themeControllerRef ? themeControllerRef.selected_palette_id : ""
        var paletteRecords = paletteRecordCombo._records
        var paletteFound = -1
        for (var pi = 0; pi < paletteRecords.length; pi++) {
            if (paletteRecords[pi].palette_id === paletteSelId) { paletteFound = pi; break }
        }
        paletteRecordCombo.currentIndex = paletteFound >= 0 ? paletteFound : 0
        diagnosticsEnabled.checked = settingsBackendRef.setting_diagnostics_enabled
        diagnosticsVerbose.checked = settingsBackendRef.setting_diagnostics_verbose
        diagnosticsVerbose.enabled = settingsBackendRef.setting_diagnostics_enabled
        // Issue #701 评论 5699565102: useAndroidTheme 开关已删除
        // （依赖已删除的 hasThemePalette，且与颜色来源下拉功能重复）。
        updatingValues = false
        // Issue #756: 删除对不存在的 coordinatedFixed 的悬空访问（Issue #727 遗留）。
    }
    onOpened: {
        // Issue #696 评论 5696993601: 删除 load_local_settings()。
        // 设置页打开只做 updateValues()，不重新加载全局设置。
        // 启动时 load_local_settings 已由 internal_open_data_root 完成，
        // 每开一次窗口重新加载会顺带触发一次主题切换。
        if (syncBackendRef) syncBackendRef.load_sync_config()
        updateValues()
    }
    onClosed: {
        // Force-write all slider current values back to settingsBackendRef before saving
        if (settingsBackendRef && !root.updatingValues) {
            settingsBackendRef.setting_font_size = fontSizeSlider.value
            settingsBackendRef.setting_line_spacing = lineSpacingSlider.value
            settingsBackendRef.setting_auto_indent_width = autoIndentWidth.value
        settingsBackendRef.setting_auto_save_delay_ms = autoSaveDelay.value * 1000
        // Issue #756: 协同模式下文字与光标共用 typing_animation_duration_ms 作为共享
        // timeline 时长，此时权威控件是"协同动画持续时间"滑块，必须写它的值；
        // 非协同模式继续分别写两个独立 duration（不覆盖用户保留的独立设置）。
        var coordinated = settingsBackendRef.setting_coordinated_text_cursor_animation_enabled
        if (coordinated) {
            settingsBackendRef.setting_typing_animation_duration_ms = coordinatedAnimDuration.value
        } else {
            settingsBackendRef.setting_typing_animation_duration_ms = typingAnimDuration.value
            settingsBackendRef.setting_smooth_cursor_duration_ms = smoothCursorDuration.value
        }
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
            AppText { dt: root.dt; text: qsTr("设置"); color: dt.textPrimary; font.pointSize: dt.subtitlePt; font.family: dt.fontFamily; font.weight: Font.Bold; Layout.fillWidth: true }
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
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues) return
                            // Issue #696 评论 5696993601 / #701 评论 5702214893:
                            // 用户切换主题只走 ThemeController 这一条入口，
                            // 不再直接写 settingsBackendRef.setting_appearance_mode。
                            // set_appearance_mode 内部写 AppBackend、重建缓存并
                            // 发 scheme_changed，然后沿现有保存入口落盘。
                            themeControllerRef.set_appearance_mode(["system", "light", "dark"][index])
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
                    onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_font_size = value }
                    onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_font_size = value; root.debouncedSave() }
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
                    onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_line_spacing = value }
                    onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_line_spacing = value; root.debouncedSave() }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("颜色来源")
                    description: qsTr("选择素笺默认主题或已保存的设备配色")
                    ModernComboBox {
                        id: colorSourceCombo
                        dt: root.dt
                        model: [qsTr("素笺默认"), qsTr("已保存的设备配色")]
                        onActivated: function(index) {
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues) return
                            // Issue #701 评论 5702214893: 颜色来源统一走
                            // ThemeController，不再直接写
                            // settingsBackendRef.setting_color_source。
                            var source = ["built_in", "saved_palette"][index]
                            themeControllerRef.set_color_source(source)
                            root.settingsDirty = true
                            root.saveAndNotify()
                        }
                    }
                }
                SettingsRow {
                    dt: root.dt
                    visible: themeControllerRef ? themeControllerRef.color_source === "built_in" : false
                    title: qsTr("内置主题")
                    description: qsTr("选择内置主题配色方案")
                    ModernComboBox {
                        id: builtinThemeCombo
                        dt: root.dt
                        property var _themes: {
                            if (!settingsBackendRef) return []
                            try { return JSON.parse(settingsBackendRef.list_builtin_themes_json()) } catch(e) { return [] }
                        }
                        model: _themes.map(function(t) { return t.name || t.theme_id })
                        onActivated: function(index) {
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues) return
                            var themeId = _themes[index] ? _themes[index].theme_id : ""
                            if (themeId.length > 0) {
                                // Issue #701 评论 5702214893: 内置主题统一走
                                // ThemeController。set_selected_builtin_theme_id
                                // 内部会同时把 color_source 设为 built_in。
                                themeControllerRef.set_selected_builtin_theme_id(themeId)
                                root.settingsDirty = true
                                root.saveAndNotify()
                            }
                        }
                    }
                }
                SettingsRow {
                    dt: root.dt
                    visible: themeControllerRef ? themeControllerRef.color_source === "saved_palette" : false
                    title: qsTr("已保存配色")
                    description: qsTr("选择已保存的设备调色板")
                    ModernComboBox {
                        id: paletteRecordCombo
                        dt: root.dt
                        property var _records: {
                            if (!settingsBackendRef) return []
                            try { return JSON.parse(settingsBackendRef.list_palette_records_json()) } catch(e) { return [] }
                        }
                        model: _records.map(function(r) {
                            var d = new Date(r.captured_at_ms)
                            return (r.source_platform || "") + " · " + (r.source_device_class || "") + " · " + (r.source_device_id || "") + " · " + d.toLocaleDateString()
                        })
                        onActivated: function(index) {
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues) return
                            var paletteId = _records[index] ? _records[index].palette_id : ""
                            if (paletteId.length > 0) {
                                // Issue #701 评论 5702214893: 已保存 palette 统一走
                                // ThemeController。set_selected_palette_id 内部会
                                // 同时把 color_source 设为 saved_palette。
                                themeControllerRef.set_selected_palette_id(paletteId)
                                root.settingsDirty = true
                                root.saveAndNotify()
                            }
                        }
                    }
                }
            }

            // ── 2. 编辑器和动画 (editor + animation) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("编辑器和动画")
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
                    onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_auto_indent_width = value }
                    onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_auto_indent_width = value; root.debouncedSave() }
                }
                // Issue #756: 协同动画（吞字/吐字）显式模式开关。
                // true 时文字与光标绑死，共用一个 timeline duration（setting_typing_animation_duration_ms），
                // 要求有效 caret motion 否则文字动画也不启动；
                // false 时 typing/smooth 两个独立开关各自决定文字/光标动画。
                SettingsRow {
                    dt: root.dt
                    title: qsTr("协同动画（吞字/吐字）")
                    description: qsTr("文字与光标绑死共用一条时间线")
                    clickable: true
                    onClicked: root.setCoordinatedAnimation(!coordinatedAnim.checked)
                    ModernSwitch { id: coordinatedAnim; dt: root.dt; onToggled: function(v) { root.setCoordinatedAnimation(v) } }
                }
                // 协同模式：显示一个"协同动画持续时间"滑块，复用 setting_typing_animation_duration_ms。
                AppSlider {
                    id: coordinatedAnimDuration
                    Layout.fillWidth: true
                    dt: root.dt
                    label: qsTr("协同动画持续时间")
                    valueText: Math.round(value) + " ms"
                    // range from Core settings_presentation: min=30, max=1000, step=10
                    from: 30
                    to: 1000
                    stepSize: 10
                    // Issue #756: 仅协同模式显示；非协同模式隐藏（用独立 duration）。
                    visible: coordinatedAnim.checked
                    onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_typing_animation_duration_ms = value }
                    onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_typing_animation_duration_ms = value; root.debouncedSave() }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("打字动画")
                    description: qsTr("输入时字符从光标处吐出")
                    clickable: true
                    // Issue #756: 协同模式开启时隐藏独立开关（文字由协同模式统一控制）。
                    visible: !coordinatedAnim.checked
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
                    // Issue #756: 协同模式隐藏独立 duration（由协同 duration 滑块代替）。
                    visible: !coordinatedAnim.checked
                    onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_typing_animation_duration_ms = value }
                    onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_typing_animation_duration_ms = value; root.debouncedSave() }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("平滑光标")
                    description: qsTr("光标移动更顺滑")
                    clickable: true
                    // Issue #756: 协同模式开启时隐藏独立开关（光标由协同模式统一控制）。
                    visible: !coordinatedAnim.checked
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
                    // Issue #756: 协同模式隐藏独立 duration（由协同 duration 滑块代替）。
                    visible: !coordinatedAnim.checked
                    onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_smooth_cursor_duration_ms = value }
                    onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_smooth_cursor_duration_ms = value; root.debouncedSave() }
                }
            }

            // ── 3. 保存和同步 (save + sync) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("保存和同步")
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
                    onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_auto_save_delay_ms = value * 1000 }
                    onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_auto_save_delay_ms = value * 1000; root.debouncedSave() }
                }
                SyncPage {
                    Layout.fillWidth: true
                    dt: root.dt
                    syncBackendRef: root.syncBackendRef
                    workspaceBackendRef: root.workspaceBackendRef
                    beforeSyncHook: function() {
                        if (typeof root.beforeSyncHook === "function") return root.beforeSyncHook();
                        return true;
                    }
                    onSettingsChanged: root.settingsChanged()
                    // Issue #762 评论 5826175490 第 4 点：转发 openConflict 信号给主窗口
                    onOpenConflict: function(projectId, path) {
                        root.openConflict(projectId, path)
                    }
                }
            }

            // ── 4. AI (ai) ──
            SettingsSection {
                dt: root.dt
                title: qsTr("AI")
                Layout.fillWidth: true
                visible: root.settingsBackendRef ? root.settingsBackendRef.ai_available : false
                SettingsRow {
                    dt: root.dt
                    title: qsTr("启用 AI 功能")
                    description: qsTr("控制 AI 功能入口显示")
                    clickable: true
                    onClicked: root.setSwitchValue(aiSwitch, "ai_enabled", !aiSwitch.checked)
                    ModernSwitch { id: aiSwitch; dt: root.dt; onToggled: function(v) { root.setSwitchValue(aiSwitch, "ai_enabled", v) } }
                }
            }

            // ── 5. 诊断与日志 (diagnostics) ──
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
                        if (!root.settingsBackendRef) return
                        root.settingsBackendRef.clear_logs()
                    }
                }
                // 导出诊断包：独立行，明确按钮
                RowLayout {
                    Layout.fillWidth: true
                    spacing: dt.sp12
                    AppText {
                        dt: root.dt
                        Layout.fillWidth: true
                        text: qsTr("导出诊断包")
                        color: dt.textSecondary
                        font.pointSize: dt.captionPt
                        font.family: dt.fontFamily
                    }
                    AppButton {
                        text: qsTr("导出")
                        dt: root.dt
                        variant: "secondary"
                        small: true
                        onClicked: {
                            if (!root.settingsBackendRef) return
                            var result = root.settingsBackendRef.export_diagnostics_pack()
                            // parse JSON envelope
                            try {
                                var obj = JSON.parse(result)
                                if (obj.success) {
                                    var zipPath = obj.nativeZipPath || obj.zipPath || obj.nativePath || obj.path || ""
                                    var exportDir = obj.nativeExportDir || obj.exportDir || ""
                                    diagnosticsFeedback.message = qsTr("日志 zip: ") + zipPath + "\n" + qsTr("导出目录: ") + exportDir
                                    if (obj.openedExportDir === false && obj.openExportDirError) {
                                        diagnosticsFeedback.message += "\n" + qsTr("打开目录失败：") + obj.openExportDirError
                                    }
                                    diagnosticsFeedback.isError = false
                                    // 后端已用平台文件管理器打开目录；作为兜底，QML 尝试打开导出目录 URL。
                                    if (obj.openedExportDir === false && obj.exportDirUrl) {
                                        Qt.openUrlExternally(obj.exportDirUrl)
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
                    dt: root.dt
                    property string message: ""
                    property bool isError: false
                    visible: message.length > 0
                    text: diagnosticsFeedback.message
                    color: isError ? dt.error : dt.textSecondary
                    font.pointSize: dt.captionPt
                    font.family: dt.fontFamily
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("复制设备信息")
                    description: qsTr("将设备信息复制到剪贴板")
                    clickable: true
                    onClicked: {
                        if (!root.settingsBackendRef) return
                        var info = root.settingsBackendRef.copy_device_info()
                        if (info && info.length > 0) {
                            var result = root.settingsBackendRef.copy_text_to_clipboard(info)
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
                        dt: root.dt
                        property string message: ""
                        visible: message.length > 0
                        text: deviceInfoFeedback.message
                        color: dt.textSecondary
                        font.pointSize: dt.captionPt
                        font.family: dt.fontFamily
                    }
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("打开日志目录")
                    description: qsTr("在文件管理器中打开日志目录")
                    clickable: true
                    onClicked: {
                        if (!root.settingsBackendRef) return
                        root.settingsBackendRef.open_log_directory()
                    }
                }
            }

            // ── 6. 关于 (about) ──
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
                    description: "github.com/Xiwei753/xiezuoruanjian"
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("开源协议")
                    description: "GPLv3"
                }
                SettingsRow {
                    dt: root.dt
                    title: qsTr("版本信息")
                    description: qsTr("Linux_qt 客户端")
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

    // Issue #695 评论 5693346400: 桌面滚轮事件直译器
    // 避免设置页回到 Qt Flickable 自己的鼠标 wheel acceleration
    DesktopWheelScrollHandler {
        targetFlickable: settingsScroll.contentItem
        // 设置页没有明确的字体行距，用一个设置行的大致高度作为每格滚动距离
        lineSpacingPx: 56
        anchors.fill: settingsScroll
    }
}
