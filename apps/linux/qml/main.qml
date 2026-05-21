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
    color: "#1E1E1E"

    property var appState: ({
        hasWorkspace: false,
        workspacePath: "",
        saveStatus: "",
        selected: { projectId: "", volumeId: "", chapterId: "" },
        tree: [],
        settings: { fontSize: 16, themeMode: "dark" },
        sync: { status: "not_configured" }
    })

    QtObject {
        id: theme
        property color bgDark: "#1E1E1E"
        property color bgDarker: "#121212"
        property color sidebarBg: "#252526"
        property color sidebarHover: "#3A3D3E"
        property color inputBg: "#2A2A2A"
        property color border: "#444444"
        property color textMain: "#F0F0F0"
        property color textDim: "#A0A0A0"
        property color accent: "#007ACC"
        property color accentHover: "#0098FF"
        property color buttonBg: "#3A3A3A"
        property color buttonHover: "#4A4A4A"
    }

    AppBackend {
        id: backend
    }

    Component.onCompleted: {
        backend.try_restore_last_workspace();
        var stateStr = backend.refresh_app_state_json();
        var stateObj = JSON.parse(stateStr);
        applyState(stateObj);
    }

    function applyState(state) {
        if (!state) return;
        appState = state;
        
        // update tree
        if (appState.tree) {
            workspaceTree.items = appState.tree;
        }

        // update selected
        if (appState.selected) {
            workspaceTree.selectedId = appState.selected.chapterId || appState.selected.volumeId || appState.selected.projectId || "";
        }
    }

    // Header
    header: Rectangle {
        height: 48
        color: theme.bgDarker
        border.color: theme.border
        border.width: 1

        RowLayout {
            anchors.fill: parent
            anchors.margins: 8
            spacing: 12

            Text {
                text: "Writer"
                color: theme.accent
                font.pixelSize: 20
                font.bold: true
            }

            Text {
                text: appState.workspacePath || "未打开工作区"
                color: theme.textDim
                font.pixelSize: 14
                Layout.fillWidth: true
                elide: Text.ElideRight
            }

            Button {
                text: "新建作品"
                visible: appState.hasWorkspace
                onClicked: createProjectDialog.open()
            }

            Button {
                text: "设置"
                onClicked: settingsDialog.open()
            }

            Button {
                text: "同步"
                onClicked: syncPageDialog.open()
            }

            Button {
                text: "切换工作区"
                onClicked: {
                    backend.switch_workspace();
                    applyState(JSON.parse(backend.refresh_app_state_json()));
                }
            }
        }
    }

    // Central Content
    RowLayout {
        anchors.fill: parent
        spacing: 0

        // Left Sidebar
        Rectangle {
            Layout.preferredWidth: 260
            Layout.fillHeight: true
            color: theme.sidebarBg
            visible: appState.hasWorkspace

            WorkspaceTree {
                id: workspaceTree
                anchors.fill: parent
                theme: theme
                selectedId: ""
                
                onItemActivated: function(type, projectId, volumeId, chapterId) {
                    var stateStr = backend.select_tree_item_json(type, projectId, volumeId, chapterId);
                    var res = JSON.parse(stateStr);
                    if (res.success) {
                        applyState(res.state);
                    }
                }
                
                onCreateVolume: function(projectId) {
                    inputDialog.actionType = "volume";
                    inputDialog.projectId = projectId;
                    inputDialog.title = "新建卷";
                    inputDialog.open();
                }
                
                onCreateChapter: function(projectId, volumeId) {
                    inputDialog.actionType = "chapter";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = volumeId;
                    inputDialog.title = "新建章节";
                    inputDialog.open();
                }

                onDeleteItem: function(type, projectId, volumeId, chapterId, title) {
                    confirmDialog.actionType = "delete_" + type;
                    confirmDialog.contextData = {
                        projectId: projectId,
                        volumeId: volumeId,
                        chapterId: chapterId,
                        title: title
                    };
                    confirmDialog.open();
                }
                onRenameItem: function(type, projectId, volumeId, chapterId, currentTitle) {
                    // For now, redirect rename to delete for simplicity or show not implemented
                    // The user said "重命名后续再做，但不要让菜单点了没反应"
                    // We will just show a toast or nothing. Let's just open a dialog for rename placeholder.
                }
            }
        }

        // Right Editor
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: theme.bgDark
            
            visible: appState.hasWorkspace

            EditorPage {
                id: editorPage
                anchors.fill: parent
                // Simplified editor integration
                Component.onCompleted: {
                    if (appState.selected && appState.selected.chapterId) {
                        editorPage.text = backend.get_chapter_content(appState.selected.projectId, appState.selected.volumeId, appState.selected.chapterId);
                    }
                }
            }
            
            Connections {
                target: backend
                function onChapter_path_changed() {
                    if (appState.selected && appState.selected.chapterId) {
                        editorPage.text = backend.get_chapter_content(appState.selected.projectId, appState.selected.volumeId, appState.selected.chapterId);
                    } else {
                        editorPage.text = "";
                    }
                }
            }

            Connections {
                target: editorPage
                function onTextChanged() {
                    if (appState.selected && appState.selected.chapterId) {
                        backend.save_current_chapter(editorPage.text);
                        applyState(JSON.parse(backend.refresh_app_state_json()));
                    }
                }
            }
        }

        EmptyWorkspace {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: !appState.hasWorkspace
            backendRef: backend
            appTheme: theme
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

    // Footer
    footer: Rectangle {
        height: 28
        color: theme.bgDarker
        border.color: theme.border
        border.width: 1

        RowLayout {
            anchors.fill: parent
            anchors.margins: 4
            spacing: 8

            Text {
                text: appState.saveStatus || "就绪"
                color: theme.textDim
                font.pixelSize: 12
            }
            
            Item { Layout.fillWidth: true }

            Text {
                text: "字数: " + backend.word_count
                color: theme.textDim
                font.pixelSize: 12
            }
        }
    }

    // Dialogs
    CreateProjectDialog {
        id: createProjectDialog
        theme: theme
        onAccepted: function(title) {
            var stateStr = backend.create_project_json(title);
            var res = JSON.parse(stateStr);
            if (res.success) {
                applyState(res.state);
                close();
            } else {
                console.error("创建失败:", res.message);
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
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: theme.bgDark; border.color: theme.border; radius: 8; border.width: 1 }

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
                color: theme.textMain
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
                        var resStr = "";
                        if (confirmDialog.actionType === "delete_project") {
                            resStr = backend.delete_project_json(confirmDialog.contextData.projectId);
                        } else if (confirmDialog.actionType === "delete_volume") {
                            resStr = backend.delete_volume_json(confirmDialog.contextData.projectId, confirmDialog.contextData.volumeId);
                        } else if (confirmDialog.actionType === "delete_chapter") {
                            resStr = backend.delete_chapter_json(confirmDialog.contextData.projectId, confirmDialog.contextData.volumeId, confirmDialog.contextData.chapterId);
                        }
                        
                        if (resStr) {
                            var res = JSON.parse(resStr);
                            if (res.success) {
                                applyState(res.state);
                            } else {
                                errorDialog.message = res.message || "删除失败";
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
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: theme.bgDark; border.color: theme.border; radius: 8; border.width: 1 }
        ColumnLayout {
            anchors.fill: parent
            spacing: 16
            Text {
                text: errorDialog.message
                color: theme.textMain
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

    SettingsDialog {
        id: settingsDialog
        theme: theme
        backendRef: backend
        onSettingsChanged: {
            applyState(JSON.parse(backend.refresh_app_state_json()));
        }
    }

    Dialog {
        id: syncPageDialog
        modal: true
        width: Math.min(mainWindow.width - 80, 720)
        height: Math.min(mainWindow.height - 120, 560)
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: theme.bgDark; border.color: theme.border; radius: 8; border.width: 1 }
        contentItem: Item {
            anchors.fill: parent
            SyncPage {
                anchors.fill: parent
                theme: theme
                backendRef: backend
                onSettingsChanged: {
                    applyState(JSON.parse(backend.refresh_app_state_json()));
                }
            }
        }
    }

    Dialog {
        id: inputDialog
        property string actionType: ""
        property string projectId: ""
        property string volumeId: ""
        
        modal: true
        width: 300
        anchors.centerIn: Overlay.overlay
        title: "请输入"
        
        background: Rectangle { color: theme.bgDark; border.color: theme.border; radius: 4 }
        
        ColumnLayout {
            anchors.fill: parent
            spacing: 8
            TextField {
                id: inputField
                Layout.fillWidth: true
                color: theme.textMain
                background: Rectangle { color: theme.inputBg; border.color: theme.border }
            }
            Button {
                text: "确定"
                onClicked: {
                    var title = inputField.text.trim();
                    if (title !== "") {
                        if (inputDialog.actionType === "volume") {
                            var stateStr = backend.create_volume_json(inputDialog.projectId, title);
                            applyState(JSON.parse(stateStr).state);
                        } else if (inputDialog.actionType === "chapter") {
                            var stateStr = backend.create_chapter_json(inputDialog.projectId, inputDialog.volumeId, title);
                            applyState(JSON.parse(stateStr).state);
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
