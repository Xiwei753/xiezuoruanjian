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
        property color sidebarHover: "#2A2D2E"
        property color inputBg: "#3C3C3C"
        property color border: "#333333"
        property color textMain: "#CCCCCC"
        property color textDim: "#808080"
        property color accent: "#007ACC"
        property color accentHover: "#0098FF"
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
                
                onItemActivated: (type, projectId, volumeId, chapterId) => {
                    var stateStr = backend.select_tree_item_json(type, projectId, volumeId, chapterId);
                    var res = JSON.parse(stateStr);
                    if (res.success) {
                        applyState(res.state);
                    }
                }
                
                onCreateVolume: (projectId) => {
                    inputDialog.actionType = "volume";
                    inputDialog.projectId = projectId;
                    inputDialog.title = "新建卷";
                    inputDialog.open();
                }
                
                onCreateChapter: (projectId, volumeId) => {
                    inputDialog.actionType = "chapter";
                    inputDialog.projectId = projectId;
                    inputDialog.volumeId = volumeId;
                    inputDialog.title = "新建章节";
                    inputDialog.open();
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
        onAccepted: (title) => {
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
        title: "同步设置"
        modal: true
        width: 600
        height: 500
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: theme.bgDark }
        SyncPage {
            anchors.fill: parent
            theme: theme
            backendRef: backend
            onSettingsChanged: {
                applyState(JSON.parse(backend.refresh_app_state_json()));
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
