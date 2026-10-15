// =============================================================================
// KdeSettingsPage.qml — KDE Kirigami Addons 设置页
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：编辑器/主题/同步/诊断设置的 KDE FormCard 实现。
// Issue #900：设置界面全面迁移到 KDE 官方组件体系：
//   - 分组卡片使用 org.kde.kirigamiaddons.formcard 的 FormCard/FormHeader；
//   - 布尔设置统一 FormSwitchDelegate（沿用 Breeze 自带 track/handle/palette，
//     不再自绘开关）；
//   - 下拉设置使用 FormComboBoxDelegate；
//   - 数值设置使用 FormSpinBoxDelegate（受卡片宽度约束，不再把开关/控件顶出卡片）；
//   - 长路径/反馈文本使用可换行 Label，并显式 Layout.fillWidth + minimumWidth: 0，
//     避免设置视图 contentWidth 被长字符串横向撑开。
// 布局（沿用 Issue #835 评论 6019235318 约定）：宽屏左右两列（左：外观/编辑器和
// 动画/AI；右：保存和同步/诊断与日志/关于），窄屏上下单列；两组 ColumnLayout 都
// visible，窄屏不丢设置。
// 约束：
//   - 设置项语义/数值范围/字段名与 Core settings_presentation 契约保持一致；
//   - 主题读写统一走 themeControllerRef，非主题设置走 settingsBackendRef；
//   - 本页不自己持久化，交给 settingsBackendRef 的 debounced/flush 保存事务。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.formcard as FormCard

