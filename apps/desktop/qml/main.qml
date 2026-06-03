// =============================================================================
// main.qml — 应用入口窗口
// =============================================================================
//
// 层级：Desktop UI 层（QML 页面）
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
import QtQuick.Layouts
import QtQuick.Window

ApplicationWindow {
    id: window
    visible: true
    width: 1100
    height: 768
    title: qsTr("素笺写作")
    color: designTokens.bg

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

    function openSettingsDialog() {
        if (!settingsDialogLoader.active) {
            settingsDialogLoader.active = true;
        }
        if (settingsDialogLoader.item) {
            settingsDialogLoader.item.open();
        }
    }

    function openSyncDialog() {
        if (!syncDialogLoader.active) {
            syncDialogLoader.active = true;
        }
        if (syncDialogLoader.item) {
            syncDialogLoader.item.open();
        }
    }

    property alias appState: appController.appState
    readonly property bool rootHasWorkspace: workspaceBackend !== null && workspaceBackend.has_workspace === true

    property string previousEditorText: ""

    // Design tokens
    DesignTokens {
        id: designTokens
        isDark: {
            if (appState.settings && appState.settings.themeMode === "dark") return true;
            if (appState.settings && appState.settings.themeMode === "light") return false;
            return appBackend !== null && appBackend.system_color_scheme !== "light";
        }
        monetColor: settingsBackend !== null ? settingsBackend.setting_monet_color : ""
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
        window.verifyBackendRuntime();
        window.debugLog("app", "qml_completed", "QML components fully loaded");
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
        }
    }

    Timer {
        id: workspaceOpenAutoSyncTimer
        interval: 1500
        repeat: false
        onTriggered: {
            if (syncBackend) {
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
                editorBackendRef: editorBackend
                starmapBackendRef: starmapBackend
                starMapController: globalStarMapController
                appState: window.appState
                tree: window.appState.tree || []
                aiCapable: settingsBackend.ai_available
                aiEnabled: settingsBackend.ai_enabled

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

                onOpenSync: {
                    window.debugLog("sync", "sync_dialog_open", "");
                    window.openSyncDialog();
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

                onBackToProjects: {
                    appController.openHub();
                    window.debugLog("workspace", "exit_writing_mode", "");
                }

                onOpenSettings: {
                    window.openSettingsDialog();
                }

                onOpenSync: {
                    window.openSyncDialog();
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
                    inputDialog.defaultText = "";
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

        // EmptyWorkspace: shown when no workspace
        EmptyWorkspace {
            anchors.fill: parent
            visible: !rootHasWorkspace
            backendRef: workspaceBackend
            appTheme: designTokens
            onCreateWorkspaceWithPath: (path) => {
                appController.createWorkspaceWithPath(path, false);
            }
            onOpenWorkspaceWithPath: (path) => {
                appController.createWorkspaceWithPath(path, true);
            }
            onInitFromGithub: {
                openSyncDialog();
            }
        }
    }

    // === Dialogs ===
    CreateProjectDialog {
        id: createProjectDialog
        theme: designTokens
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
                    theme: designTokens
                    variant: "text"
                    onClicked: confirmDialog.close()
                }
                AppButton {
                    text: qsTr("删除")
                    theme: designTokens
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
                text: errorDialog.message
                color: designTokens.textPrimary
                font.pixelSize: designTokens.body
                font.family: designTokens.fontFamily
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            AppButton {
                text: qsTr("确定")
                theme: designTokens
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
            onSettingsChanged: {
                appController.refreshState(qsTr("刷新设置失败"));
            }
        }
    }

    Loader {
        id: syncDialogLoader
        active: false
        sourceComponent: Dialog {
            modal: true
            title: qsTr("同步设置")
            width: Math.max(360, Math.min(window.width - 80, 720))
            height: Math.max(420, Math.min(window.height - 120, 560))
            parent: Overlay.overlay
            x: Math.round((parent.width - width) / 2)
            y: Math.round((parent.height - height) / 2)
            background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusXl; border.width: 1 }

            header: null

            contentItem: Item {
                SyncPage {
                    id: syncPage
                    anchors.fill: parent
                    anchors.margins: designTokens.sp16
                    theme: designTokens
                    backendRef: syncBackend
                    beforeSyncHook: function() {
                        editorBackend.flush_writing_stats();
                    }
                    onSettingsChanged: {
                        appController.refreshState(qsTr("刷新同步设置失败"));
                    }
                }
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
                text: {
                    if (inputDialog.actionType === "volume") return qsTr("卷名称");
                    if (inputDialog.actionType === "chapter") return qsTr("章节名称");
                    return qsTr("新名称");
                }
                color: designTokens.textSecondary
                font.pixelSize: designTokens.label
                font.family: designTokens.fontFamily
            }

            AppTextField {
                id: inputField
                Layout.fillWidth: true
                theme: designTokens
                placeholderText: {
                    if (inputDialog.actionType === "volume") return qsTr("例如：第一卷");
                    if (inputDialog.actionType === "chapter") return qsTr("例如：第一章");
                    return qsTr("请输入新名称");
                }
                onAccepted: confirmInputButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    id: confirmInputButton
                    text: qsTr("确定")
                    theme: designTokens
                    variant: "primary"
                    onClicked: {
                        var title = inputField.text.trim();
                        if (title !== "") {
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
            inputField.forceActiveFocus();
        }
    }
}
