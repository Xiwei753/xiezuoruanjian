// Windows 设置页：Qt Quick Controls 原生控件，不依赖 KDE Kirigami 页面。
// 通过 QRC alias 覆盖虚拟入口 KdeSettingsPage.qml；保留 Linux 页面同名
// property/signal/function 面，主窗口接线无需分叉。

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: root

    property bool widePanel: true
    property bool overlayPanel: true
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var syncBackendRef: null
    property var editorBackendRef: null
    property var themeControllerRef: null
    property var beforeSyncHook: null
    property string searchText: ""

    signal settingsChanged()
    signal openConflict(string targetKind, string projectId, string path)
    signal switchWorkspaceRequested()

    property bool updatingValues: false
    property bool settingsDirty: false

    property var builtinThemes: {
        if (!settingsBackendRef) return []
        try { return JSON.parse(settingsBackendRef.list_builtin_themes_json()) } catch (e) { return [] }
    }

    contentWidth: availableWidth
    clip: true

    function saveAndNotify() {
        if (settingsBackendRef) {
            settingsBackendRef.save_local_settings()
        }
        settingsDirty = false
        settingsChanged()
    }

    function debouncedSave() {
        settingsDirty = true
        if (settingsBackendRef) {
            settingsBackendRef.debounced_save_local_settings()
        }
    }

    function flushSave() {
        if (settingsBackendRef) {
            settingsBackendRef.flush_pending_settings_save()
        }
    }

    function resetSearch() {
        searchText = ""
    }

    function revealSection(section, settingKey) {
        // 页面为单列滚动视图，直接滚到对应分组。
        const target = section === "about" ? aboutSection
            : section === "diagnostics" ? diagnosticsSection
            : section === "sync" ? syncSection
            : section === "ai" ? aiSection
            : section === "editor" ? editorSection
            : appearanceSection
        if (target) {
            root.contentItem.contentY = Math.max(0, target.y - 12)
        }
    }

    function onDialogOpened() {
        if (syncBackendRef) {
            syncBackendRef.load_sync_config()
        }
    }

    function onDialogClosed() {
        flushSave()
    }

    function setSetting(name, value) {
        if (!settingsBackendRef || updatingValues) return
        settingsBackendRef[name] = value
        debouncedSave()
    }

    ColumnLayout {
        width: root.availableWidth
        spacing: 16

        Label {
            Layout.fillWidth: true
            text: qsTr("设置")
            font.pixelSize: 24
            font.bold: true
        }

        Label {
            Layout.fillWidth: true
            visible: root.settingsDirty
            text: qsTr("修改会稍后自动保存")
            color: palette.mid
        }

        // ── 外观 ──
        Rectangle {
            id: appearanceSection
            Layout.fillWidth: true
            implicitHeight: appearanceLayout.implicitHeight + 24
            radius: 8
            color: palette.alternateBase
            border.color: palette.mid

            ColumnLayout {
                id: appearanceLayout
                anchors.fill: parent
                anchors.margins: 12
                spacing: 10

                Label { text: qsTr("外观"); font.bold: true; font.pixelSize: 16 }

                RowLayout {
                    Layout.fillWidth: true
                    Label { text: qsTr("主题模式"); Layout.fillWidth: true }
                    ComboBox {
                        model: [qsTr("跟随系统"), qsTr("浅色"), qsTr("深色")]
                        currentIndex: {
                            if (!root.themeControllerRef) return 0
                            return Math.max(0, ["system", "light", "dark"].indexOf(root.themeControllerRef.appearance_mode))
                        }
                        onActivated: function(index) {
                            if (!root.themeControllerRef) return
                            root.themeControllerRef.set_appearance_mode(["system", "light", "dark"][index])
                            root.settingsDirty = true
                            root.saveAndNotify()
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    Label { text: qsTr("内置主题"); Layout.fillWidth: true }
                    ComboBox {
                        id: builtinThemeCombo
                        model: root.builtinThemes.map(function(theme) {
                            return theme.name || theme.theme_id
                        })
                        currentIndex: {
                            var selectedId = root.themeControllerRef
                                ? root.themeControllerRef.selected_builtin_theme_id : ""
                            for (var i = 0; i < root.builtinThemes.length; ++i) {
                                if (root.builtinThemes[i].theme_id === selectedId)
                                    return i
                            }
                            return -1
                        }
                        onActivated: function(index) {
                            if (!root.themeControllerRef || root.updatingValues) return
                            var theme = root.builtinThemes[index]
                            if (!theme || !theme.theme_id) return
                            root.themeControllerRef.set_selected_builtin_theme_id(theme.theme_id)
                            root.settingsDirty = true
                            root.saveAndNotify()
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    Label { text: qsTr("工作区"); Layout.fillWidth: true }
                    Label {
                        text: root.workspaceBackendRef ? root.workspaceBackendRef.workspace_path : qsTr("未加载")
                        elide: Text.ElideMiddle
                        Layout.maximumWidth: root.width * 0.45
                        color: palette.mid
                    }
                    Button {
                        text: qsTr("切换工作区")
                        onClicked: root.switchWorkspaceRequested()
                    }
                }
            }
        }

        // ── 编辑器 ──
        Rectangle {
            id: editorSection
            Layout.fillWidth: true
            implicitHeight: editorLayout.implicitHeight + 24
            radius: 8
            color: palette.alternateBase
            border.color: palette.mid

            ColumnLayout {
                id: editorLayout
                anchors.fill: parent
                anchors.margins: 12
                spacing: 10

                Label { text: qsTr("编辑器"); font.bold: true; font.pixelSize: 16 }

                SpinBox {
                    Layout.fillWidth: true
                    from: 12; to: 72; stepSize: 1
                    value: root.settingsBackendRef ? Math.round(root.settingsBackendRef.setting_font_size) : 16
                    textFromValue: function(value) { return value + " px" }
                    onValueModified: root.setSetting("setting_font_size", value)
                }

                SpinBox {
                    Layout.fillWidth: true
                    from: 10; to: 30; stepSize: 1
                    value: root.settingsBackendRef ? Math.round(root.settingsBackendRef.setting_line_spacing * 10) : 15
                    textFromValue: function(value) { return (value / 10).toFixed(1) + "x" }
                    onValueModified: root.setSetting("setting_line_spacing", value / 10)
                }

                Switch {
                    Layout.fillWidth: true
                    text: qsTr("自动缩进")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.setting_auto_indent_enabled : true
                    onToggled: root.setSetting("setting_auto_indent_enabled", checked)
                }

                SpinBox {
                    Layout.fillWidth: true
                    visible: root.settingsBackendRef ? root.settingsBackendRef.setting_auto_indent_enabled : true
                    from: 0; to: 16; stepSize: 1
                    value: root.settingsBackendRef ? Math.round(root.settingsBackendRef.setting_auto_indent_width * 2) : 4
                    textFromValue: function(value) { return (value / 2).toFixed(1) + qsTr(" 字符") }
                    onValueModified: root.setSetting("setting_auto_indent_width", value / 2)
                }

                Switch {
                    id: autoSaveSwitch
                    Layout.fillWidth: true
                    text: qsTr("自动保存")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.setting_auto_save_enabled : true
                    onToggled: root.setSetting("setting_auto_save_enabled", checked)
                }

                SpinBox {
                    Layout.fillWidth: true
                    visible: autoSaveSwitch.checked
                    from: 1; to: 10; stepSize: 1
                    value: root.settingsBackendRef
                        ? Math.max(1, Math.min(10, Math.round(root.settingsBackendRef.setting_auto_save_delay_ms / 1000)))
                        : 5
                    textFromValue: function(value) { return value + qsTr(" 秒") }
                    onValueModified: root.setSetting("setting_auto_save_delay_ms", value * 1000)
                }

                Switch {
                    Layout.fillWidth: true
                    text: qsTr("协同动画（吞字/吐字）")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.setting_coordinated_text_cursor_animation_enabled : true
                    onToggled: root.setSetting("setting_coordinated_text_cursor_animation_enabled", checked)
                }

                Switch {
                    Layout.fillWidth: true
                    text: qsTr("平滑光标")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.setting_smooth_cursor_enabled : true
                    onToggled: root.setSetting("setting_smooth_cursor_enabled", checked)
                }

                SpinBox {
                    Layout.fillWidth: true
                    visible: root.settingsBackendRef ? root.settingsBackendRef.setting_smooth_cursor_enabled : true
                    from: 30; to: 1000; stepSize: 10
                    value: root.settingsBackendRef ? root.settingsBackendRef.setting_smooth_cursor_duration_ms : 120
                    textFromValue: function(value) { return value + " ms" }
                    onValueModified: root.setSetting("setting_smooth_cursor_duration_ms", value)
                }

                Switch {
                    Layout.fillWidth: true
                    text: qsTr("打字动画")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.setting_typing_animation_enabled : true
                    onToggled: root.setSetting("setting_typing_animation_enabled", checked)
                }

                SpinBox {
                    Layout.fillWidth: true
                    visible: root.settingsBackendRef ? root.settingsBackendRef.setting_typing_animation_enabled : true
                    from: 30; to: 1000; stepSize: 10
                    value: root.settingsBackendRef ? root.settingsBackendRef.setting_typing_animation_duration_ms : 160
                    textFromValue: function(value) { return value + " ms" }
                    onValueModified: root.setSetting("setting_typing_animation_duration_ms", value)
                }
            }
        }

        // ── AI ──
        Rectangle {
            id: aiSection
            Layout.fillWidth: true
            visible: root.settingsBackendRef ? root.settingsBackendRef.ai_available : false
            implicitHeight: aiLayout.implicitHeight + 24
            radius: 8
            color: palette.alternateBase
            border.color: palette.mid

            ColumnLayout {
                id: aiLayout
                anchors.fill: parent
                anchors.margins: 12
                spacing: 10
                Label { text: qsTr("AI"); font.bold: true; font.pixelSize: 16 }
                Switch {
                    Layout.fillWidth: true
                    text: qsTr("启用 AI 功能")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.ai_enabled : false
                    onToggled: root.setSetting("ai_enabled", checked)
                }
            }
        }

        // ── 同步 ──
        Rectangle {
            id: syncSection
            Layout.fillWidth: true
            implicitHeight: syncLayout.implicitHeight + 24
            radius: 8
            color: palette.alternateBase
            border.color: palette.mid

            ColumnLayout {
                id: syncLayout
                anchors.fill: parent
                anchors.margins: 12
                spacing: 10
                Label { text: qsTr("同步"); font.bold: true; font.pixelSize: 16 }
                SyncPage {
                    Layout.fillWidth: true
                    syncBackendRef: root.syncBackendRef
                    workspaceBackendRef: root.workspaceBackendRef
                    beforeSyncHook: root.beforeSyncHook
                    onSettingsChanged: root.settingsChanged()
                    onOpenConflict: function(targetKind, projectId, path) {
                        root.openConflict(targetKind, projectId, path)
                    }
                }
            }
        }

        // ── 诊断 ──
        Rectangle {
            id: diagnosticsSection
            Layout.fillWidth: true
            implicitHeight: diagnosticsLayout.implicitHeight + 24
            radius: 8
            color: palette.alternateBase
            border.color: palette.mid

            ColumnLayout {
                id: diagnosticsLayout
                anchors.fill: parent
                anchors.margins: 12
                spacing: 10
                Label { text: qsTr("诊断与日志"); font.bold: true; font.pixelSize: 16 }
                Switch {
                    Layout.fillWidth: true
                    text: qsTr("记录诊断日志")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.setting_diagnostics_enabled : true
                    onToggled: root.setSetting("setting_diagnostics_enabled", checked)
                }
                Switch {
                    Layout.fillWidth: true
                    text: qsTr("详细日志")
                    checked: root.settingsBackendRef ? root.settingsBackendRef.setting_diagnostics_verbose : true
                    onToggled: root.setSetting("setting_diagnostics_verbose", checked)
                }
                RowLayout {
                    Layout.fillWidth: true
                    Button {
                        text: qsTr("导出诊断")
                        onClicked: { if (root.settingsBackendRef) root.settingsBackendRef.export_diagnostics_pack() }
                    }
                    Button {
                        text: qsTr("打开日志目录")
                        onClicked: { if (root.settingsBackendRef) root.settingsBackendRef.open_log_directory() }
                    }
                    Button {
                        text: qsTr("清理日志")
                        onClicked: { if (root.settingsBackendRef) root.settingsBackendRef.clear_logs() }
                    }
                }
            }
        }

        // ── 关于 ──
        Rectangle {
            id: aboutSection
            Layout.fillWidth: true
            implicitHeight: aboutLayout.implicitHeight + 24
            radius: 8
            color: palette.alternateBase
            border.color: palette.mid

            ColumnLayout {
                id: aboutLayout
                anchors.fill: parent
                anchors.margins: 12
                spacing: 6
                Label { text: qsTr("关于"); font.bold: true; font.pixelSize: 16 }
                Label { text: qsTr("素笺写作 · Windows Qt 客户端"); }
                Label { text: qsTr("作者：Xiwei753"); color: palette.mid }
                Label { text: "github.com/Xiwei753/xiezuoruanjian"; color: palette.mid }
                Label { text: qsTr("开源协议：GPLv3"); color: palette.mid }
            }
        }

        Item { Layout.fillHeight: true }
    }
}