ScrollView {
    id: root

    clip: true
    // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面，
    // 保留滚轮/触摸板/滚动条/触屏滚动。
    Component.onCompleted: {
        if (contentItem) contentItem.acceptedButtons = Qt.NoButton
    }
    contentWidth: availableWidth
    contentHeight: settingsColumns.height

    // ── 依赖注入（与旧 SettingsDialog 保持同一属性名） ──
    property var layoutPlan: null
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var themeControllerRef: null
    property var beforeSyncHook: null
    // 宽屏顶部搜索关键词：只做分组过滤，不改任何设置读取/保存路径。
    property string searchText: ""

    // Issue #836：layoutPlan 为 null 时默认宽屏 / Workbench。
    readonly property bool widePanel: layoutPlan === null || layoutPlan.primaryNavigationPlacement === "Side"
    readonly property bool overlayPanel: layoutPlan === null || layoutPlan.workspaceLayoutMode === "Workbench"

    signal settingsChanged()
    signal openConflict(string projectId, string path)
    signal switchWorkspaceRequested()

    property bool updatingValues: false
    // Issue #815 评论 5955676896：「打字动画持续时间」和「协同动画时长」是同一个设置项
    // setting_typing_animation_duration_ms，只有一份状态。
    property real textAnimationDurationValue: 100
    property bool settingsDirty: false
    // 防止两个共享同一设置项的控件互相回写时递归。
    property bool _syncingDuration: false

    // ── 分组过滤（宽屏搜索） ──
    function sectionVisible(title) {
        var q = root.searchText.trim().toLowerCase()
        if (q.length === 0)
            return true
        return String(title).toLowerCase().indexOf(q) !== -1
    }

    // ── 保存事务 ──
    function saveAndNotify() {
        if (!settingsBackendRef || !root.settingsDirty)
            return
        settingsBackendRef.save_local_settings()
        root.settingsDirty = false
        root.settingsChanged()
    }
    function debouncedSave() {
        if (!settingsBackendRef)
            return
        root.settingsDirty = true
        settingsBackendRef.debounced_save_local_settings()
    }
    function flushSave() {
        if (!root.settingsDirty)
            return
        if (settingsBackendRef)
            settingsBackendRef.flush_pending_settings_save()
        root.settingsDirty = false
        root.settingsChanged()
    }

    function setSwitchValue(control, key, value) {
        control.checked = value
        if (!settingsBackendRef || updatingValues)
            return
        settingsBackendRef[key] = value
        root.settingsDirty = true
        debouncedSave()
    }

    // Issue #815 评论 5955676896: 协同动画开关。协同开时文字与光标是同一条 caret track，
    // 没有两条独立 duration；协同动画时长就是「打字动画持续时间」这一项。
    function setCoordinatedAnimation(value) {
        coordinatedAnim.checked = value
        if (!settingsBackendRef || updatingValues)
            return
        settingsBackendRef.setting_coordinated_text_cursor_animation_enabled = value
        root.settingsDirty = true
        debouncedSave()
    }

    // 打字动画时长 / 协同动画时长的唯一写入口：backend 与另一个控件同时同步。
    function setTextAnimationDuration(value) {
        if (updatingValues || _syncingDuration)
            return
        root.textAnimationDurationValue = value
        _syncingDuration = true
        if (coordinatedAnimDuration.value !== value) coordinatedAnimDuration.value = value
        if (typingAnimDuration.value !== value) typingAnimDuration.value = value
        _syncingDuration = false
        if (settingsBackendRef)
            settingsBackendRef.setting_typing_animation_duration_ms = value
        root.settingsDirty = true
    }

    function updateValues() {
        if (!settingsBackendRef)
            return
        updatingValues = true
        autoSave.checked = settingsBackendRef.setting_auto_save_enabled
        typingAnim.checked = settingsBackendRef.setting_typing_animation_enabled
        smoothCursor.checked = settingsBackendRef.setting_smooth_cursor_enabled
        coordinatedAnim.checked = settingsBackendRef.setting_coordinated_text_cursor_animation_enabled
        aiSwitch.checked = settingsBackendRef.ai_enabled
        autoSaveDelaySpin.value = Math.round(settingsBackendRef.setting_auto_save_delay_ms / 1000.0)
        fontSizeSpin.value = Math.round(settingsBackendRef.setting_font_size || 16.0)
        lineSpacingSpin.value = Math.round((settingsBackendRef.setting_line_spacing || 1.5) * 10)
        autoIndent.checked = settingsBackendRef.setting_auto_indent_enabled
        autoIndentWidthSpin.value = Math.round((settingsBackendRef.setting_auto_indent_width || 2.0) * 2)
        root.textAnimationDurationValue = settingsBackendRef.setting_typing_animation_duration_ms || 100
        _syncingDuration = true
        typingAnimDuration.value = root.textAnimationDurationValue
        coordinatedAnimDuration.value = root.textAnimationDurationValue
        _syncingDuration = false
        smoothCursorDuration.value = settingsBackendRef.setting_smooth_cursor_duration_ms || 80

        var mode = themeControllerRef ? themeControllerRef.appearance_mode : "system"
        themeCombo.currentIndex = mode === "light" ? 1 : (mode === "dark" ? 2 : 0)
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
        updatingValues = false
    }

    // 由 SettingsDialog.onOpened 调用；不重新加载全局设置（启动时已加载）。
    function onDialogOpened() {
        if (syncBackendRef)
            syncBackendRef.load_sync_config()
        updateValues()
    }

    // 由 SettingsDialog.onClosed 调用：强制把 spin 当前值写回，再统一 flush。
    function onDialogClosed() {
        if (settingsBackendRef && !updatingValues) {
            settingsBackendRef.setting_font_size = fontSizeSpin.value
            settingsBackendRef.setting_line_spacing = lineSpacingSpin.value / 10
            settingsBackendRef.setting_auto_indent_width = autoIndentWidthSpin.value / 2
            settingsBackendRef.setting_auto_save_delay_ms = autoSaveDelaySpin.value * 1000
            settingsBackendRef.setting_typing_animation_duration_ms = root.textAnimationDurationValue
            settingsBackendRef.setting_smooth_cursor_duration_ms = smoothCursorDuration.value
            root.settingsDirty = true
        }
        flushSave()
    }

    // Issue #835 评论 6019235318: 外层用普通 Item，宽屏左右摆两列、窄屏上下接，
    // 两组 ColumnLayout 都 visible:true，窄屏不再丢掉右半边设置。
    // 左列：外观/编辑器和动画/AI；右列：保存和同步/诊断与日志/关于。
    Item {
        id: settingsColumns
        width: root.availableWidth
        readonly property real gap: Kirigami.Units.largeSpacing
        readonly property real columnWidth:
            root.widePanel ? (width - gap) / 2 : width
        height: root.widePanel
            ? Math.max(leftSettingsColumn.implicitHeight, rightSettingsColumn.implicitHeight)
            : leftSettingsColumn.implicitHeight + gap + rightSettingsColumn.implicitHeight

        ColumnLayout {
            id: leftSettingsColumn
            width: settingsColumns.columnWidth
            x: 0
            y: 0
            spacing: settingsColumns.gap

            // Issue #825：浮层顶部搜索。纯窄屏路由不加搜索框。
            TextField {
                id: settingsSearchField
                visible: root.overlayPanel
                Layout.fillWidth: true
                placeholderText: qsTr("搜索设置分组")
                onTextChanged: root.searchText = text
            }

            // ── 1. 外观 ──
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                visible: root.sectionVisible(qsTr("外观"))

                FormCard.FormHeader { title: qsTr("外观") }

                FormCard.FormCard {
                    FormCard.FormComboBoxDelegate {
                        id: themeCombo
                        text: qsTr("主题模式")
                        description: qsTr("切换系统、浅色或深色")
                        model: [qsTr("跟随系统"), qsTr("浅色"), qsTr("深色")]
                        onActivated: function(index) {
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues)
                                return
                            themeControllerRef.set_appearance_mode(["system", "light", "dark"][index])
                            root.settingsDirty = true
                            root.saveAndNotify()
                        }
                    }
                    FormCard.FormSpinBoxDelegate {
                        id: fontSizeSpin
                        label: qsTr("字体大小")
                        // range from Core settings_presentation: min=12, max=72, step=1
                        from: 12
                        to: 72
                        stepSize: 1
                        textFromValue: function(value) { return value + " px" }
                        onValueModified: {
                            if (!settingsBackendRef || root.updatingValues)
                                return
                            settingsBackendRef.setting_font_size = value
                            root.debouncedSave()
                        }
                    }
                    FormCard.FormSpinBoxDelegate {
                        id: lineSpacingSpin
                        label: qsTr("行距倍数")
                        // range from Core settings_presentation: min=1.0, max=3.0, step=0.1（以 0.1 为整数刻度）
                        from: 10
                        to: 30
                        stepSize: 1
                        textFromValue: function(value) { return (value / 10).toFixed(1) + "x" }
                        valueFromText: function(text) { return Math.round(parseFloat(text) * 10) }
                        onValueModified: {
                            if (!settingsBackendRef || root.updatingValues)
                                return
                            settingsBackendRef.setting_line_spacing = value / 10
                            root.debouncedSave()
                        }
                    }
                    FormCard.FormComboBoxDelegate {
                        id: colorSourceCombo
                        text: qsTr("颜色来源")
                        description: qsTr("选择素笺默认主题或已保存的设备配色")
                        model: [qsTr("素笺默认"), qsTr("已保存的设备配色")]
                        onActivated: function(index) {
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues)
                                return
                            themeControllerRef.set_color_source(["built_in", "saved_palette"][index])
                            root.settingsDirty = true
                            root.saveAndNotify()
                        }
                    }
                    FormCard.FormComboBoxDelegate {
                        id: builtinThemeCombo
                        visible: themeControllerRef ? themeControllerRef.color_source === "built_in" : false
                        text: qsTr("内置主题")
                        description: qsTr("选择内置主题配色方案")
                        property var _themes: {
                            if (!settingsBackendRef) return []
                            try { return JSON.parse(settingsBackendRef.list_builtin_themes_json()) } catch (e) { return [] }
                        }
                        model: _themes.map(function(t) { return t.name || t.theme_id })
                        onActivated: function(index) {
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues)
                                return
                            var themeId = _themes[index] ? _themes[index].theme_id : ""
                            if (themeId.length > 0) {
                                themeControllerRef.set_selected_builtin_theme_id(themeId)
                                root.settingsDirty = true
                                root.saveAndNotify()
                            }
                        }
                    }
                    FormCard.FormComboBoxDelegate {
                        id: paletteRecordCombo
                        visible: themeControllerRef ? themeControllerRef.color_source === "saved_palette" : false
                        text: qsTr("已保存配色")
                        description: qsTr("选择已保存的设备调色板")
                        property var _records: {
                            if (!settingsBackendRef) return []
                            try { return JSON.parse(settingsBackendRef.list_palette_records_json()) } catch (e) { return [] }
                        }
                        model: _records.map(function(r) {
                            var d = new Date(r.captured_at_ms)
                            return (r.source_platform || "") + " · " + (r.source_device_class || "") + " · " + (r.source_device_id || "") + " · " + d.toLocaleDateString()
                        })
                        onActivated: function(index) {
                            if (!settingsBackendRef || !themeControllerRef || root.updatingValues)
                                return
                            var paletteId = _records[index] ? _records[index].palette_id : ""
                            if (paletteId.length > 0) {
                                themeControllerRef.set_selected_palette_id(paletteId)
                                root.settingsDirty = true
                                root.saveAndNotify()
                            }
                        }
                    }
                }
            }

            // ── 2. 编辑器和动画 ──
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                visible: root.sectionVisible(qsTr("编辑器和动画"))

                FormCard.FormHeader { title: qsTr("编辑器和动画") }

                FormCard.FormCard {
                    FormCard.FormSwitchDelegate {
                        id: autoIndent
                        text: qsTr("自动首行缩进")
                        description: qsTr("回车时自动添加缩进")
                        onToggled: root.setSwitchValue(autoIndent, "setting_auto_indent_enabled", autoIndent.checked)
                    }
                    FormCard.FormSpinBoxDelegate {
                        id: autoIndentWidthSpin
                        label: qsTr("首行缩进宽度")
                        // range from Core settings_presentation: min=0.0, max=8.0, step=0.5（以 0.5 为整数刻度）
                        from: 0
                        to: 16
                        stepSize: 1
                        textFromValue: function(value) { return (value / 2).toFixed(1) + qsTr(" 字符") }
                        valueFromText: function(text) { return Math.round(parseFloat(text) * 2) }
                        onValueModified: {
                            if (!settingsBackendRef || root.updatingValues)
                                return
                            settingsBackendRef.setting_auto_indent_width = value / 2
                            root.debouncedSave()
                        }
                    }
                    // Issue #756 / #785 / #815: 协同动画（吞字/吐字）显式模式开关。
                    FormCard.FormSwitchDelegate {
                        id: coordinatedAnim
                        text: qsTr("协同动画（吞字/吐字）")
                        description: qsTr("完整模式：文字吐吞/位移与光标共用同一条时间线；关闭后恢复下方各自开关之前的设置")
                        onToggled: root.setCoordinatedAnimation(coordinatedAnim.checked)
                    }
                    // Issue #815 评论 5955090551: 协同动画时长 = caret track 时长 = 打字动画时长。
                    FormCard.FormSpinBoxDelegate {
                        id: coordinatedAnimDuration
                        visible: coordinatedAnim.checked
                        label: qsTr("协同动画时长")
                        // 与打字动画时长同一区间（Core settings_presentation: min=30, max=1000, step=10）
                        from: 30
                        to: 1000
                        stepSize: 10
                        textFromValue: function(value) { return value + " ms" }
                        onValueModified: {
                            root.setTextAnimationDuration(value)
                            root.debouncedSave()
                        }
                    }
                    FormCard.FormSwitchDelegate {
                        id: typingAnim
                        visible: !coordinatedAnim.checked
                        text: qsTr("打字动画")
                        description: qsTr("输入时字符从光标处吐出")
                        onToggled: root.setSwitchValue(typingAnim, "setting_typing_animation_enabled", typingAnim.checked)
                    }
                    FormCard.FormSpinBoxDelegate {
                        id: typingAnimDuration
                        visible: !coordinatedAnim.checked
                        label: qsTr("打字动画持续时间")
                        from: 30
                        to: 1000
                        stepSize: 10
                        textFromValue: function(value) { return value + " ms" }
                        onValueModified: {
                            root.setTextAnimationDuration(value)
                            root.debouncedSave()
                        }
                    }
                    FormCard.FormSwitchDelegate {
                        id: smoothCursor
                        visible: !coordinatedAnim.checked
                        text: qsTr("平滑光标")
                        description: qsTr("光标移动更顺滑")
                        onToggled: root.setSwitchValue(smoothCursor, "setting_smooth_cursor_enabled", smoothCursor.checked)
                    }
                    FormCard.FormSpinBoxDelegate {
                        id: smoothCursorDuration
                        visible: !coordinatedAnim.checked
                        label: qsTr("平滑光标持续时间")
                        from: 30
                        to: 1000
                        stepSize: 10
                        textFromValue: function(value) { return value + " ms" }
                        onValueModified: {
                            if (!settingsBackendRef || root.updatingValues)
                                return
                            settingsBackendRef.setting_smooth_cursor_duration_ms = value
                            root.debouncedSave()
                        }
                    }
                }
            }

            // ── 3. AI ──
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                visible: (root.settingsBackendRef ? root.settingsBackendRef.ai_available : false)
                         && root.sectionVisible(qsTr("AI"))

                FormCard.FormHeader { title: qsTr("AI") }

                FormCard.FormCard {
                    FormCard.FormSwitchDelegate {
                        id: aiSwitch
                        text: qsTr("启用 AI 功能")
                        description: qsTr("控制 AI 功能入口显示")
                        onToggled: root.setSwitchValue(aiSwitch, "ai_enabled", aiSwitch.checked)
                    }
                }
            }
        }

        ColumnLayout {
            id: rightSettingsColumn
            width: settingsColumns.columnWidth
            x: root.widePanel ? leftSettingsColumn.width + settingsColumns.gap : 0
            y: root.widePanel ? 0 : leftSettingsColumn.implicitHeight + settingsColumns.gap
            spacing: settingsColumns.gap

            // ── 4. 保存和同步 ──
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                visible: root.sectionVisible(qsTr("保存和同步"))

                FormCard.FormHeader { title: qsTr("保存和同步") }

                FormCard.FormCard {
                    FormCard.FormSwitchDelegate {
                        id: autoSave
                        text: qsTr("自动保存")
                        description: qsTr("编辑时自动保存到本地")
                        onToggled: root.setSwitchValue(autoSave, "setting_auto_save_enabled", autoSave.checked)
                    }
                    // Issue #790 评论 5875963057: 切换工作区入口收口到设置页
                    FormCard.FormButtonDelegate {
                        text: qsTr("切换工作区")
                        description: qsTr("切换到另一个工作区目录")
                        onClicked: root.switchWorkspaceRequested()
                    }
                    FormCard.FormSpinBoxDelegate {
                        id: autoSaveDelaySpin
                        label: qsTr("自动保存延迟")
                        // range from Core settings_presentation: min=1, max=10, step=1
                        from: 1
                        to: 10
                        stepSize: 1
                        textFromValue: function(value) { return value + qsTr(" 秒") }
                        onValueModified: {
                            if (!settingsBackendRef || root.updatingValues)
                                return
                            settingsBackendRef.setting_auto_save_delay_ms = value * 1000
                            root.debouncedSave()
                        }
                    }
                }

                SyncPage {
                    Layout.fillWidth: true
                    syncBackendRef: root.syncBackendRef
                    workspaceBackendRef: root.workspaceBackendRef
                    beforeSyncHook: function() {
                        if (typeof root.beforeSyncHook === "function")
                            return root.beforeSyncHook()
                        return true
                    }
                    onSettingsChanged: root.settingsChanged()
                    onOpenConflict: function(projectId, path) { root.openConflict(projectId, path) }
                }
            }

            // ── 5. 诊断与日志 ──
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                visible: root.sectionVisible(qsTr("诊断与日志"))

                FormCard.FormHeader { title: qsTr("诊断与日志") }

                FormCard.FormCard {
                    FormCard.FormSwitchDelegate {
                        id: diagnosticsEnabled
                        text: qsTr("启用诊断日志")
                        description: qsTr("记录应用运行日志，用于问题排查")
                        onToggled: {
                            root.setSwitchValue(diagnosticsEnabled, "setting_diagnostics_enabled", diagnosticsEnabled.checked)
                            diagnosticsVerbose.enabled = diagnosticsEnabled.checked
                            if (!diagnosticsEnabled.checked) {
                                diagnosticsVerbose.checked = false
                                root.setSwitchValue(diagnosticsVerbose, "setting_diagnostics_verbose", false)
                            }
                        }
                    }
                    FormCard.FormSwitchDelegate {
                        id: diagnosticsVerbose
                        text: qsTr("详细日志")
                        description: qsTr("记录更详细的调试信息")
                        onToggled: root.setSwitchValue(diagnosticsVerbose, "setting_diagnostics_verbose", diagnosticsVerbose.checked)
                    }
                    FormCard.FormButtonDelegate {
                        text: qsTr("清空日志")
                        description: qsTr("删除所有日志文件")
                        onClicked: { if (root.settingsBackendRef) root.settingsBackendRef.clear_logs() }
                    }
                    FormCard.FormButtonDelegate {
                        text: qsTr("导出诊断包")
                        description: qsTr("打包日志与设备信息为 ZIP")
                        onClicked: {
                            if (!root.settingsBackendRef) return
                            var result = root.settingsBackendRef.export_diagnostics_pack()
                            try {
                                var obj = JSON.parse(result)
                                if (obj.success) {
                                    var zipPath = obj.nativeZipPath || obj.zipPath || obj.nativePath || obj.path || ""
                                    var exportDir = obj.nativeExportDir || obj.exportDir || ""
                                    diagnosticsFeedback.message = qsTr("日志 zip: ") + zipPath + "\n" + qsTr("导出目录: ") + exportDir
                                    if (obj.openedExportDir === false && obj.openExportDirError)
                                        diagnosticsFeedback.message += "\n" + qsTr("打开目录失败：") + obj.openExportDirError
                                    diagnosticsFeedback.isError = false
                                    if (obj.openedExportDir === false && obj.exportDirUrl)
                                        Qt.openUrlExternally(obj.exportDirUrl)
                                } else {
                                    diagnosticsFeedback.message = qsTr("导出失败：") + (obj.error || qsTr("未知错误"))
                                    diagnosticsFeedback.isError = true
                                }
                            } catch (e) {
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
                    // 长路径可换行反馈，受卡片宽度约束，不横向撑开设置视图。
                    Label {
                        id: diagnosticsFeedback
                        property string message: ""
                        property bool isError: false
                        visible: message.length > 0
                        text: message
                        color: isError ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
                        wrapMode: Text.WrapAnywhere
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.leftMargin: Kirigami.Units.largeSpacing
                        Layout.rightMargin: Kirigami.Units.largeSpacing
                        Layout.bottomMargin: Kirigami.Units.smallSpacing
                    }
                    FormCard.FormButtonDelegate {
                        text: qsTr("复制设备信息")
                        description: qsTr("将设备信息复制到剪贴板")
                        onClicked: {
                            if (!root.settingsBackendRef) return
                            var info = root.settingsBackendRef.copy_device_info()
                            if (info && info.length > 0) {
                                var result = root.settingsBackendRef.copy_text_to_clipboard(info)
                                try {
                                    var obj = JSON.parse(result)
                                    if (obj.success)
                                        deviceInfoFeedback.message = qsTr("已复制")
                                    else
                                        deviceInfoFeedback.message = qsTr("复制失败：") + (obj.messageKey || obj.rawError || qsTr("未知错误"))
                                } catch (e) {
                                    if (result === "ok" || result.length > 0)
                                        deviceInfoFeedback.message = qsTr("已复制")
                                    else
                                        deviceInfoFeedback.message = qsTr("复制失败")
                                }
                            } else {
                                deviceInfoFeedback.message = qsTr("获取设备信息失败")
                            }
                        }
                    }
                    Label {
                        id: deviceInfoFeedback
                        property string message: ""
                        visible: message.length > 0
                        text: message
                        color: Kirigami.Theme.disabledTextColor
                        wrapMode: Text.WrapAnywhere
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.leftMargin: Kirigami.Units.largeSpacing
                        Layout.rightMargin: Kirigami.Units.largeSpacing
                        Layout.bottomMargin: Kirigami.Units.smallSpacing
                    }
                    FormCard.FormButtonDelegate {
                        text: qsTr("打开日志目录")
                        description: qsTr("在文件管理器中打开日志目录")
                        onClicked: { if (root.settingsBackendRef) root.settingsBackendRef.open_log_directory() }
                    }
                }
            }

            // ── 6. 关于 ──
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                visible: root.sectionVisible(qsTr("关于"))

                FormCard.FormHeader { title: qsTr("关于") }

                FormCard.FormCard {
                    FormCard.FormTextDelegate { text: qsTr("应用名"); description: qsTr("素笺写作") }
                    FormCard.FormTextDelegate { text: qsTr("作者"); description: "Xiwei753" }
                    FormCard.FormTextDelegate { text: qsTr("项目地址"); description: "github.com/Xiwei753/xiezuoruanjian" }
                    FormCard.FormTextDelegate { text: qsTr("开源协议"); description: "GPLv3" }
                    FormCard.FormTextDelegate { text: qsTr("版本信息"); description: qsTr("Linux_qt 客户端") }
                    // 工作区路径可能很长：用只读文本框承载，不横向撑开设置视图。
                    FormCard.FormTextFieldDelegate {
                        label: qsTr("工作区路径")
                        readOnly: true
                        text: root.workspaceBackendRef ? root.workspaceBackendRef.workspace_path : qsTr("未加载")
                    }
                    FormCard.FormButtonDelegate {
                        text: qsTr("动作注册表")
                        description: qsTr("查看已注册的动作")
                        onClicked: { /* Navigate to ActionRegistryPage later */ }
                    }
                }
            }
        }
    }

    // Issue #695 评论 5693346400: 桌面滚轮事件直译器，避免回到 Qt Flickable
    // 自己的鼠标 wheel acceleration。
    DesktopWheelScrollHandler {
        targetFlickable: root.contentItem
        lineSpacingPx: 56
        anchors.fill: root
    }
}
