import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import WriterApp 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 1100
    height: 768
    title: "Writer"
    color: designTokens.bg

    function generateActionId() {
        return Math.random().toString(36).substring(2, 8);
    }

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

    property var appState: ({
        hasWorkspace: false,
        workspacePath: "",
        saveStatus: "",
        selected: { projectId: "", volumeId: "", chapterId: "" },
        tree: [],
        settings: { fontSize: 16, themeMode: "dark" },
        sync: { status: "not_configured" }
    })

    property bool isLoadingChapter: false
    property string previousEditorText: ""
    property bool writingMode: false
    property string writingProjectId: ""
    property string writingProjectTitle: ""
    property string writingVolumeId: ""
    property string writingChapterId: ""
    property string writingChapterTitle: ""

    property bool starmapMode: false
    property string currentStarmapId: ""
    property string currentStarmapTitle: ""

    // Design tokens
    DesignTokens {
        id: designTokens
        isDark: {
            if (appState.settings && appState.settings.themeMode === "dark") return true;
            if (appState.settings && appState.settings.themeMode === "light") return false;
            return backend.system_color_scheme !== "light";
        }
    }

    AppBackend {
        id: backend
    }

    Component.onCompleted: {
        window.debugLog("app", "qml_completed", "QML components fully loaded");
        backend.query_system_color_scheme();
        backend.try_restore_last_workspace();
        var stateStr = backend.refresh_app_state_json();
        try {
            var stateObj = JSON.parse(stateStr);
            applyState(stateObj);
        } catch (e) {
            window.debugError("app", "app_state_parse_failed", "error: " + e + ", raw: " + stateStr);
        }
    }

    onActiveChanged: {
        if (active && backend) {
            foregroundAutoSyncTimer.restart();
        }
    }

    onClosing: {
        if (writingMode && writingChapterId) {
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
        if (!state) return;
        appState = state;

        if (appState.tree && creativeHubLoader.item) {
            creativeHubLoader.item.tree = appState.tree;
        }
        if (appState.tree && writingWorkspaceLoader.item) {
            writingWorkspaceLoader.item.tree = appState.tree;
        }
    }

    Connections {
        target: backend
        function onProjects_reloaded() {
            try {
                applyState(JSON.parse(backend.refresh_app_state_json()));
            } catch (e) {
                window.debugError("tree", "projects_reloaded_parse_failed", "error: " + e);
            }
        }
        function onWorkspace_state_changed() {
            try {
                applyState(JSON.parse(backend.refresh_app_state_json()));
            } catch (e) {
                window.debugError("workspace", "workspace_state_changed_parse_failed", "error: " + e);
            }
        }
        function onWorkspace_opened() {
            workspaceOpenAutoSyncTimer.restart();
        }
        function onWorkspace_content_changed() {
            try {
                applyState(JSON.parse(backend.refresh_app_state_json()));
            } catch (e) {
                window.debugError("workspace", "workspace_content_changed_parse_failed", "error: " + e);
            }
        }
        function onClear_editor() {
            if (writingWorkspaceLoader.item) {
                writingWorkspaceLoader.item.previousEditorText = "";
            }
        }
        function onSettings_changed() {
            try {
                applyState(JSON.parse(backend.refresh_app_state_json()));
            } catch (e) {}
        }
    }

    // === Main Content ===
    Item {
        anchors.fill: parent

        // StarMapWorkspace: shown when in starmap editor mode
        Loader {
            id: starmapWorkspaceLoader
            anchors.fill: parent
            active: appState.hasWorkspace && starmapMode
            sourceComponent: StarMapWorkspace {
                starmapId: window.currentStarmapId
                starmapTitle: window.currentStarmapTitle
                onBackClicked: {
                    window.starmapMode = false;
                }
            }
        }

        // CreativeHub: shown when workspace open and not in writing mode
        Loader {
            id: creativeHubLoader
            anchors.fill: parent
            active: appState.hasWorkspace && !writingMode && !starmapMode
            sourceComponent: CreativeHub {
                dt: designTokens
                backendRef: backend
                appState: window.appState
                tree: window.appState.tree || []
                aiCapable: backend.ai_available
                aiEnabled: backend.ai_enabled

                onOpenProject: function(projectId, projectTitle) {
                    window.writingProjectId = projectId;
                    window.writingProjectTitle = projectTitle;
                    window.writingMode = true;
                    window.debugLog("workspace", "enter_writing_mode", "projectId=" + projectId);
                }

                onCreateProject: {
                    window.debugLog("project", "create_project_dialog_open", "");
                    createProjectDialog.open();
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
                    backend.switch_workspace();
                    window.writingMode = false;
                    try {
                        applyState(JSON.parse(backend.refresh_app_state_json()));
                    } catch (e) {
                        window.debugError("workspace", "switch_workspace_parse_failed", "error: " + e);
                    }
                }
            }
        }

        // WritingWorkspace: shown when in writing mode
        Loader {
            id: writingWorkspaceLoader
            anchors.fill: parent
            active: appState.hasWorkspace && writingMode
            sourceComponent: WritingWorkspace {
                dt: designTokens
                backendRef: backend
                appState: window.appState
                tree: window.appState.tree || []
                projectId: window.writingProjectId
                projectTitle: window.writingProjectTitle
                aiCapable: backend.ai_available
                aiEnabled: backend.ai_enabled

                onBackToProjects: {
                    window.writingMode = false;
                    window.debugLog("workspace", "exit_writing_mode", "");
                    try {
                        applyState(JSON.parse(backend.refresh_app_state_json()));
                    } catch (e) {}
                }

                Connections {
                    target: creativeHubLoader.item
                    function onOpenStarmapWorkspace(smId, smTitle) {
                        window.currentStarmapId = smId;
                        window.currentStarmapTitle = smTitle;
                        window.starmapMode = true;
                    }
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
                backend.create_new_workspace();
                applyState(JSON.parse(backend.refresh_app_state_json()));
            }
            onOpenWorkspace: {
                backend.open_existing_workspace();
                applyState(JSON.parse(backend.refresh_app_state_json()));
            }
        }
    }

    // === Dialogs ===
    CreateProjectDialog {
        id: createProjectDialog
        theme: designTokens
        onSubmitProject: function(title) {
            var actionId = window.generateActionId();
            var trimmedTitle = title ? title.trim() : "";
            var isEmpty = (trimmedTitle === "");
            window.debugLog("project", "create_project_submit", "[actionId=" + actionId + "] titleLength=" + (title ? title.length : 0) + ", isEmpty=" + isEmpty);

            var stateStr = "";
            try {
                stateStr = backend.create_project_json(title, actionId);
            } catch (e) {
                window.debugError("project", "create_project_failed", "[actionId=" + actionId + "] backend call failed: " + e);
                errorDialog.message = "后端调用失败: " + e;
                errorDialog.open();
                return;
            }

            var res;
            try {
                res = JSON.parse(stateStr);
            } catch (e) {
                window.debugError("project", "create_project_parse_failed", "[actionId=" + actionId + "] JSON.parse failed. Raw response: " + stateStr + ", error: " + e);
                errorDialog.message = "解析后端返回数据失败";
                errorDialog.open();
                return;
            }

            window.debugLog("project", "create_project_received", "[actionId=" + actionId + "] success=" + res.success + ", userMessage=" + (res.userMessage || res.message || ""));

            if (res.success) {
                applyState(res.state);
                createProjectDialog.close();
            } else {
                errorDialog.message = res.userMessage || res.message || "创建失败";
                errorDialog.open();
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
                        var actionId = window.generateActionId();
                        var resStr = "";
                        if (confirmDialog.actionType === "delete_project") {
                            window.debugLog("project", "delete_project_submit", "[actionId=" + actionId + "] projectId=" + confirmDialog.contextData.projectId);
                            resStr = backend.delete_project_json(confirmDialog.contextData.projectId, actionId);
                        } else if (confirmDialog.actionType === "delete_volume") {
                            window.debugLog("volume", "delete_volume_submit", "[actionId=" + actionId + "] projectId=" + confirmDialog.contextData.projectId + ", volumeId=" + confirmDialog.contextData.volumeId);
                            resStr = backend.delete_volume_json(confirmDialog.contextData.projectId, confirmDialog.contextData.volumeId, actionId);
                        } else if (confirmDialog.actionType === "delete_chapter") {
                            window.debugLog("chapter", "delete_chapter_submit", "[actionId=" + actionId + "] projectId=" + confirmDialog.contextData.projectId + ", volumeId=" + confirmDialog.contextData.volumeId + ", chapterId=" + confirmDialog.contextData.chapterId);
                            resStr = backend.delete_chapter_json(confirmDialog.contextData.projectId, confirmDialog.contextData.volumeId, confirmDialog.contextData.chapterId, actionId);
                        }

                        if (resStr) {
                            try {
                                var res = JSON.parse(resStr);
                                if (res.success) {
                                    window.debugLog(confirmDialog.actionType.replace("delete_", ""), "delete_success", "[actionId=" + actionId + "]");
                                    applyState(res.state);
                                } else {
                                    window.debugError(confirmDialog.actionType.replace("delete_", ""), "delete_failed", "[actionId=" + actionId + "] error=" + (res.message || ""));
                                    errorDialog.message = res.message || "删除失败";
                                    errorDialog.open();
                                }
                            } catch (e) {
                                window.debugError(confirmDialog.actionType.replace("delete_", ""), "delete_parse_failed", "[actionId=" + actionId + "] error=" + e + ", raw=" + resStr);
                                errorDialog.message = "解析后端返回数据失败";
                                errorDialog.open();
                            }
                        }
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
                applyState(JSON.parse(backend.refresh_app_state_json()));
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
                        applyState(JSON.parse(backend.refresh_app_state_json()));
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
                        var actionId = window.generateActionId();
                        if (inputDialog.actionType === "volume") {
                            window.debugLog("volume", "create_volume_submit", "[actionId=" + actionId + "] projectId=" + inputDialog.projectId + ", title=" + title);
                            var stateStr = "";
                            try {
                                stateStr = backend.create_volume_json(inputDialog.projectId, title, actionId);
                            } catch (e) {
                                window.debugError("volume", "create_volume_failed", "[actionId=" + actionId + "] backend call failed: " + e);
                                errorDialog.message = "后端调用失败: " + e;
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            var res;
                            try {
                                res = JSON.parse(stateStr);
                            } catch (e) {
                                window.debugError("volume", "create_volume_parse_failed", "[actionId=" + actionId + "] JSON.parse failed. Raw response: " + stateStr + ", error: " + e);
                                errorDialog.message = "解析后端返回数据失败";
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            window.debugLog("volume", "create_volume_received", "[actionId=" + actionId + "] success=" + res.success + ", message=" + (res.message || ""));
                            if (res.success) {
                                applyState(res.state);
                            } else {
                                errorDialog.message = res.message || "创建卷失败";
                                errorDialog.open();
                            }
                        } else if (inputDialog.actionType === "chapter") {
                            window.debugLog("chapter", "create_chapter_submit", "[actionId=" + actionId + "] projectId=" + inputDialog.projectId + ", volumeId=" + inputDialog.volumeId + ", title=" + title);
                            var stateStr = "";
                            try {
                                stateStr = backend.create_chapter_json(inputDialog.projectId, inputDialog.volumeId, title, actionId);
                            } catch (e) {
                                window.debugError("chapter", "create_chapter_failed", "[actionId=" + actionId + "] backend call failed: " + e);
                                errorDialog.message = "后端调用失败: " + e;
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            var res;
                            try {
                                res = JSON.parse(stateStr);
                            } catch (e) {
                                window.debugError("chapter", "create_chapter_parse_failed", "[actionId=" + actionId + "] JSON.parse failed. Raw response: " + stateStr + ", error: " + e);
                                errorDialog.message = "解析后端返回数据失败";
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            window.debugLog("chapter", "create_chapter_received", "[actionId=" + actionId + "] success=" + res.success + ", message=" + (res.message || ""));
                            if (res.success) {
                                applyState(res.state);
                            } else {
                                errorDialog.message = res.message || "创建章节失败";
                                errorDialog.open();
                            }
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
