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
    signal showError(string message)
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
        if (typeof window !== "undefined" && typeof window.debugLog === "function") {
            window.debugLog("tree", "tree_model_updated", "count=" + treeModel.count);
        }
    }

    ScrollView {
        anchors.fill: parent
        clip: true

        ListView {
            id: listView
            width: parent ? parent.availableWidth : 0
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
                
                color: isSelected ? (theme ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, 0.15) : "#404040") :
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
                            if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                                window.debugLog("tree", "item_clicked", "type=" + delegateRect.itemData.type + ", id=" + delegateRect.itemData.id + ", title=" + delegateRect.itemData.title);
                            }
                            root.itemActivated(delegateRect.itemData.type, delegateRect.itemData.projectIdForAction, delegateRect.itemData.volumeIdForAction, delegateRect.itemData.chapterIdForAction);
                        } else if (mouse.button === Qt.RightButton) {
                            if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                                window.debugLog("tree", "context_menu_open", "type=" + delegateRect.itemData.type + ", id=" + delegateRect.itemData.id + ", title=" + delegateRect.itemData.title);
                            }
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
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("volume", "menu_create_volume_triggered", "projectId=" + contextMenu.itemData.id);
                    }
                    root.createVolume(contextMenu.itemData.id);
                }
            }
        }
        MenuItem {
            text: "新建章节"
            visible: contextMenu.itemData && contextMenu.itemData.type === "volume"
            onTriggered: {
                if (contextMenu.itemData) {
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("chapter", "menu_create_chapter_triggered", "projectId=" + contextMenu.itemData.projectId + ", volumeId=" + contextMenu.itemData.id);
                    }
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
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("tree", "menu_rename_triggered", "type=" + data.type + ", id=" + data.id + ", title=" + data.title);
                    }
                    if (!data || !data.id) {
                        if (typeof window !== "undefined" && typeof window.debugError === "function") {
                            window.debugError("tree", "rename_failed", "missing id");
                        }
                        root.showError("重命名失败：缺失节点 ID");
                        return;
                    }
                    root.renameItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);
                }
            }
        }
        MenuItem {
            text: "删除"
            onTriggered: {
                if (contextMenu.itemData) {
                    var data = contextMenu.itemData;
                    if (!data) return;
                    if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                        window.debugLog("tree", "menu_delete_triggered", "type=" + data.type + ", id=" + data.id + ", title=" + data.title);
                    }
                    if (data.type === "project" && !data.projectIdForAction) {
                        if (typeof window !== "undefined" && typeof window.debugError === "function") {
                            window.debugError("tree", "delete_failed", "missing project id");
                        }
                        root.showError("删除失败：缺失项目 ID");
                        return;
                    }
                    if (data.type === "volume" && (!data.projectIdForAction || !data.volumeIdForAction)) {
                        if (typeof window !== "undefined" && typeof window.debugError === "function") {
                            window.debugError("tree", "delete_failed", "missing volume ids");
                        }
                        root.showError("删除失败：缺失卷的归属 ID");
                        return;
                    }
                    if (data.type === "chapter" && (!data.projectIdForAction || !data.volumeIdForAction || !data.chapterIdForAction)) {
                        if (typeof window !== "undefined" && typeof window.debugError === "function") {
                            window.debugError("tree", "delete_failed", "missing chapter ids");
                        }
                        root.showError("删除失败：缺失章节的归属 ID");
                        return;
                    }
                    root.deleteItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);
                }
            }
        }
    }
}
