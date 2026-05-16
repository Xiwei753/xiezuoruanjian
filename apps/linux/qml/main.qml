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
        onWorkspaceOpened: reloadTree()
        onProjectsReloaded: reloadTree()
    }

    function reloadTree() {
        treeModel.clear();
        let items = backend.getTreeModel();
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
        if (backend.saveStatus === "未保存") {
            backend.saveCurrentChapter(editorArea.text);
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
                onClicked: backend.openWorkspaceDialog()
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            ToolButton {
                text: "新建作品"
                onClicked: backend.createNewProject()
                contentItem: Text {
                    text: parent.text
                    color: "white"
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
            ToolButton {
                text: "保存"
                onClicked: backend.saveCurrentChapter(editorArea.text)
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
                text: backend.saveStatus
                color: "white"
                Layout.minimumWidth: 100
            }
            Label {
                id: wordCountLabel
                text: "字数: " + backend.wordCount
                color: "white"
                Layout.minimumWidth: 100
            }
            Item { Layout.fillWidth: true }
            Label {
                id: workspacePathLabel
                text: backend.workspacePath
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
                                if (backend.saveStatus === "未保存") {
                                    backend.saveCurrentChapter(editorArea.text);
                                }
                                treeView.currentIndex = index;
                                let node = treeModel.get(index);
                                if (node.type === "project") {
                                    backend.selectProject(node.id);
                                } else if (node.type === "volume") {
                                    backend.selectVolume(node.projectId, node.id);
                                } else if (node.type === "chapter") {
                                    backend.selectChapter(node.projectId, node.volumeId, node.id);
                                    editorArea.text = backend.getChapterContent(node.projectId, node.volumeId, node.id);
                                    backend.saveStatus = "已保存";
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
                                        backend.createNewVolume(model.id);
                                    } else if (model.type === "volume") {
                                        backend.createNewChapter(model.projectId, model.id);
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
                        backend.wordCount = text.length;
                        backend.saveStatus = "未保存";
                    }
                }
            }
        }
    }
}
