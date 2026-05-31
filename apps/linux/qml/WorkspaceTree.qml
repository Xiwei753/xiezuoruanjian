// =============================================================================
// WorkspaceTree.qml — 工作区侧栏树
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
// 职责：卷/章树形展示、右键菜单、新建/重命名/删除操作入口
// 约束：
//   - 纯 UI 组件，业务逻辑通过 signal 传递给 WritingWorkspace
//   - 不直接操作文件系统或 Core 层
//   - 使用 ListModel 绑定树数据
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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

    color: theme ? theme.sidebar : "#14161B"

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
                
                color: isSelected ? (theme ? theme.primaryContainer : "#CCE5FF") :
                       isHovered ? (theme ? theme.surfaceVariant : "#DFE3EB") : "transparent"

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

                    AppText {
                        text: {
                            if (model.type === "project") return "[P]";
                            if (model.type === "volume") return "■";
                            if (model.type === "chapter") return "●";
                            return "";
                        }
                        color: delegateRect.isSelected ? (root.theme ? root.theme.onPrimaryContainer : "#CCE5FF") : (root.theme ? root.theme.textSecondary : "#8C9198")
                        font.pixelSize: 14
                    }

                    AppText {
                        Layout.fillWidth: true
                        text: model.title || ""
                        color: delegateRect.isSelected ? (root.theme ? root.theme.onPrimaryContainer : "#CCE5FF") : (root.theme ? root.theme.textPrimary : "#E2E2E5")
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

    AppText {
        anchors.centerIn: parent
        visible: treeModel.count === 0
        text: qsTr("暂无作品")
        color: theme ? theme.textMuted : "#8C9198"
        font.pixelSize: 14
    }

    Menu {
        id: contextMenu
        property var itemData: null
        background: Rectangle {
            color: theme ? theme.surface : "#1A1D23"
            border.color: theme ? theme.border : "#2A2E36"
            radius: theme ? theme.radiusMd : 12
            border.width: 1
        }

        MenuItem {
            id: menuCreateVolume
            text: qsTr("新建卷")
            visible: contextMenu.itemData && contextMenu.itemData.type === "project"
            contentItem: AppText {
                text: menuCreateVolume.text
                color: theme ? theme.textPrimary : "#E2E2E5"
                font.pixelSize: theme ? theme.label : 13
                font.family: theme ? theme.fontFamily : "sans-serif"
                verticalAlignment: Text.AlignVCenter
            }
            background: Rectangle { color: menuCreateVolume.highlighted ? (theme ? theme.surfaceVariant : "#DFE3EB") : "transparent" }
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
            id: menuCreateChapter
            text: qsTr("新建章节")
            visible: contextMenu.itemData && contextMenu.itemData.type === "volume"
            contentItem: AppText {
                text: menuCreateChapter.text
                color: theme ? theme.textPrimary : "#E2E2E5"
                font.pixelSize: theme ? theme.label : 13
                font.family: theme ? theme.fontFamily : "sans-serif"
                verticalAlignment: Text.AlignVCenter
            }
            background: Rectangle { color: menuCreateChapter.highlighted ? (theme ? theme.surfaceVariant : "#DFE3EB") : "transparent" }
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
            id: menuRename
            text: qsTr("重命名")
            contentItem: AppText {
                text: menuRename.text
                color: theme ? theme.textPrimary : "#E2E2E5"
                font.pixelSize: theme ? theme.label : 13
                font.family: theme ? theme.fontFamily : "sans-serif"
                verticalAlignment: Text.AlignVCenter
            }
            background: Rectangle { color: menuRename.highlighted ? (theme ? theme.surfaceVariant : "#DFE3EB") : "transparent" }
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
                        root.showError(qsTr("重命名失败：缺失节点 ID"));
                        return;
                    }
                    root.renameItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);
                }
            }
        }
        MenuItem {
            id: menuDelete
            text: qsTr("删除")
            contentItem: AppText {
                text: menuDelete.text
                color: theme ? theme.error : "#FFB4AB"
                font.pixelSize: theme ? theme.label : 13
                font.family: theme ? theme.fontFamily : "sans-serif"
                verticalAlignment: Text.AlignVCenter
            }
            background: Rectangle { color: menuDelete.highlighted ? (theme ? theme.surfaceVariant : "#DFE3EB") : "transparent" }
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
                        root.showError(qsTr("删除失败：缺失项目 ID"));
                        return;
                    }
                    if (data.type === "volume" && (!data.projectIdForAction || !data.volumeIdForAction)) {
                        if (typeof window !== "undefined" && typeof window.debugError === "function") {
                            window.debugError("tree", "delete_failed", "missing volume ids");
                        }
                        root.showError(qsTr("删除失败：缺失卷的归属 ID"));
                        return;
                    }
                    if (data.type === "chapter" && (!data.projectIdForAction || !data.volumeIdForAction || !data.chapterIdForAction)) {
                        if (typeof window !== "undefined" && typeof window.debugError === "function") {
                            window.debugError("tree", "delete_failed", "missing chapter ids");
                        }
                        root.showError(qsTr("删除失败：缺失章节的归属 ID"));
                        return;
                    }
                    root.deleteItem(data.type, data.projectIdForAction, data.volumeIdForAction, data.chapterIdForAction, data.title);
                }
            }
        }
    }
}
