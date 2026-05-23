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
    color: theme.bgDark

    property var appState: ({
        hasWorkspace: false,
        workspacePath: "",
        saveStatus: "",
        selected: { projectId: "", volumeId: "", chapterId: "" },
        tree: [],
        settings: { fontSize: 16, themeMode: "dark" },
        sync: { status: "not_configured" }
    })

    SystemPalette { id: sysPalette; colorGroup: SystemPalette.Active }

    QtObject {
        id: theme
        // Keep spacing and font sizes to avoid rewriting geometry
        property int sp4: 4
        property int sp6: 6
        property int sp8: 8
        property int sp12: 12
        property int sp16: 16
        property int sp24: 24
        property int sp32: 32

        property int radiusSm: 4
        property int radiusMd: 8
        property int radiusLg: 12

        property int fontXs: 11
        property int fontSm: 12
        property int fontMd: 14
        property int fontLg: 16
        property int fontXl: 18
        property int fontXxl: 22

        // Alias colors to system palette to provide seamless desktop integration
        property color bg: sysPalette.window
        property color bgDark: sysPalette.window
        property color bgDarker: sysPalette.window
        property color surface: sysPalette.base
        property color surfaceAlt: sysPalette.base
        property color border: sysPalette.mid
        property color divider: sysPalette.midlight
        property color primary: sysPalette.highlight
        property color primaryHover: sysPalette.highlight
        property color primaryText: sysPalette.highlightedText
        property color textPrimary: sysPalette.text
        property color textSecondary: sysPalette.text
        property color danger: "#EF4444"
        property color warning: "#F59E0B"
        property color success: "#10B981"
        property color hover: sysPalette.light
        property color sidebarBg: sysPalette.window
        property color sidebarHover: sysPalette.light
        property color inputBg: sysPalette.base
        property color textMain: sysPalette.windowText
        property color textDim: sysPalette.text
        property color accent: sysPalette.highlight
        property color accentHover: sysPalette.highlight
        property color buttonBg: sysPalette.button
        property color buttonHover: sysPalette.light
        property color editorBg: sysPalette.base


    }

    AppBackend {
        id: backend
    }

    Component.onCompleted: {
        backend.query_system_color_scheme();
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

    function getSelectedItemTitle() {
        if (!appState || !appState.selected || !appState.tree) return "";
        var sel = appState.selected;
        var targetId = sel.chapterId || sel.volumeId || sel.projectId || "";
        if (!targetId) return "";
        for (var i = 0; i < appState.tree.length; i++) {
            if (appState.tree[i].id === targetId) {
                return appState.tree[i].title;
            }
        }
        return "";
    }

    function getProjectVolumes(projectId) {
        var vols = [];
        if (!appState || !appState.tree) return vols;
        for (var i = 0; i < appState.tree.length; i++) {
            var item = appState.tree[i];
            if (item.type === "volume" && item.projectId === projectId) {
                vols.push(item);
            }
        }
        return vols;
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
                onClicked: {
                    console.log("[LinuxCreateProjectQml] open dialog");
                    createProjectDialog.open();
                }
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
                onShowError: function(msg) {
                    errorDialog.message = msg;
                    errorDialog.open();
                }
                theme: theme
                selectedId: {
                    if (!appState || !appState.selected) return "";
                    return appState.selected.chapterId || appState.selected.volumeId || appState.selected.projectId || "";
                }
                
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
                    errorDialog.message = "重命名功能尚未实现";
                    errorDialog.open();
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
                appTheme: theme
                backendRef: backend
                visible: appState.selected && appState.selected.chapterId
                // Simplified editor integration
                Component.onCompleted: {
                    if (appState.selected && appState.selected.chapterId) {
                        var content = backend.get_chapter_content(appState.selected.projectId, appState.selected.volumeId, appState.selected.chapterId);
                        console.log("[LinuxEditorLoad] onCompleted loading content, len=" + content.length);
                        editorPage.loadContent(content);
                    }
                }
            }
            
            Connections {
                target: backend
                function onChapter_path_changed() {
                    if (appState.selected && appState.selected.chapterId) {
                        var content = backend.get_chapter_content(appState.selected.projectId, appState.selected.volumeId, appState.selected.chapterId);
                        console.log("[LinuxEditorLoad] loading chapter content: projectId=" + appState.selected.projectId + ", volumeId=" + appState.selected.volumeId + ", chapterId=" + appState.selected.chapterId + ", len=" + content.length);
                        editorPage.loadContent(content);
                    } else {
                        console.log("[LinuxEditorLoad] clearing editor content");
                        editorPage.clearText();
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

            // Overview Dashboard shown when a project or volume (but not chapter) is selected
            Rectangle {
                anchors.fill: parent
                color: "transparent"
                visible: appState.hasWorkspace && (!appState.selected || !appState.selected.chapterId) && (appState.selected && (appState.selected.projectId || appState.selected.volumeId))

                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 24
                    width: Math.min(parent.width - 48, 600)

                    Text {
                        text: getSelectedItemTitle()
                        color: theme.textMain
                        font.pixelSize: theme.fontXxl
                        font.bold: true
                        horizontalAlignment: Text.AlignHCenter
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                    }

                    Text {
                        text: {
                            if (!appState.selected) return "";
                            if (appState.selected.volumeId) return "当前选中：分卷";
                            if (appState.selected.projectId) return "当前选中：作品";
                            return "";
                        }
                        color: theme.textDim
                        font.pixelSize: theme.fontLg
                        horizontalAlignment: Text.AlignHCenter
                        Layout.fillWidth: true
                    }

                    RowLayout {
                        spacing: 16
                        Layout.alignment: Qt.AlignHCenter

                        Button {
                            text: "新建卷"
                            visible: appState.selected && appState.selected.projectId && !appState.selected.volumeId
                            onClicked: {
                                console.log("[LinuxCreateVolumeQml] open dialog via dashboard");
                                inputDialog.actionType = "volume";
                                inputDialog.projectId = appState.selected.projectId;
                                inputDialog.title = "新建卷";
                                inputDialog.open();
                            }
                        }

                        Button {
                            text: "新建章节"
                            visible: {
                                if (!appState.selected) return false;
                                if (appState.selected.volumeId) return true;
                                if (appState.selected.projectId) {
                                    var vols = getProjectVolumes(appState.selected.projectId);
                                    return vols.length > 0;
                                }
                                return false;
                            }
                            onClicked: {
                                var projId = appState.selected.projectId;
                                var volId = appState.selected.volumeId;
                                if (!volId) {
                                    var vols = getProjectVolumes(projId);
                                    if (vols.length > 0) {
                                        volId = vols[0].id;
                                    }
                                }
                                if (volId) {
                                    console.log("[LinuxCreateChapterQml] open dialog via dashboard: projectId=" + projId + ", volumeId=" + volId);
                                    inputDialog.actionType = "chapter";
                                    inputDialog.projectId = projId;
                                    inputDialog.volumeId = volId;
                                    inputDialog.title = "新建章节";
                                    inputDialog.open();
                                }
                            }
                        }
                    }
                }
            }

            // Fallback empty view when absolutely nothing is selected
            Rectangle {
                anchors.fill: parent
                color: "transparent"
                visible: appState.hasWorkspace && (!appState.selected || (!appState.selected.projectId && !appState.selected.volumeId && !appState.selected.chapterId))

                Text {
                    anchors.centerIn: parent
                    text: "请在左侧选择或创建一个作品/章节"
                    color: theme.textDim
                    font.pixelSize: theme.fontLg
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
        onSubmitProject: function(title) {
            var trimmedTitle = title ? title.trim() : "";
            var isEmpty = (trimmedTitle === "");
            console.log("[LinuxCreateProjectQml] submit titleLength=" + (title ? title.length : 0) + ", isEmpty=" + isEmpty);
            
            var stateStr = "";
            try {
                stateStr = backend.create_project_json(title);
            } catch (e) {
                console.error("[LinuxCreateProjectQml] backend call failed: " + e);
                errorDialog.message = "后端调用失败: " + e;
                errorDialog.open();
                return;
            }

            var res;
            try {
                res = JSON.parse(stateStr);
            } catch (e) {
                console.error("[LinuxCreateProjectQml] JSON.parse failed. Raw response: " + stateStr + ", error: " + e);
                errorDialog.message = "解析后端返回数据失败";
                errorDialog.open();
                return;
            }

            console.log("[LinuxCreateProjectQml] backend returned success=" + res.success + ", userMessage=" + (res.userMessage || res.message || ""));
            
            if (res.success) {
                applyState(res.state);
                createProjectDialog.close();
            } else {
                errorDialog.message = res.userMessage || res.message || "创建失败";
                errorDialog.open();
                console.error("创建失败:", res.userMessage || res.message);
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
        title: "同步设置"
        width: Math.max(360, Math.min(window.width - 80, 720))
        height: Math.max(420, Math.min(window.height - 120, 560))
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: theme.bgDark; border.color: theme.border; radius: 8; border.width: 1 }
        
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
                height: Math.max(syncDialogScroll.availableHeight, implicitHeight)
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
                            console.log("[LinuxCreateVolumeQml] submit: projectId=" + inputDialog.projectId + ", title=" + title);
                            var stateStr = "";
                            try {
                                stateStr = backend.create_volume_json(inputDialog.projectId, title);
                            } catch (e) {
                                console.error("[LinuxCreateVolumeQml] backend call failed: " + e);
                                errorDialog.message = "后端调用失败: " + e;
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            var res;
                            try {
                                res = JSON.parse(stateStr);
                            } catch (e) {
                                console.error("[LinuxCreateVolumeQml] JSON.parse failed. Raw response: " + stateStr + ", error: " + e);
                                errorDialog.message = "解析后端返回数据失败";
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            console.log("[LinuxCreateVolumeQml] backend returned success=" + res.success + ", message=" + (res.message || ""));
                            if (res.success) {
                                applyState(res.state);
                            } else {
                                errorDialog.message = res.message || "创建卷失败";
                                errorDialog.open();
                            }
                        } else if (inputDialog.actionType === "chapter") {
                            console.log("[LinuxCreateChapterQml] submit: projectId=" + inputDialog.projectId + ", volumeId=" + inputDialog.volumeId + ", title=" + title);
                            var stateStr = "";
                            try {
                                stateStr = backend.create_chapter_json(inputDialog.projectId, inputDialog.volumeId, title);
                            } catch (e) {
                                console.error("[LinuxCreateChapterQml] backend call failed: " + e);
                                errorDialog.message = "后端调用失败: " + e;
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            var res;
                            try {
                                res = JSON.parse(stateStr);
                            } catch (e) {
                                console.error("[LinuxCreateChapterQml] JSON.parse failed. Raw response: " + stateStr + ", error: " + e);
                                errorDialog.message = "解析后端返回数据失败";
                                errorDialog.open();
                                inputDialog.close();
                                return;
                            }

                            console.log("[LinuxCreateChapterQml] backend returned success=" + res.success + ", message=" + (res.message || ""));
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

