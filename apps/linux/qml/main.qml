// =============================================================================
// main.qml — 应用入口窗口
// =============================================================================
//
// 层级：Linux UI 层（QML 页面）
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
import WriterApp 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 1100
    height: 768
    title: "Writer"
    color: designTokens.bg

    function debugLog(module, event, message) {
        if (backend.debug_qml_enabled) {
            backend.log_qml("info", module, event, message);
        }
    }

    function debugWarn(module, event, message) {
        if (backend.debug_qml_enabled) {
            backend.log_qml("warn", module, event, message);
        }
    }

    function debugError(module, event, message) {
        if (backend.debug_qml_enabled) {
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

    property string previousEditorText: ""

    // Design tokens
    DesignTokens {
        id: designTokens
        isDark: {
            if (appState.settings && appState.settings.themeMode === "dark") return true;
            if (appState.settings && appState.settings.themeMode === "light") return false;
            return backend.system_color_scheme !== "light";
        }
        monetColor: backend.setting_monet_color
    }

    AppBackend {
        id: backend
    }

    AppController {
        id: appController
        backendRef: backend
        onErrorRaised: function(message) {
            errorDialog.message = message;
            errorDialog.open();
        }
    }

    Component.onCompleted: {
        window.debugLog("app", "qml_completed", "QML components fully loaded");
        appController.restoreWorkspace();
    }

    onActiveChanged: {
        if (active && backend) {
            foregroundAutoSyncTimer.restart();
        }
    }

    onClosing: {
        if (appController.inWriting) {
            backend.flush_writing_stats();
        }
    }

    Timer {
        id: workspaceOpenAutoSyncTimer
        interval: 1500
        repeat: false
        onTriggered: {
            if (backend && backend.has_workspace && backend.sync_enabled && backend.sync_auto_sync && backend.sync_remote_url && backend.has_sync_token) {
                backend.request_auto_sync("auto_sync_on_workspace_open");
            }
        }
    }

    Timer {
        id: foregroundAutoSyncTimer
        interval: 1200
        repeat: false
        onTriggered: {
            if (backend && backend.has_workspace && backend.sync_enabled && backend.sync_auto_sync && backend.sync_remote_url && backend.has_sync_token) {
                backend.maybe_auto_sync_on_foreground();
            }
        }
    }

    function applyState(state) {
        appController.applyState(state);
    }

    Connections {
        target: backend
        function onProjects_reloaded() {
            appController.refreshState("刷新作品列表失败");
        }
        function onWorkspace_state_changed() {
            appController.refreshState("刷新工作区状态失败");
        }
        function onWorkspace_opened() {
            workspaceOpenAutoSyncTimer.restart();
        }
        function onWorkspace_content_changed() {
            appController.refreshState("刷新工作区内容失败");
        }
        function onClear_editor() {
            if (writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.previousEditorText = "";
            }
        }
        function onSettings_changed() {
            appController.refreshState("刷新设置失败");
        }
    }

    // === Main Content ===
    Item {
        anchors.fill: parent

        // StarMapWorkspace: shown when in starmap editor mode
        Loader {
            id: starmapWorkspaceLoader
            anchors.fill: parent
            active: appState.hasWorkspace && appController.inStarmap
            sourceComponent: StarMapWorkspace {
                dt: designTokens
                backendRef: backend
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
            active: appState.hasWorkspace && appController.route === "hub"
            sourceComponent: CreativeHub {
                dt: designTokens
                backendRef: backend
                appState: window.appState
                tree: window.appState.tree || []
                aiCapable: backend.ai_available
                aiEnabled: backend.ai_enabled

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
                    appController.renameProject(projectId, title);
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
            active: appState.hasWorkspace && appController.inWriting
            sourceComponent: WritingWorkspace {
                dt: designTokens
                backendRef: backend
                appState: window.appState
                tree: window.appState.tree || []
                workspaceProjectId: appController.writingProjectId
                projectTitle: appController.writingProjectTitle
                aiCapable: backend.ai_available
                aiEnabled: backend.ai_enabled

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
                    inputDialog.dialogTitle = "新建卷";
                    inputDialog.open();
                }

                onCreateChapterRequested: function(projectId, volumeId) {
                    inputDialog.actionType = "chapter";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = volumeId;
                    inputDialog.dialogTitle = "新建章节";
                    inputDialog.open();
                }
            }
        }

        // EmptyWorkspace: shown when no workspace
        EmptyWorkspace {
            anchors.fill: parent
            visible: !appState.hasWorkspace
            backendRef: backend
            appTheme: designTokens
            onCreateWorkspace: {
                appController.createWorkspace(false);
            }
            onOpenWorkspace: {
                appController.createWorkspace(true);
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
            if (appController.createProject(title)) {
                createProjectDialog.close();
            }
        }
    }

    Dialog {
        id: confirmDialog
        property string actionType: ""
        property var contextData: ({})

        title: "确认删除"
        modal: true
        width: 400
        height: 220
        x: (parent ? (parent.width - width) / 2 : 0)
        y: (parent ? (parent.height - height) / 2 : 0)
        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusMd; border.width: 1 }

        ColumnLayout {
            anchors.fill: parent
            spacing: 16

            Text {
                text: {
                    if (confirmDialog.actionType === "delete_project") return "您确定要删除作品「" + confirmDialog.contextData.title + "」及其所有分卷、章节吗？";
                    if (confirmDialog.actionType === "delete_volume") return "您确定要删除分卷「" + confirmDialog.contextData.title + "」及包含的所有章节吗？";
                    if (confirmDialog.actionType === "delete_chapter") return "您确定要删除章节「" + confirmDialog.contextData.title + "」吗？";
                    return "确定要删除吗？";
                }
                color: designTokens.textPrimary
                font.pixelSize: 14
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignRight
                spacing: 8
                Item { Layout.fillWidth: true }
                Button {
                    text: "取消"
                    onClicked: confirmDialog.close()
                }
                Button {
                    text: "删除"
                    onClicked: {
                        appController.deleteItem(confirmDialog.actionType, confirmDialog.contextData);
                        confirmDialog.close();
                    }
                }
            }
        }
    }

    Dialog {
        id: errorDialog
        property string message: ""
        title: "提示"
        modal: true
        width: 340
        height: 180
        x: (parent ? (parent.width - width) / 2 : 0)
        y: (parent ? (parent.height - height) / 2 : 0)
        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusMd; border.width: 1 }
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Text {
                text: errorDialog.message
                color: designTokens.textPrimary
                font.pixelSize: 14
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Button {
                text: "确定"
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
            backendRef: backend
            onSettingsChanged: {
                appController.refreshState("刷新设置失败");
            }
        }
    }

    Loader {
        id: syncDialogLoader
        active: false
        sourceComponent: Dialog {
            modal: true
            title: "同步设置"
            width: Math.max(360, Math.min(window.width - 80, 720))
            height: Math.max(420, Math.min(window.height - 120, 560))
            x: (parent ? (parent.width - width) / 2 : 0)
            y: (parent ? (parent.height - height) / 2 : 0)
            background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusMd; border.width: 1 }

            header: null

            contentItem: ScrollView {
                id: syncDialogScroll
                clip: true
                topPadding: 16
                bottomPadding: 16
                leftPadding: 16
                rightPadding: 16

                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                ScrollBar.vertical.policy: ScrollBar.AsNeeded

                SyncPage {
                    id: syncPage
                    width: syncDialogScroll.availableWidth
                    theme: designTokens
                    backendRef: backend
                    beforeSyncHook: function() {
                        backend.flush_writing_stats();
                    }
                    onSettingsChanged: {
                        appController.refreshState("刷新同步设置失败");
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
        property string dialogTitle: "请输入"

        modal: true
        width: 300
        height: 200
        x: (parent ? (parent.width - width) / 2 : 0)
        y: (parent ? (parent.height - height) / 2 : 0)
        title: inputDialog.dialogTitle

        background: Rectangle { color: designTokens.surface; border.color: designTokens.border; radius: designTokens.radiusMd }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            Text {
                text: inputDialog.actionType === "volume" ? "卷名称" : "章节名称"
                color: designTokens.textSecondary
                font.pixelSize: 12
            }

            TextField {
                id: inputField
                Layout.fillWidth: true
                placeholderText: inputDialog.actionType === "volume" ? "例如：第一卷" : "例如：第一章"
                color: designTokens.textPrimary
                background: Rectangle { color: designTokens.paper; border.color: designTokens.border; radius: designTokens.radiusSm }
            }
            Button {
                text: "确定"
                onClicked: {
                    var title = inputField.text.trim();
                    if (title !== "") {
                        if (inputDialog.actionType === "volume") {
                            appController.createVolume(inputDialog.projectId, title);
                        } else if (inputDialog.actionType === "chapter") {
                            appController.createChapter(inputDialog.projectId, inputDialog.volumeId, title);
                        }
                    }
                    inputDialog.close();
                }
            }
        }
        onOpened: {
            inputField.text = "";
            inputField.forceActiveFocus();
        }
    }
}
