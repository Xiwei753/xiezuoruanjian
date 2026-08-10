// =============================================================================
// main.qml — 应用入口窗口
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：应用主窗口、全局路由管理、初始化 AppBackend、全局错误处理
// 约束：
//   - 不包含任何业务逻辑，所有操作委托给 AppBackend (Rust QObject)
//   - 页面切换通过 appController.route 状态驱动
//   - 不直接操作文件系统或 Core 层
//
// 调用链：main.qml → AppBackend (Rust QObject) → WriterCore (Core 层)
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import QtQuick.Window
import Sujian 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 1100
    height: 768
    title: qsTr("素笺写作")
    color: designTokens.bg

    // ── Material 主题绑定：确保 Qt 原生控件（Dialog/Popup/Menu/TextField 等）跟随深浅色 ──
    Material.theme: designTokens.isDark ? Material.Dark : Material.Light
    Material.primary: designTokens.primary
    Material.accent: designTokens.primary
    Material.foreground: designTokens.textPrimary
    Material.background: designTokens.bg

    // ── Palette 绑定：保留作为非 Material 控件的 fallback ──
    palette.window: designTokens.bg
    palette.windowText: designTokens.textPrimary
    palette.base: designTokens.surfaceContainerLow
    palette.text: designTokens.textPrimary
    palette.button: designTokens.surfaceContainer
    palette.buttonText: designTokens.textPrimary
    palette.highlight: designTokens.primary
    palette.highlightedText: designTokens.onPrimary

    function reportNullBackend(name) {
        var message = "Required QML context property is null: " + name;
        console.error(message);
        if (backend !== null && backend.debug_qml_enabled) {
            backend.log_qml("error", "app", "null_context_property", message);
        }
    }

    function verifyBackendRuntime() {
        if (appBackend === null) reportNullBackend("appBackend");
        if (workspaceBackend === null) reportNullBackend("workspaceBackend");
        if (projectBackend === null) reportNullBackend("projectBackend");
        if (editorBackend === null) reportNullBackend("editorBackend");
        if (settingsBackend === null) reportNullBackend("settingsBackend");
        if (syncBackend === null) reportNullBackend("syncBackend");
        if (starmapBackend === null) reportNullBackend("starmapBackend");
    }

    function debugLog(module, event, message) {
        if (backend !== null && backend.debug_qml_enabled) {
            backend.log_qml("info", module, event, message);
        }
    }

    function debugWarn(module, event, message) {
        if (backend !== null && backend.debug_qml_enabled) {
            backend.log_qml("warn", module, event, message);
        }
    }

    function debugError(module, event, message) {
        if (backend !== null && backend.debug_qml_enabled) {
            backend.log_qml("error", module, event, message);
        }
    }

    function preSyncBarrier() {
        if (appController.inWriting && writingWorkspaceLoader.item) {
            if (!writingWorkspaceLoader.item.flushActiveEditorBeforeSync()) return false
        }
        if (editorBackend) {
            editorBackend.flush_writing_stats()
            editorBackend.flush_recent_edits()
        }
        if (settingsBackend) {
            settingsBackend.flush_pending_settings_save()
        }
        return true
    }

    function openSettingsDialog() {
        if (!settingsDialogLoader.active) {
            settingsDialogLoader.active = true;
        }
        if (settingsDialogLoader.item) {
            settingsDialogLoader.item.open();
        }
    }

    function openSyncDialog() {
        if (syncBackend) {
            settingsBackend.load_local_settings();
        }
        window.openSettingsDialog();
    }


    property alias appState: appController.appState
    readonly property bool rootHasWorkspace: workspaceBackend !== null && workspaceBackend.has_workspace === true

    property string previousEditorText: ""

    // ── 布局契约驱动（#610）：Qt 侧按本平台窗口系统算能力，再套 Core 契约 ──
    property var layoutPlan: null

    function applyLayoutPlan() {
        if (appBackend === null) return;
        var w = window.width;
        var h = window.height;
        var plan = appBackend.resolve_layout(w, h);
        if (plan) {
            window.layoutPlan = plan;
        }
    }

    onWidthChanged: window.applyLayoutPlan()
    onHeightChanged: window.applyLayoutPlan()

    SystemPalette {
        id: systemPalette
        colorGroup: SystemPalette.Active
    }

    function colorLuminance(colorValue) {
        return 0.2126 * colorValue.r + 0.7152 * colorValue.g + 0.0722 * colorValue.b;
    }

    function systemPaletteIsDark() {
        return colorLuminance(systemPalette.window) < colorLuminance(systemPalette.windowText);
    }

    function systemThemeIsDark() {
        // Try Qt.styleHints (Qt 6.5+)
        if (typeof Qt !== "undefined" && Qt.styleHints && typeof Qt.styleHints.colorScheme !== "undefined") {
            var scheme = Qt.styleHints.colorScheme;
            if (scheme === 2 || (Qt.ColorScheme && scheme === Qt.ColorScheme.Dark)) {
                return true;
            }
            if (scheme === 1 || (Qt.ColorScheme && scheme === Qt.ColorScheme.Light)) {
                return false;
            }
        }
        // Try Qt.application.styleHints
        if (typeof Qt !== "undefined" && Qt.application && Qt.application.styleHints && typeof Qt.application.styleHints.colorScheme !== "undefined") {
            var schemeApp = Qt.application.styleHints.colorScheme;
            if (schemeApp === 2 || (Qt.ColorScheme && schemeApp === Qt.ColorScheme.Dark)) {
                return true;
            }
            if (schemeApp === 1 || (Qt.ColorScheme && schemeApp === Qt.ColorScheme.Light)) {
                return false;
            }
        }
        // Fallback to SystemPalette brightness inference
        return systemPaletteIsDark();
    }

    function logThemeDiagnostics(event) {
        if (backend === null || !backend.log_qml) return;
        var schemeStr = "<unknown>";
        if (typeof Qt !== "undefined" && Qt.styleHints && typeof Qt.styleHints.colorScheme !== "undefined") {
            schemeStr = Qt.styleHints.colorScheme;
        } else if (typeof Qt !== "undefined" && Qt.application && Qt.application.styleHints && typeof Qt.application.styleHints.colorScheme !== "undefined") {
            schemeStr = Qt.application.styleHints.colorScheme;
        }
        backend.log_qml("info", "theme", event,
                        "themeMode=" + (appState.settings ? appState.settings.themeMode : "<unset>")
                        + " colorScheme=" + schemeStr
                        + " systemPaletteIsDark=" + window.systemPaletteIsDark()
                        + " isDark=" + designTokens.isDark
                        + " textPrimary=" + designTokens.textPrimary
                        + " textSecondary=" + designTokens.textSecondary
                        + " editorText=" + designTokens.editorText);
    }

    // Design tokens
    DesignTokens {
        id: designTokens
        isDark: {
            var mode = settingsBackend !== null ? settingsBackend.resolved_appearance_mode : "system"
            if (mode === "dark") return true;
            if (mode === "light") return false;
            return window.systemThemeIsDark();
        }
        themePaletteJson: settingsBackend !== null ? settingsBackend.resolved_theme_palette_json : ""
        colorSource: settingsBackend !== null ? settingsBackend.resolved_color_source : "built_in"
        selectedBuiltinThemeId: settingsBackend !== null ? settingsBackend.setting_selected_builtin_theme_id : ""
        builtinThemesJson: settingsBackend !== null ? settingsBackend.resolved_builtin_themes_json : "[]"
        resolvedSchemeJson: themeController !== null ? themeController.resolved_scheme_json : ""
    }

    Connections {
        target: designTokens
        function onIsDarkChanged() {
            window.logThemeDiagnostics("is_dark_changed");
            if (appBackend) {
                appBackend.apply_window_dark_mode(designTokens.isDark);
            }
            if (themeController) {
                themeController.set_system_is_dark(designTokens.isDark);
            }
        }
        function onEditorTextChanged() {
            window.logThemeDiagnostics("editor_text_changed");
        }
    }

    Connections {
        target: syncBackend
        function onSync_action_completed() {
            if (settingsBackend) settingsBackend.refresh_theme_data()
            if (themeController) themeController.reload()
        }
    }

    AppController {
        id: appController
        backendRef: backend
        workspaceBackendRef: workspaceBackend
        stateBackendRef: projectBackend
        appBackendRef: backend
        onErrorRaised: function(message) {
            errorDialog.message = message;
            errorDialog.open();
        }
    }

    SujianEditorItem {
        id: globalTextCoordinator
        x: 0
        y: 0
        width: 0
        height: 0
        visible: false
        editor_enabled: false
    }

    ProjectController {
        id: projectController
        backendRef: backend
        projectBackendRef: projectBackend
        appController: appController
    }

    StarMapController {
        id: globalStarMapController
        backendRef: backend
        starmapBackendRef: starmapBackend
        appController: appController
    }

    Component.onCompleted: {
        // 启动守卫：确保 DesignTokens 已初始化
        if (!designTokens) {
            console.error("[STARTUP GUARD] designTokens is null!")
        }
        window.verifyBackendRuntime();
        window.debugLog("app", "qml_completed", "QML components fully loaded");
        window.logThemeDiagnostics("startup");
        if (appBackend) {
            appBackend.apply_window_dark_mode(designTokens.isDark);
        }
        appController.restoreWorkspace();
    }

    onActiveChanged: {
        if (active && syncBackend) {
            foregroundAutoSyncTimer.restart();
        }
    }

    onClosing: {
        if (appController.inWriting) {
            editorBackend.flush_writing_stats();
            editorBackend.flush_recent_edits();
        }
        // 应用关闭前 flush pending settings save
        if (settingsBackend) {
            settingsBackend.flush_pending_settings_save();
        }
    }

    // ── Debounced settings save Timer ──
    // settingsBackend.save_requested 信号触发后，延迟 300ms 执行实际保存
    Timer {
        id: settingsDebounceSaveTimer
        interval: 300
        repeat: false
        onTriggered: {
            if (settingsBackend) {
                settingsBackend.do_save_local_settings();
            }
        }
    }

    Connections {
        target: settingsBackend
        function onSave_requested() {
            settingsDebounceSaveTimer.restart();
        }
    }

    Timer {
        id: workspaceOpenAutoSyncTimer
        interval: 1500
        repeat: false
        onTriggered: {
            if (syncBackend) {
                if (!window.preSyncBarrier()) return;
                syncBackend.request_auto_sync("auto_sync_on_workspace_open");
            }
        }
    }

    Timer {
        id: foregroundAutoSyncTimer
        interval: 1200
        repeat: false
        onTriggered: {
            if (syncBackend) {
                if (!window.preSyncBarrier()) return;
                syncBackend.maybe_auto_sync_on_foreground();
            }
        }
    }

    function applyState(state) {
        appController.applyState(state);
    }

    Connections {
        target: projectBackend
        function onProjects_reloaded() {
            appController.refreshState(qsTr("刷新作品列表失败"));
        }
    }

    Connections {
        target: workspaceBackend
        function onWorkspace_state_changed() {
            appController.refreshState(qsTr("刷新工作区状态失败"));
        }
        function onWorkspace_opened() {
            if (settingsBackend) {
                settingsBackend.load_local_settings();
            }
            if (syncBackend) {
                syncBackend.load_sync_config();
            }
            workspaceOpenAutoSyncTimer.restart();
        }
        function onWorkspace_content_changed() {
            appController.refreshState(qsTr("刷新工作区内容失败"));
        }
    }

    Connections {
        target: appBackend
        function onWorkspace_content_changed() {
            appController.refreshState(qsTr("刷新工作区内容失败"));
            if (appController.inWriting && writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.reloadActiveChapter();
            }
        }
        function onWorkspace_state_changed() {
            appController.refreshState(qsTr("刷新工作区状态失败"));
        }
    }

    Connections {
        target: editorBackend
        function onClear_editor() {
            if (writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.previousEditorText = "";
            }
        }
    }

    Connections {
        target: settingsBackend
        function onSettings_changed() {
            appController.refreshState(qsTr("刷新设置失败"));
        }
    }

    // === Main Content ===
    Item {
        anchors.fill: parent

        // StarMapWorkspace: shown when in starmap editor mode
        Loader {
            id: starmapWorkspaceLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.inStarmap
            sourceComponent: StarMapWorkspace {
                dt: designTokens
                backendRef: starmapBackend
                starmapId: appController.starmapId
                starmapTitle: appController.starmapTitle
                onBackClicked: {
                    appController.openHub();
                }
            }
        }

        // CreativeHub: shown when workspace open and not in writing mode
        Loader {
            id: creativeHubLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.route === "hub"
            sourceComponent: CreativeHub {
                dt: designTokens
                backendRef: projectBackend
                projectBackendRef: projectBackend
            editorBackendRef: editorBackend
                starmapBackendRef: starmapBackend
                textCoordinator: globalTextCoordinator
                starMapController: globalStarMapController
                appState: window.appState
                tree: window.appState.tree || []
                aiCapable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled
                layoutPlan: window.layoutPlan

                onOpenStarmapWorkspace: function(smId, smTitle) {
                    appController.openStarmap(smId, smTitle);
                }

                onOpenProject: function(projectId, projectTitle) {
                    appController.openWriting(projectId, projectTitle);
                    window.debugLog("workspace", "enter_writing_mode", "projectId=" + projectId);
                }

                onCreateProject: {
                    window.debugLog("project", "create_project_dialog_open", "");
                    createProjectDialog.open();
                }

                onRenameProjectRequested: function(projectId, title) {
                    projectController.renameProject(projectId, title);
                }

                onDeleteProjectRequested: function(projectId, title) {
                    confirmDialog.actionType = "delete_project";
                    confirmDialog.contextData = { projectId: projectId, title: title };
                    confirmDialog.open();
                }

                onOpenSettings: {
                    window.debugLog("settings", "settings_dialog_open", "");
                    window.openSettingsDialog();
                }

                onRequestSync: {
                    if (!window.preSyncBarrier()) return;
                    if (syncBackend && !syncBackend.sync_in_progress) {
                        syncBackend.perform_sync();
                    }
                }

                onSwitchWorkspace: {
                    window.debugLog("workspace", "switch_workspace_clicked", "");
                    appController.switchWorkspace();
                }
            }
        }

        // WritingWorkspace: shown when in writing mode
        Loader {
            id: writingWorkspaceLoader
            anchors.fill: parent
            active: rootHasWorkspace && appController.inWriting
            sourceComponent: WritingWorkspace {
                dt: designTokens
                backendRef: editorBackend
                starMapController: globalStarMapController
                appState: window.appState
                tree: window.appState.tree || []
                workspaceProjectId: appController.writingProjectId
                projectTitle: appController.writingProjectTitle
                aiCapable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled
                layoutPlan: window.layoutPlan

                onBackToProjects: {
                    appController.openHub();
                    window.debugLog("workspace", "exit_writing_mode", "");
                }

                onOpenSettings: {
                    window.openSettingsDialog();
                }


                onCreateVolumeRequested: function(projectId) {
                    inputDialog.actionType = "volume";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = "";
                    inputDialog.dialogTitle = qsTr("新建卷");
                    inputDialog.defaultText = "";
                    inputDialog.open();
                }

                onCreateChapterRequested: function(projectId, volumeId) {
                    inputDialog.actionType = "chapter";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = volumeId;
                    inputDialog.dialogTitle = qsTr("新建章节");
                    // 从树数据中统计当前卷的章节数量，生成默认标题
                    var chapterCount = 0;
                    var treeData = window.appState ? (window.appState.tree || []) : [];
                    for (var i = 0; i < treeData.length; i++) {
                        if (treeData[i].type === "chapter" && treeData[i].volumeId === volumeId) {
                            chapterCount++;
                        }
                    }
                    inputDialog.defaultText = qsTr("第%1章").arg(chapterCount + 1);
                    inputDialog.open();
                }

                onRenameItemRequested: function(itemData) {
                    inputDialog.actionType = "rename_" + itemData.type;
                    inputDialog.projectId = itemData.projectId || "";
                    inputDialog.volumeId = itemData.volumeId || "";
                    inputDialog.chapterId = itemData.id || "";
                    inputDialog.dialogTitle = qsTr("重命名");
                    inputDialog.defaultText = itemData.title || "";
                    inputDialog.open();
                }

                onDeleteItemRequested: function(itemData) {
                    confirmDialog.actionType = "delete_" + itemData.type;
                    confirmDialog.contextData = {
                        projectId: itemData.projectId || "",
                        volumeId: itemData.volumeId || "",
                        chapterId: itemData.id || "",
                        title: itemData.title || ""
                    };
                    confirmDialog.open();
                }
            }
        }

        // EmptyWorkspace: loaded only when no workspace (Loader destroys on deactivate)
        Loader {
            id: emptyWorkspaceLoader
            anchors.fill: parent
            active: !rootHasWorkspace
            onActiveChanged: {
                window.debugLog("app", "empty_workspace_loader_active_changed", "active=" + active);
            }
            sourceComponent: EmptyWorkspace {
                backendRef: workspaceBackend
                dt: designTokens
                onCreateWorkspaceWithPath: (path) => {
                    appController.createWorkspaceWithPath(path, false);
                }
                onOpenWorkspaceWithPath: (path) => {
                    appController.createWorkspaceWithPath(path, true);
                }
                onInitFromGithub: {
                    window.openSyncDialog()
                }

            }
        }
    }

    // === Dialogs ===
    CreateProjectDialog {
        id: createProjectDialog
        theme: designTokens
        textCoordinator: globalTextCoordinator
        onSubmitProject: function(title) {
            var trimmedTitle = title ? title.trim() : "";
            var isEmpty = (trimmedTitle === "");
            window.debugLog("project", "create_project_submit", "titleLength=" + (title ? title.length : 0) + ", isEmpty=" + isEmpty);
            if (projectController.createProject(title)) {
                createProjectDialog.close();
            }
        }
    }

    Dialog {
        id: confirmDialog
        property string actionType: ""
        property var contextData: ({})

        title: qsTr("确认删除")
        modal: true
        width: 400
        height: 220
        parent: Overlay.overlay
        x: Math.round((parent.width - width) / 2)
        y: Math.round((parent.height - height) / 2)
        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusXl; border.width: 1 }
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: designTokens.sp24
            spacing: designTokens.sp16

            AppText {
                dt: designTokens
                text: {
                    if (confirmDialog.actionType === "delete_project") return qsTr("您确定要删除作品「%1」及其所有分卷、章节吗？").arg(confirmDialog.contextData.title);
                    if (confirmDialog.actionType === "delete_volume") return qsTr("您确定要删除分卷「%1」及包含的所有章节吗？").arg(confirmDialog.contextData.title);
                    if (confirmDialog.actionType === "delete_chapter") return qsTr("您确定要删除章节「%1」吗？").arg(confirmDialog.contextData.title);
                    return qsTr("确定要删除吗？");
                }
                color: designTokens.textPrimary
                font.pixelSize: designTokens.body
                font.family: designTokens.fontFamily
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignRight
                spacing: designTokens.sp8
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    dt: designTokens
                    variant: "text"
                    onClicked: confirmDialog.close()
                }
                AppButton {
                    text: qsTr("删除")
                    dt: designTokens
                    variant: "danger"
                    onClicked: {
                        if (projectController.deleteItem(confirmDialog.actionType, confirmDialog.contextData)) {
                            confirmDialog.close();
                        }
                    }
                }
            }
        }
    }

    Dialog {
        id: errorDialog
        property string message: ""
        title: qsTr("提示")
        modal: true
        width: 340
        height: 180
        parent: Overlay.overlay
        x: Math.round((parent.width - width) / 2)
        y: Math.round((parent.height - height) / 2)
        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: designTokens.sp24
            spacing: designTokens.sp16
            AppText {
                dt: designTokens
                text: errorDialog.message
                color: designTokens.textPrimary
                font.pixelSize: designTokens.body
                font.family: designTokens.fontFamily
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            AppButton {
                text: qsTr("确定")
                dt: designTokens
                variant: "primary"
                Layout.alignment: Qt.AlignRight
                onClicked: errorDialog.close()
            }
        }
    }

    Loader {
        id: settingsDialogLoader
        active: false
        sourceComponent: SettingsDialog {
            theme: designTokens
            backendRef: settingsBackend
            workspaceBackendRef: workspaceBackend
            syncBackendRef: syncBackend
            editorBackendRef: editorBackend
            textCoordinator: globalTextCoordinator
            beforeSyncHook: function() { return window.preSyncBarrier() }
            onSettingsChanged: {
                appController.refreshState(qsTr("刷新设置失败"));
                if (themeController) themeController.reload();
            }
        }
    }


    Dialog {
        id: inputDialog
        property string actionType: ""
        property string projectId: ""
        property string volumeId: ""
        property string chapterId: ""
        property string defaultText: ""
        property string dialogTitle: qsTr("请输入")

        modal: true
        width: 300
        height: 200
        parent: Overlay.overlay
        x: Math.round((parent.width - width) / 2)
        y: Math.round((parent.height - height) / 2)
        title: inputDialog.dialogTitle

        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusXl; border.width: 1 }
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: designTokens.sp24
            spacing: designTokens.sp12

            AppText {
                dt: designTokens
                text: {
                    if (inputDialog.actionType === "volume") return qsTr("卷名称");
                    if (inputDialog.actionType === "chapter") return qsTr("章节名称");
                    return qsTr("新名称");
                }
                color: designTokens.textSecondary
                font.pixelSize: designTokens.label
                font.family: designTokens.fontFamily
            }

            CoordinatorTextField {
                id: inputField
                Layout.fillWidth: true
                dt: designTokens
                placeholderText: {
                    if (inputDialog.actionType === "volume") return qsTr("例如：第一卷");
                    if (inputDialog.actionType === "chapter") return qsTr("例如：第一章");
                    return qsTr("请输入新名称");
                }
                coordinator: globalTextCoordinator
                targetId: "main-rename-dialog"
                onAccepted: confirmInputButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    id: confirmInputButton
                    text: qsTr("确定")
                    dt: designTokens
                    variant: "primary"
                    onClicked: {
                         var title = inputField.text.trim();
                         // 章节允许空标题，后端会兜底生成默认标题
                         var allowEmpty = (inputDialog.actionType === "chapter");
                         if (title !== "" || allowEmpty) {
                             if (inputDialog.actionType === "volume") {
                                 projectController.createVolume(inputDialog.projectId, title);
                             } else if (inputDialog.actionType === "chapter") {
                                 projectController.createChapter(inputDialog.projectId, inputDialog.volumeId, title);
                             } else if (inputDialog.actionType === "rename_project") {
                                 projectController.renameProject(inputDialog.projectId, title);
                             } else if (inputDialog.actionType === "rename_volume") {
                                 projectController.renameVolume(inputDialog.projectId, inputDialog.volumeId, title);
                             } else if (inputDialog.actionType === "rename_chapter") {
                                 projectController.renameChapter(inputDialog.projectId, inputDialog.volumeId, inputDialog.chapterId, title);
                             }
                         }
                         inputDialog.close();
                     }
                }
            }
        }
        onOpened: {
            inputField.text = inputDialog.defaultText;
        }
    }
}
