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
    // Issue #825：设置一直是 Dialog + Overlay.overlay，悬浮在打开设置前的那一页之上，
    // 不改成路由页。面板尺寸分两档：
    // - widePanel（primaryNavigationPlacement === Side，>=840vp）：大号面板，顶部搜索 + 两列分组。
    // - 其余（含 600–839vp 那档 Workbench 但一级导航仍在 Bottom）：单列分组，
    //   宽度是「不超过 640 的窄对话框」再按窗口收窄，不再拆两列。
    // 两档判定都直接读 Core 下发的 primaryNavigationPlacement，平台不自己猜宽度。
    readonly property bool widePanel: layoutPlan === null || layoutPlan.primaryNavigationPlacement === "Side"
    readonly property bool overlayPanel: layoutPlan === null || layoutPlan.workspaceLayoutMode === "Workbench"
    readonly property int widePanelMaxWidth: 1120
    // Issue #825 复核第4项：Workbench 从 600vp 宽就成立，之前 Math.max(720, ...) 的下限
    // 会让 700vp 窗口里弹出比窗口还宽的面板。宽度一律先减掉两侧留白再夹上限。
    width: root.widePanel
           ? Math.max(400, Math.min(root.widePanelMaxWidth, (parent ? parent.width : 1120) - dt.sp64))
           : Math.min(640, (parent ? parent.width : 640) - dt.sp64)
    // 复核第3项：非 Side 档不做两列大面板，高度回到窄对话框那一套。
    // Issue #833：Dialog 高度不再跟 settingsScroll.contentHeight 动态绑定。
    // 高度只由窗口可用高度决定，展开内容在 ScrollView 内滚动。
    // 否则一展开分组，整个 Dialog 自己会变高并重新居中，看起来就是"界面乱动"。
    // Issue #833 复核：内容已放进 ScrollView，Dialog 不需要 480 的硬下限。
    // 主窗口允许缩到 240 高，若仍强制 480，高度会超过 Overlay，居中后顶部跑到负坐标。
    // 高度只夹"窗口可用高度"和"面板最大高度"，最小高度不能再反过来超过 parent。
    height: Math.max(1, Math.min(root.widePanel ? 880 : 720, (parent ? parent.height : 800) - dt.sp32))
    parent: Overlay.overlay
    x: Math.round((parent.width - width) / 2)
    y: Math.round((parent.height - height) / 2)
    property var layoutPlan: null
    // 宽屏顶部搜索关键词。只做分组过滤，不改任何设置读取/保存路径。
    property string searchText: ""
    // Issue #842：多分组同时展开。默认只展开"外观"，其余按需独立切换。
    property var expandedSections: ({ "外观": true })
    function sectionExpanded(key) {
        return !!root.expandedSections[key]
    }
    function toggleSection(key) {
        var next = {}
        for (var k in root.expandedSections) {
            next[k] = root.expandedSections[k]
        }
        next[key] = !root.sectionExpanded(key)
        root.expandedSections = next
    }
    property var theme: null
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var themeControllerRef: null
    property var beforeSyncHook: null
    property var dt: theme
    property bool updatingValues: false
    // Issue #815 评论 5955676896: 「打字动画持续时间」和「协同动画时长」是同一个设置项
    // `setting_typing_animation_duration_ms`。它们必须只有**一份**状态：
    // #815 之后协同吞吐字是 CaretTrack，没有自己的 duration，整段协同动画的速度
    // 就是这条 track 的时长，而 pipeline.rs 传的就是打字动画时长。两个滑块各持一份
    // 旧值会在切协同开关时被 onClosed 用另一个的旧值覆盖回去。
    property real textAnimationDurationValue: 100
    property bool settingsDirty: false
    property var _saveTimer: null
    signal settingsChanged()
    // Issue #762 评论 5826175490 第 4 点：转发 SyncPage 的 openConflict 信号给主窗口
    signal openConflict(string projectId, string path)
    // Issue #790 评论 5875963057: 切换工作区入口收口到设置页
    signal switchWorkspaceRequested()

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
    // Issue #825：宽屏顶部搜索按分组标题过滤当前浮层里的设置分组。
    // 空搜索词时全部显示；命中与否只影响可见性，不动 backend。
    function sectionVisible(title) {
        var q = root.searchText.trim().toLowerCase()
        if (q.length === 0) return true
        return String(title).toLowerCase().indexOf(q) !== -1
    }
    function setSwitchValue(control, key, value) {
        control.checked = value
        if (!settingsBackendRef || updatingValues) return
        settingsBackendRef[key] = value
        root.settingsDirty = true
        debouncedSave()
    }
    // Issue #815 评论 5955676896: 协同动画开关。协同开时，同一笔 cursor track 同时
    // 驱动文字吞吐与光标位移，**没有两条独立 duration**——协同动画时长就是
    // 「打字动画持续时间」这一个设置项（下面的 textAnimationDurationValue）。
    // 协同关时，文字与光标才各自走独立 duration。
    function setCoordinatedAnimation(value) {
        coordinatedAnim.checked = value
        if (!settingsBackendRef || updatingValues) return
        settingsBackendRef.setting_coordinated_text_cursor_animation_enabled = value
        root.settingsDirty = true
        debouncedSave()
    }

    // Issue #815 评论 5955676896: 打字动画时长 / 协同动画时长的**唯一**写入口。
    // 两个滑块都调它，backend 与另一个滑块同时同步，避免两份状态互相回滚。
    function setTextAnimationDuration(value) {
        if (updatingValues) return
        root.textAnimationDurationValue = value
        if (coordinatedAnimDuration.value !== value) coordinatedAnimDuration.value = value
        if (typingAnimDuration.value !== value) typingAnimDuration.value = value
        if (!settingsBackendRef) return
        settingsBackendRef.setting_typing_animation_duration_ms = value
        root.settingsDirty = true
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
        // Issue #815 评论 5955090551 + 5955676896: 协同动画时长与打字动画时长是同
        // 一个设置项，两个滑块读同一份共享值，而不是各存一份。
        root.textAnimationDurationValue = settingsBackendRef.setting_typing_animation_duration_ms || 100
        typingAnimDuration.value = root.textAnimationDurationValue
        coordinatedAnimDuration.value = root.textAnimationDurationValue
        smoothCursorDuration.value = settingsBackendRef.setting_smooth_cursor_duration_ms || 80
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
        // Issue #825：每次打开都清掉宽屏搜索词，避免上一次过滤结果留在浮层里。
        root.searchText = ""
        updateValues()
    }
    onClosed: {
        // Force-write all slider current values back to settingsBackendRef before saving
        if (settingsBackendRef && !root.updatingValues) {
            settingsBackendRef.setting_font_size = fontSizeSlider.value
            settingsBackendRef.setting_line_spacing = lineSpacingSlider.value
            settingsBackendRef.setting_auto_indent_width = autoIndentWidth.value
        settingsBackendRef.setting_auto_save_delay_ms = autoSaveDelay.value * 1000
        // Issue #785: 始终分别写两个独立 duration，不再因协同共享。
        // Issue #815 评论 5955090551: 打字动画时长同时就是协同动画时长。
        // Issue #815 评论 5955676896: 只写这一份共享值，不再根据协同开关从两个滑块
        // 二选一——那会让隐藏滑块的旧值在切开关后覆盖掉刚调好的新值。
        settingsBackendRef.setting_typing_animation_duration_ms = root.textAnimationDurationValue
        settingsBackendRef.setting_smooth_cursor_duration_ms = smoothCursorDuration.value
        root.settingsDirty = true
        }
        flushSave()
    }

    Rectangle {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: root.overlayPanel ? 72 : 64
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt.sp24
            anchors.rightMargin: dt.sp16
            spacing: dt.sp16
            AppText {
                dt: root.dt
                text: qsTr("设置")
                color: dt.textPrimary
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.Bold
                Layout.fillWidth: !root.overlayPanel
                Layout.leftMargin: root.overlayPanel ? dt.sp8 : 0
            }
            // Issue #825：浮层（Workbench）顶部搜索。600–839vp 那档虽然不拆两列，
            // 但仍是浮层，保留搜索。纯窄屏路由保持原来的窄对话框布局，不加搜索框。
            AppTextField {
                id: settingsSearchField
                dt: root.dt
                visible: root.overlayPanel
                Layout.fillWidth: true
                Layout.maximumWidth: 420
                Layout.alignment: Qt.AlignVCenter
                placeholder: qsTr("搜索设置分组")
                onTextChanged: root.searchText = text
            }
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
        // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面，
        // 保留滚轮/触摸板/滚动条/触屏滚动。
        Component.onCompleted: {
            if (contentItem) contentItem.acceptedButtons = Qt.NoButton
        }
        contentWidth: availableWidth
        contentHeight: settingsColumns.height

        // Issue #835 评论 6019235318: 外层用普通 Item，宽屏左右摆两列、窄屏上下接，
        // 两组 ColumnLayout 都 visible:true，窄屏不再丢掉右半边设置。
        // 不用 RowLayout+visible:false（窄屏会把右列三组设置删没），也不用旧
        // GridLayout columns=2（左右卡片互相拉高）。左列：外观/编辑器和动画/AI；
        // 右列：保存和同步/诊断与日志/关于。每个 SettingsSection 根据
        // expandedSections 设置 expanded，toggleRequested 时独立切换该分组展开状态。
        Item {
            id: settingsColumns
            width: settingsScroll.availableWidth
            readonly property real gap: root.dt.cardGap
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
                spacing: root.dt.cardGap

                // ── 1. 外观 (appearance) ──
                SettingsSection {
                    dt: root.dt
                    title: qsTr("外观")
                    visible: root.sectionVisible(qsTr("外观"))
                    expanded: root.sectionExpanded(qsTr("外观"))
                    onToggleRequested: root.toggleSection(qsTr("外观"))
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
                    visible: root.sectionVisible(qsTr("编辑器和动画"))
                    expanded: root.sectionExpanded(qsTr("编辑器和动画"))
                    onToggleRequested: root.toggleSection(qsTr("编辑器和动画"))
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
                    // Issue #756 / Issue #785: 协同动画（吞字/吐字）显式模式开关。
                    // Issue #815 评论 5955090551: true 时文字与光标是**同一条** caret track，
                    // 吞吐字（CaretTrack）自己没有时长，整段协同动画速度就是这条 track 的
                    // 时长，所以由下面的「协同动画时长」统一控制（绑定打字动画时长）。
                    // false 时 typing/smooth 两个独立开关与各自时长仍然分开。
                    SettingsRow {
                        dt: root.dt
                        title: qsTr("协同动画（吞字/吐字）")
                        description: qsTr("文字与光标同事务协同，速度统一见下方「协同动画时长」")
                        clickable: true
                        onClicked: root.setCoordinatedAnimation(!coordinatedAnim.checked)
                        ModernSwitch { id: coordinatedAnim; dt: root.dt; onToggled: function(v) { root.setCoordinatedAnimation(v) } }
                    }
                    // Issue #815 评论 5955090551: 协同动画时长。
                    //
                    // #815 之后协同模式下 InsertReveal / DeleteConceal 是
                    // `VisualUnitTiming::CaretTrack`，自己**没有时长**，逐帧边界完全跟随
                    // 同一笔 cursor track。也就是说：协同动画速度 = 这条 track 的时长。
                    //
                    // 而这条 track 的时长在 pipeline.rs 里取的是「打字动画时长」，所以这个
                    // 滑块直接绑定 setting_typing_animation_duration_ms。绝不能绑定被隐藏的
                    // setting_smooth_cursor_duration_ms——那正是实机上「动画快得看不见」的根因。
                    //
                    // 协同关闭时不显示，此时文字与光标各自独立时长（下面的两个滑块）。
                    AppSlider {
                        id: coordinatedAnimDuration
                        Layout.fillWidth: true
                        dt: root.dt
                        visible: coordinatedAnim.checked
                        label: qsTr("协同动画时长")
                        valueText: Math.round(value) + " ms"
                        // 与打字动画时长同一个区间（Core settings_presentation: min=30, max=1000, step=10）
                        from: 30
                        to: 1000
                        stepSize: 10
                        onMoved: function() { root.setTextAnimationDuration(value) }
                        onCommitted: function() { root.setTextAnimationDuration(value); root.debouncedSave() }
                    }
                    // Issue #808: 协同开启时整组隐藏（开关 + duration 滑块一起消失）。
                    // Issue #815 评论 5955676896: 打字动画时长与上面的协同动画时长是同一个
                    // 设置项，共用 root.textAnimationDurationValue，不各存一份。
                    SettingsRow {
                        visible: !coordinatedAnim.checked
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
                        // Issue #808: 协同开启时整组隐藏，duration 不再改名兜底。
                        visible: !coordinatedAnim.checked
                        label: qsTr("打字动画持续时间")
                        valueText: Math.round(value) + " ms"
                        // range from Core settings_presentation: min=30, max=1000, step=10
                        from: 30
                        to: 1000
                        stepSize: 10
                        onMoved: function() { root.setTextAnimationDuration(value) }
                        onCommitted: function() { root.setTextAnimationDuration(value); root.debouncedSave() }
                    }
                    SettingsRow {
                        visible: !coordinatedAnim.checked
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
                        // Issue #808: 协同开启时整组隐藏，duration 不再改名兜底。
                        visible: !coordinatedAnim.checked
                        label: qsTr("平滑光标持续时间")
                        valueText: Math.round(value) + " ms"
                        // range from Core settings_presentation: min=30, max=1000, step=10
                        from: 30
                        to: 1000
                        stepSize: 10
                        onMoved: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_smooth_cursor_duration_ms = value }
                        onCommitted: function() { if (!settingsBackendRef || root.updatingValues) return; settingsBackendRef.setting_smooth_cursor_duration_ms = value; root.debouncedSave() }
                    }
                }

                // ── 4. AI (ai) ──
                SettingsSection {
                    dt: root.dt
                    title: qsTr("AI")
                    Layout.fillWidth: true
                    expanded: root.sectionExpanded(qsTr("AI"))
                    onToggleRequested: root.toggleSection(qsTr("AI"))
                    visible: (root.settingsBackendRef ? root.settingsBackendRef.ai_available : false)
                             && root.sectionVisible(qsTr("AI"))
                    SettingsRow {
                        dt: root.dt
                        title: qsTr("启用 AI 功能")
                        description: qsTr("控制 AI 功能入口显示")
                        clickable: true
                        onClicked: root.setSwitchValue(aiSwitch, "ai_enabled", !aiSwitch.checked)
                        ModernSwitch { id: aiSwitch; dt: root.dt; onToggled: function(v) { root.setSwitchValue(aiSwitch, "ai_enabled", v) } }
                    }
                }
            }

            ColumnLayout {
                id: rightSettingsColumn
                width: settingsColumns.columnWidth
                x: root.widePanel ? leftSettingsColumn.width + settingsColumns.gap : 0
                y: root.widePanel ? 0 : leftSettingsColumn.implicitHeight + settingsColumns.gap
                visible: true
                spacing: root.dt.cardGap

                // ── 3. 保存和同步 (save + sync) ──
                SettingsSection {
                    dt: root.dt
                    title: qsTr("保存和同步")
                    visible: root.sectionVisible(qsTr("保存和同步"))
                    expanded: root.sectionExpanded(qsTr("保存和同步"))
                    onToggleRequested: root.toggleSection(qsTr("保存和同步"))
                    Layout.fillWidth: true
                    SettingsRow {
                        dt: root.dt
                        title: qsTr("自动保存")
                        description: qsTr("编辑时自动保存到本地")
                        clickable: true
                        onClicked: root.setSwitchValue(autoSave, "setting_auto_save_enabled", !autoSave.checked)
                        ModernSwitch { id: autoSave; dt: root.dt; onToggled: function(v) { root.setSwitchValue(autoSave, "setting_auto_save_enabled", v) } }
                    }
                    // Issue #790 评论 5875963057: 切换工作区入口收口到设置页
                    SettingsRow {
                        dt: root.dt
                        title: qsTr("切换工作区")
                        description: qsTr("切换到另一个工作区目录")
                        clickable: true
                        onClicked: root.switchWorkspaceRequested()
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

                // ── 5. 诊断与日志 (diagnostics) ──
                SettingsSection {
                    dt: root.dt
                    title: qsTr("诊断与日志")
                    visible: root.sectionVisible(qsTr("诊断与日志"))
                    expanded: root.sectionExpanded(qsTr("诊断与日志"))
                    onToggleRequested: root.toggleSection(qsTr("诊断与日志"))
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
                    visible: root.sectionVisible(qsTr("关于"))
                    expanded: root.sectionExpanded(qsTr("关于"))
                    onToggleRequested: root.toggleSection(qsTr("关于"))
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
