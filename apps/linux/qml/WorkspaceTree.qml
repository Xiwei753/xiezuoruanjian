import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var items: []
    property string selectedId: ""
    property var theme: null

    signal itemActivated(string type, string projectId, string volumeId, string chapterId)
    signal createVolume(string projectId)
    signal createChapter(string projectId, string volumeId)
    signal renameItem(string type, string projectId, string volumeId, string chapterId, string currentTitle)
    signal deleteItem(string type, string projectId, string volumeId, string chapterId, string title)

    color: theme ? theme.sidebarBg : "#2D2D2D"

    ListModel {
        id: treeModel
    }

    onItemsChanged: {
        treeModel.clear();
        if (items) {
            for (let i = 0; i < items.length; i++) {
                treeModel.append(items[i]);
            }
        }
    }

    ScrollView {
        anchors.fill: parent
        contentWidth: parent.width
        clip: true

        ListView {
            id: listView
            width: parent.width
            model: treeModel
            
            delegate: Rectangle {
                id: delegateRect
                width: ListView.view.width
                height: 32
                
                                property var itemData: {
                    var out = {
                        "id": model.id || "",
                        "type": model.type || "",
                        "title": model.title || "",
                        "projectId": model.projectId || "",
                        "volumeId": model.volumeId || ""
                    };
                    if (out.type === "project") {
                        out.projectIdForAction = out.id;
                        out.volumeIdForAction = "";
                        out.chapterIdForAction = "";
                    } else if (out.type === "volume") {
                        out.projectIdForAction = out.projectId;
                        out.volumeIdForAction = out.id;
                        out.chapterIdForAction = "";
                    } else if (out.type === "chapter") {
                        out.projectIdForAction = out.projectId;
                        out.volumeIdForAction = out.volumeId;
                        out.chapterIdForAction = out.id;
                    }
                    return out;
                }

                property bool isSelected: root.selectedId !== "" && root.selectedId === model.id
                property bool isHovered: hoverArea.containsMouse
                
                color: isSelected ? (theme ? theme.sidebarHover : "#404040") :
                       isHovered ? (theme ? theme.sidebarHover : "#383838") : "transparent"

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: {
                        if (model.type === "project") return 8;
                        if (model.type === "volume") return 24;
                        if (model.type === "chapter") return 40;
                        return 8;
                    }
                    anchors.rightMargin: 8
                    spacing: 8

                    Text {
                        text: {
                            if (model.type === "project") return "📚";
                            if (model.type === "volume") return "📁";
                            if (model.type === "chapter") return "📄";
                            return "";
                        }
                        color: root.theme ? root.theme.textMain : "#E0E0E0"
                        font.pixelSize: 14
                    }

                    Text {
                        Layout.fillWidth: true
                        text: model.title || ""
                        color: delegateRect.isSelected ? (root.theme ? root.theme.accent : "#82AAFF") : (root.theme ? root.theme.textMain : "#E0E0E0")
                        font.pixelSize: 14
                        elide: Text.ElideRight
                    }
                }

                MouseArea {
                    id: hoverArea
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    onClicked: function(mouse) {
                        if (mouse.button === Qt.LeftButton) {
                            root.itemActivated(delegateRect.itemData.type, delegateRect.itemData.projectIdForAction, delegateRect.itemData.volumeIdForAction, delegateRect.itemData.chapterIdForAction);
                        } else if (mouse.button === Qt.RightButton) {
                            contextMenu.itemData = delegateRect.itemData;
                            contextMenu.popup();
                        }
                    }
                }
            }
        }
    }

    Text {
        anchors.centerIn: parent
        visible: treeModel.count === 0
        text: "暂无作品"
        color: theme ? theme.textDim : "#808080"
        font.pixelSize: 14
    }

    Menu {
        id: contextMenu
        property var itemData: null

        MenuItem {
            text: "新建卷"
            visible: contextMenu.itemData && contextMenu.itemData.type === "project"
            onTriggered: {
                if (contextMenu.itemData) {
                    root.createVolume(contextMenu.itemData.id);
                }
            }
        }
        MenuItem {
            text: "新建章节"
            visible: contextMenu.itemData && contextMenu.itemData.type === "volume"
            onTriggered: {
                if (contextMenu.itemData) {
                    root.createChapter(contextMenu.itemData.projectId, contextMenu.itemData.id);
                }
            }
        }
        MenuSeparator {
            visible: contextMenu.itemData && (contextMenu.itemData.type === "project" || contextMenu.itemData.type === "volume")
        }
        MenuItem {
            text: "重命名"
            onTriggered: {
                if (contextMenu.itemData) {
                    var data = contextMenu.itemData;
                    if (!data || !data.id) { console.error("Missing node ID"); return; }
                    root.renameItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);
                }
            }
        }
        MenuItem {
            text: "删除"
            onTriggered: {
                if (contextMenu.itemData) {
                    var data = contextMenu.itemData;
                    if (!data || !data.id) { console.error("Missing node ID"); return; }
                    root.deleteItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);
                }
            }
        }
    }
}
