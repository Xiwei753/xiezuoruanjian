import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import WriterApp 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 1024
    height: 768
    title: "Writer"
    color: "#1e1e1e" // Dark theme

    AppBackend {
        id: backend
        onWorkspace_opened: reloadTree()
        onProjects_reloaded: reloadTree()
    }

    function reloadTree() {
        treeModel.clear();
        let items = backend.get_tree_model();
        for (let i = 0; i < items.length; i++) {
            let item = items[i];
            treeModel.append({
                "title": item.title,
                "id": item.id,
                "projectId": item.projectId || "",
                "volumeId": item.volumeId || "",
                "type": item.type
            });
        }
    }

    onClosing: function(close_event) {
        if (backend.save_status === "未保存") {
            backend.save_current_chapter(editorArea.text);
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
                onClicked: backend.create_new_project()
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
                delegate: Item {
                    width: parent.width
                    height: 30
                    Rectangle {
                        anchors.fill: parent
                        color: treeView.currentIndex === index ? "#37373d" : "transparent"
                        MouseArea {
                            anchors.fill: parent
                            onClicked: {
                                if (backend.save_status === "未保存") {
                                    backend.save_current_chapter(editorArea.text);
                                }
                                treeView.currentIndex = index;
                                let node = treeModel.get(index);
                                if (node.type === "project") {
                                    backend.select_project(node.id);
                                } else if (node.type === "volume") {
                                    backend.select_volume(node.projectId, node.id);
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
                                text: "+"
                                visible: model.type !== "chapter"
                                onClicked: {
                                    if (model.type === "project") {
                                        backend.create_new_volume(model.id);
                                    } else if (model.type === "volume") {
                                        backend.create_new_chapter(model.projectId, model.id);
                                    }
                                }
                                contentItem: Text { text: parent.text; color: "white" }
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
                TextArea {
                    id: editorArea
                    color: "#d4d4d4"
                    font.pixelSize: 16
                    wrapMode: TextArea.Wrap
                    background: null
                    onTextChanged: {
                        backend.word_count = text.length;
                        backend.save_status = "未保存";
                    }
                }
            }
        }
    }
}
