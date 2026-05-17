import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import QtQuick.Window 2.15
import QtQuick.Dialogs 1.3
import WriterApp 1.0

ApplicationWindow {
    id: window
    onClosing: {
        saveCurrentIfNeeded();
    }
    visible: true
    width: 1024
    height: 768
    title: "Writer"
    color: "#1e1e1e" // Dark theme

    AppBackend {
        id: backend
        onWorkspace_opened: reloadTree()
        onProjects_reloaded: reloadTree()
        onError_occurred: {
            errorDialog.open();
        }
        onClear_editor: {
            editorArea.text = "";
            backend.save_status = "未选择章节";
        }
    }

    function saveCurrentIfNeeded() {
        if (backend.save_status === "未保存" && backend.has_selected_chapter() && backend.selected_chapter_exists()) {
            backend.save_current_chapter(editorArea.text);
        }
    }

    function canMoveUp(index) {
        let node = treeModel.get(index);
        for (let i = 0; i < treeModel.count; i++) {
            let sib = treeModel.get(i);
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                if (sib.id === node.id) return i !== index; // If it's the first sibling, index === i so we can't move up. Wait, this loop always finds the first one.
            }
        }
        return false;
    }

    function isFirstSibling(index) {
        let node = treeModel.get(index);
        for (let i = 0; i < index; i++) {
            let sib = treeModel.get(i);
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                return false;
            }
        }
        return true;
    }

    function isLastSibling(index) {
        let node = treeModel.get(index);
        for (let i = index + 1; i < treeModel.count; i++) {
            let sib = treeModel.get(i);
            if (sib.type === node.type && sib.projectId === node.projectId && sib.volumeId === node.volumeId) {
                return false;
            }
        }
        return true;
    }

    function reloadTree() {
        treeModel.clear();
        let items = backend.get_tree_model();
        let selId = backend.selected_item_id;
        let matchIndex = -1;
        for (let i = 0; i < items.length; i++) {
            let item = items[i];
            treeModel.append({
                "title": item.title,
                "id": item.id,
                "projectId": item.projectId || "",
                "volumeId": item.volumeId || "",
                "type": item.type
            });
            if (selId !== "" && item.id === selId) {
                matchIndex = i;
            }
        }
        if (matchIndex !== -1) {
            treeView.currentIndex = matchIndex;
        }

        if (treeModel.count === 0) {
            treeView.currentIndex = -1;
        }
    }



    Dialog {
        id: inputDialog
        x: (parent.width - width) / 2
        y: (parent.height - height) / 2
        width: 300
        title: "输入"
        standardButtons: Dialog.Ok | Dialog.Cancel

        property string actionType: ""
        property var contextData: ({})

        ColumnLayout {
            anchors.fill: parent
            TextField {
                id: inputField
                Layout.fillWidth: true
                focus: true
                onAccepted: inputDialog.accept()
            }
        }

        onOpened: {
            inputField.text = contextData.initialText || "";
            inputField.forceActiveFocus();
        }

        onAccepted: {
            if (inputField.text.trim() === "") {
                return;
            }
            saveCurrentIfNeeded();
            if (actionType === "create_project") {
                backend.create_new_project(inputField.text);
            } else if (actionType === "create_volume") {
                backend.create_new_volume(contextData.projectId, inputField.text);
            } else if (actionType === "create_chapter") {
                backend.create_new_chapter(contextData.projectId, contextData.volumeId, inputField.text);
            } else if (actionType === "rename_project") {
                backend.rename_project(contextData.id, inputField.text);
            } else if (actionType === "rename_volume") {
                backend.rename_volume(contextData.projectId, contextData.id, inputField.text);
            } else if (actionType === "rename_chapter") {
                backend.rename_chapter(contextData.projectId, contextData.volumeId, contextData.id, inputField.text);
            }
        }
    }

    MessageDialog {
        id: errorDialog
        title: "错误"
        buttons: MessageDialog.Ok
        text: backend.error_message
    }

    MessageDialog {
        id: confirmDialog
        title: "确认删除"
        buttons: MessageDialog.Yes | MessageDialog.No

        property string actionType: ""
        property var contextData: ({})

        onButtonClicked: function(button, role) {
            if (role === MessageDialog.YesRole) {
                if (actionType === "delete_project") {
                    backend.delete_project(contextData.id);
                } else if (actionType === "delete_volume") {
                    backend.delete_volume(contextData.projectId, contextData.id);
                } else if (actionType === "delete_chapter") {
                    let wasSelected = (backend.selected_item_id === contextData.id);
                    backend.delete_chapter(contextData.projectId, contextData.volumeId, contextData.id);
                    if (wasSelected) {
                        editorArea.text = "";
                        backend.save_status = "已保存";
                    }
                }
            }
        }
    }

    ListModel {
        id: treeModel
    }

    header: ToolBar {
        background: Rectangle { color: "#2d2d2d" }
        RowLayout {
            anchors.fill: parent
            ToolButton {
                text: "打开/创建工作区"
                onClicked: backend.open_workspace_dialog()
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            ToolButton {
                text: "新建作品"
                onClicked: { inputDialog.actionType = "create_project"; inputDialog.contextData = { initialText: "新作品" }; inputDialog.open(); }
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            ToolButton {
                text: "保存"
                onClicked: backend.save_current_chapter(editorArea.text)
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            Item { Layout.fillWidth: true }
        }
    }

    footer: ToolBar {
        background: Rectangle { color: "#2d2d2d" }
        RowLayout {
            anchors.fill: parent
            anchors.margins: 5
            Label {
                id: statusLabel
                text: backend.save_status
                color: "white"
                Layout.minimumWidth: 100
            }
            Label {
                id: wordCountLabel
                text: "字数: " + backend.word_count
                color: "white"
                Layout.minimumWidth: 100
            }
            Item { Layout.fillWidth: true }
            Label {
                text: backend.chapter_path
                color: "gray"
                Layout.minimumWidth: 100
            }
            Label {
                id: workspacePathLabel
                text: backend.workspace_path
                color: "gray"
            }
        }
    }

    SplitView {
        anchors.fill: parent
        orientation: Qt.Horizontal

        Rectangle {
            SplitView.preferredWidth: 250
            SplitView.minimumWidth: 150
            color: "#252526"

            ListView {
                id: treeView
                anchors.fill: parent
                model: treeModel

                Text {
                    anchors.centerIn: parent
                    text: "未选择作品"
                    color: "gray"
                    visible: treeModel.count === 0
                }

                delegate: Item {
                    width: parent.width
                    height: 30
                    Rectangle {
                        anchors.fill: parent
                        color: treeView.currentIndex === index ? "#37373d" : "transparent"
                        MouseArea {
                            anchors.fill: parent
                            onClicked: {

                                treeView.currentIndex = index;
                                let node = treeModel.get(index);
                                saveCurrentIfNeeded();
                                if (node.type === "project") {
                                    backend.select_project(node.id);
                                    editorArea.text = "";
                                    backend.save_status = "已保存";
                                } else if (node.type === "volume") {
                                    backend.select_volume(node.projectId, node.id);
                                    editorArea.text = "";
                                    backend.save_status = "已保存";
                                } else if (node.type === "chapter") {
                                    backend.select_chapter(node.projectId, node.volumeId, node.id);
                                    editorArea.text = backend.get_chapter_content(node.projectId, node.volumeId, node.id);
                                    backend.save_status = "已保存";
                                }
                            }
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: (model.type === "project" ? 5 : model.type === "volume" ? 20 : 35)
                            Text {
                                text: model.title
                                color: "white"
                                Layout.fillWidth: true
                            }

                            ToolButton {
                                visible: treeView.currentIndex === index
                                text: "⋮"
                                onClicked: contextMenu.open()
                                contentItem: Text { text: parent.text; color: "white"; font.pixelSize: 18 }

                                Menu {
                                    id: contextMenu
                                    MenuItem {
                                        text: model.type === "project" ? "新建分卷" : "新建章节"
                                        visible: model.type !== "chapter"
                                        onTriggered: {
                                            if (model.type === "project") {
                                                inputDialog.actionType = "create_volume";
                                                inputDialog.contextData = { projectId: model.id, initialText: "新分卷" };
                                                inputDialog.open();
                                            } else if (model.type === "volume") {
                                                inputDialog.actionType = "create_chapter";
                                                inputDialog.contextData = { projectId: model.projectId, volumeId: model.id, initialText: "新章节" };
                                                inputDialog.open();
                                            }
                                        }
                                    }
                                    MenuItem {
                                        text: "重命名"
                                        onTriggered: {
                                            inputDialog.actionType = "rename_" + model.type;
                                            inputDialog.contextData = { id: model.id, projectId: model.projectId, volumeId: model.volumeId, initialText: model.title.trim() };
                                            inputDialog.open();
                                        }
                                    }
                                    MenuItem {
                                        text: "删除"
                                        onTriggered: {
                                            if (model.type === "volume") {
                                                confirmDialog.text = "确定要删除此分卷及其包含的所有章节吗？";
                                            } else {
                                                confirmDialog.text = "确定要删除吗？";
                                            }
                                            confirmDialog.actionType = "delete_" + model.type;
                                            confirmDialog.contextData = { id: model.id, projectId: model.projectId, volumeId: model.volumeId };
                                            confirmDialog.open();
                                        }
                                    }
                                    MenuItem {
                                        text: "上移"
                                        visible: !isFirstSibling(index)
                                        onTriggered: {
                                            let ids = [];
                                            let my_pos = -1;
                                            for (let i = 0; i < treeModel.count; i++) {
                                                let node = treeModel.get(i);
                                                if (node.type === model.type && node.projectId === model.projectId && node.volumeId === model.volumeId) {
                                                    if (node.id === model.id) my_pos = ids.length;
                                                    ids.push(node.id);
                                                }
                                            }
                                            if (my_pos > 0) {
                                                let temp = ids[my_pos];
                                                ids[my_pos] = ids[my_pos - 1];
                                                ids[my_pos - 1] = temp;

                                                if (model.type === "project") backend.reorder_projects(ids.join(","));
                                                else if (model.type === "volume") backend.reorder_volumes(model.projectId, ids.join(","));
                                                else if (model.type === "chapter") backend.reorder_chapters(model.projectId, model.volumeId, ids.join(","));
                                            }
                                        }
                                    }
                                    MenuItem {
                                        text: "下移"
                                        visible: !isLastSibling(index)
                                        onTriggered: {
                                            let ids = [];
                                            let my_pos = -1;
                                            for (let i = 0; i < treeModel.count; i++) {
                                                let node = treeModel.get(i);
                                                if (node.type === model.type && node.projectId === model.projectId && node.volumeId === model.volumeId) {
                                                    if (node.id === model.id) my_pos = ids.length;
                                                    ids.push(node.id);
                                                }
                                            }
                                            if (my_pos < ids.length - 1) {
                                                let temp = ids[my_pos];
                                                ids[my_pos] = ids[my_pos + 1];
                                                ids[my_pos + 1] = temp;

                                                if (model.type === "project") backend.reorder_projects(ids.join(","));
                                                else if (model.type === "volume") backend.reorder_volumes(model.projectId, ids.join(","));
                                                else if (model.type === "chapter") backend.reorder_chapters(model.projectId, model.volumeId, ids.join(","));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Rectangle {
            SplitView.fillWidth: true
            color: "#1e1e1e"

            ScrollView {
                anchors.fill: parent
                anchors.margins: 20

                Timer {
                    id: autoSaveTimer
                    interval: 1500 // 1.5 seconds
                    repeat: false
                    onTriggered: {
                        if (backend.save_status === "未保存") {
                            backend.save_current_chapter(editorArea.text);
                        }
                    }
                }


                Text {
                    anchors.centerIn: parent
                    text: "请在左侧选择或创建一个章节"
                    color: "gray"
                    visible: !backend.selected_item_id || backend.selected_item_id === ""
                }

                TextArea {
                    id: editorArea
                    color: "#d4d4d4"
                    font.pixelSize: 16
                    wrapMode: TextArea.Wrap
                    background: null
                    enabled: backend.has_selected_chapter()
                    onTextChanged: {
                        backend.calculate_word_count(text);
                        if (backend.has_selected_chapter() && backend.save_status !== "未保存") {
                            backend.save_status = "未保存";
                        }
                        if (backend.has_selected_chapter()) {
                            autoSaveTimer.restart();
                        }
                    }
                }
            }
        }
    }
}
