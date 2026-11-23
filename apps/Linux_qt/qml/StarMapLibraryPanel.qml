// =============================================================================
// StarMapLibraryPanel.qml — 全局 / 当前作品素材库
// =============================================================================
//
// 层级：Linux_qt UI 层
// 职责：显示一个 scope 的素材库目录。folder 可展开；双击节点/星图把引用插入
//   当前活动画布；可直接拖到画布。右键复制、移动归属、查看全部引用或删除本体。
//
// 边界：归属、复制、引用保护全部由 Core 决定；此文件只发请求并刷新目录。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import SujianApp 1.0
import org.kde.kirigami as Kirigami

Rectangle {
    id: root

    required property var dt
    property var starmapBackendRef: null
    property string scopeJson: "{\"kind\":\"global\"}"
    property string otherScopeJson: ""
    property string header: qsTr("素材库")
    property string emptyText: qsTr("暂无素材")
    property string newFolderParentId: ""

    signal openAsset(string kind, string assetId, string title, string scopeJson, string targetJson)
    signal assetActivated(string kind, string assetId, string title, string scopeJson, string targetJson)
    signal copyRequested(string targetJson, string scopeJson, string title)
    signal referenceJumpRequested(var reference)
    signal libraryChanged()

    // Issue #1048：拖拽只移动独立的 ghost 代理，真实 ListView delegate 永远留在原位。
    // payload 在按住时快照，避免拖动过程中模型刷新导致 ghost 读取到别的行。
    property var libraryDragSource: null
    property var libraryDragPayload: ({})
    property string libraryDragTitle: ""

    function clearLibraryDragSource() {
        root.libraryDragSource = null
        root.libraryDragPayload = ({})
        root.libraryDragTitle = ""
    }

    implicitHeight: contentColumn.implicitHeight
    color: "transparent"

    StarmapLibraryModel {
        id: libraryModel
    }

    // 拖拽可视代理：只有它跟随鼠标，且只在按住拖动时存在。
    Rectangle {
        id: assetDragGhost
        z: 1000
        width: Math.min(root.width - 8, ghostLabel.implicitWidth + 18)
        height: 24
        radius: Kirigami.Units.cornerRadius
        color: Kirigami.Theme.highlightColor
        opacity: 0.92
        visible: root.libraryDragSource !== null
                 && root.libraryDragSource.drag.active

        Drag.active: visible
        Drag.source: assetDragGhost
        Drag.mimeData: root.libraryDragPayload
        Drag.hotSpot: Qt.point(8, height / 2)

        Label {
            id: ghostLabel
            anchors.centerIn: parent
            width: parent.width - 16
            text: root.libraryDragTitle
            elide: Text.ElideRight
            font.pixelSize: 11
            color: Kirigami.Theme.highlightedTextColor
        }
    }

    function reload() {
        libraryModel.set_backend_ref(root.starmapBackendRef)
        libraryModel.set_scope_json(root.scopeJson)
        libraryModel.refresh()
    }

    Component.onCompleted: reload()

    onScopeJsonChanged: reload()
    onStarmapBackendRefChanged: reload()

    function envelopeData(raw) {
        try {
            var value = JSON.parse(String(raw || "{}"))
            if (value && value.success === true)
                return value.data
        } catch (e) {
        }
        return null
    }

    function showReferences(targetJson, title) {
        if (!root.starmapBackendRef)
            return
        var refs = root.envelopeData(
                    root.starmapBackendRef.list_starmap_asset_references_json(targetJson))
        if (!Array.isArray(refs))
            refs = []
        referencesDialog.references = refs
        referencesDialog.referenceTitle = title
        referencesDialog.open()
    }

    function removeAsset(targetJson, title) {
        if (!root.starmapBackendRef)
            return
        var refs = root.envelopeData(
                    root.starmapBackendRef.list_starmap_asset_references_json(targetJson))
        if (Array.isArray(refs) && refs.length > 0) {
            root.showReferences(targetJson, title)
            return
        }
        var raw = root.starmapBackendRef.delete_starmap_library_asset_json(targetJson)
        try {
            var value = JSON.parse(String(raw || "{}"))
            if (!value || value.success !== true) {
                messageDialog.text = String(value && value.rawError
                                            ? value.rawError : qsTr("删除素材失败"))
                messageDialog.open()
                return
            }
        } catch (e) {
            return
        }
        reload()
        root.libraryChanged()
    }

    ColumnLayout {
        id: contentColumn
        width: parent.width
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 10
            Layout.rightMargin: 6
            Layout.topMargin: 6

            Label {
                text: root.header
                font.bold: true
                Layout.fillWidth: true
                elide: Text.ElideRight
                color: Kirigami.Theme.textColor
            }
            ToolButton {
                text: "+"
                font.bold: true
                implicitWidth: 24
                implicitHeight: 24
                ToolTip.text: qsTr("新建文件夹")
                ToolTip.visible: hovered
                onClicked: {
                    root.newFolderParentId = ""
                    newFolderDialog.open()
                }
            }
        }

        ListView {
            id: libraryList
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight, 220)
            clip: true
            model: libraryModel

            delegate: Rectangle {
                id: delegateRoot
                required property int index
                required property string title
                required property string subtitle
                required property string rowKind
                required property string assetId
                required property string entryId
                required property string folderId
                required property string scopeJson
                required property string targetJson
                required property int depth
                required property bool expanded
                required property bool hasChildren

                width: ListView.view.width
                height: 26
                color: rowMouse.containsMouse
                       ? Kirigami.Theme.hoverColor : "transparent"

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 4 + Math.max(0, depth) * 14
                    anchors.rightMargin: 4
                    spacing: 3

                    ToolButton {
                        visible: delegateRoot.rowKind === "folder"
                                 && delegateRoot.hasChildren
                        text: delegateRoot.expanded ? "▾" : "▸"
                        font.pixelSize: 11
                        implicitWidth: 18
                        implicitHeight: 18
                        onClicked: libraryModel.toggle_folder(delegateRoot.folderId)
                    }
                    Item {
                        visible: delegateRoot.rowKind !== "folder"
                                 || !delegateRoot.hasChildren
                        Layout.preferredWidth: 18
                    }
                    Label {
                        Layout.fillWidth: true
                        text: delegateRoot.title
                        elide: Text.ElideRight
                        font.pixelSize: 12
                        color: Kirigami.Theme.textColor
                    }
                    Label {
                        visible: delegateRoot.subtitle !== ""
                        text: delegateRoot.subtitle
                        font.pixelSize: 10
                        opacity: 0.55
                        color: Kirigami.Theme.textColor
                    }
                }

                MouseArea {
                    id: dragArea
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    // 文件夹只展开/右键；素材行拖动时移动的是 ghost，不是 delegate。
                    drag.target: delegateRoot.rowKind === "folder"
                                 ? null : assetDragGhost

                    onPressed: function(mouse) {
                        if (delegateRoot.rowKind === "folder"
                                || mouse.button !== Qt.LeftButton)
                            return
                        root.libraryDragSource = dragArea
                        root.libraryDragTitle = delegateRoot.title
                        // 与画布 DropArea 的既有 payload 协议保持一致。
                        root.libraryDragPayload = {
                            "application/x-sujian-starmap-asset": JSON.stringify({
                                "kind": delegateRoot.rowKind,
                                "assetId": delegateRoot.assetId,
                                "entryId": delegateRoot.entryId,
                                "title": delegateRoot.title,
                                "scopeJson": delegateRoot.scopeJson,
                                "targetJson": delegateRoot.targetJson
                            })
                        }
                        // ghost 从真实行位置起飞；真实行本身不移动。
                        var origin = delegateRoot.mapToItem(root, 0, 0)
                        assetDragGhost.x = Math.max(0, Math.min(
                            root.width - assetDragGhost.width, origin.x + 4))
                        assetDragGhost.y = origin.y + 2
                    }

                    // 释放/取消后延后一拍清理：先让 DropArea 完成 onDropped，
                    // 再撤掉 Drag.active 与 payload，避免取消掉本次拖放。
                    onReleased: Qt.callLater(root.clearLibraryDragSource)
                    onCanceled: Qt.callLater(root.clearLibraryDragSource)

                    onClicked: function(mouse) {
                        if (delegateRoot.rowKind === "folder") {
                            if (mouse.button === Qt.LeftButton) {
                                libraryModel.toggle_folder(delegateRoot.folderId)
                            } else if (mouse.button === Qt.RightButton) {
                                contextMenu.rowKind = "folder"
                                contextMenu.folderId = delegateRoot.folderId
                                contextMenu.entryId = delegateRoot.entryId
                                contextMenu.popup()
                            }
                            return
                        }
                        if (mouse.button === Qt.LeftButton) {
                            // 单击是“打开/定位素材本体”；插入引用只通过拖拽或右键菜单。
                            root.openAsset(delegateRoot.rowKind,
                                           delegateRoot.assetId,
                                           delegateRoot.title,
                                           delegateRoot.scopeJson,
                                           delegateRoot.targetJson)
                        } else if (mouse.button === Qt.RightButton) {
                            contextMenu.rowKind = delegateRoot.rowKind
                            contextMenu.assetId = delegateRoot.assetId
                            contextMenu.assetTitle = delegateRoot.title
                            contextMenu.targetJson = delegateRoot.targetJson
                            contextMenu.scopeJson = delegateRoot.scopeJson
                            contextMenu.entryId = delegateRoot.entryId
                            contextMenu.folderId = ""
                            contextMenu.popup()
                        }
                    }

                    onDoubleClicked: {
                        if (delegateRoot.rowKind !== "folder") {
                            root.openAsset(delegateRoot.rowKind,
                                           delegateRoot.assetId,
                                           delegateRoot.title,
                                           delegateRoot.scopeJson,
                                           delegateRoot.targetJson)
                        }
                    }
                }
            }
        }

        Label {
            visible: libraryList.count === 0
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.bottomMargin: 4
            text: root.emptyText
            font.pixelSize: 11
            opacity: 0.55
            color: Kirigami.Theme.textColor
        }
    }

    DropArea {
        anchors.fill: parent
        keys: ["application/x-sujian-starmap-asset"]
        onDropped: function(drop) {
            if (!root.starmapBackendRef || root.scopeJson === "")
                return
            var payload = {}
            try {
                payload = JSON.parse(String(drop.getDataAsString(
                            "application/x-sujian-starmap-asset") || "{}"))
            } catch (e) {
                return
            }
            if (!payload || !payload.entryId || !payload.scopeJson)
                return
            var targetFolderId = ""
            var local = libraryList.mapFromItem(root, drop.x, drop.y)
            var rowIndex = libraryList.indexAt(local.x, local.y)
            if (rowIndex >= 0) {
                var modelIndex = libraryModel.index(rowIndex, 0)
                if (String(libraryModel.data(modelIndex, 302) || "") === "folder")
                    targetFolderId = String(libraryModel.data(modelIndex, 305) || "")
            }
            // 同库拖放只用于移入/移出文件夹；跨库拖放同时改变归属 scope。
            if (payload.scopeJson === root.scopeJson && targetFolderId === "")
                return
            root.starmapBackendRef.move_starmap_library_entry_json(
                        String(payload.entryId), root.scopeJson, targetFolderId)
            root.reload()
            root.libraryChanged()
        }
    }

    Menu {
        id: contextMenu
        property string rowKind: ""
        property string assetId: ""
        property string assetTitle: ""
        property string targetJson: ""
        property string scopeJson: ""
        property string entryId: ""
        property string folderId: ""

        MenuItem {
            text: qsTr("新建子文件夹")
            visible: contextMenu.rowKind === "folder"
            onTriggered: {
                root.newFolderParentId = contextMenu.folderId
                newFolderDialog.open()
            }
        }
        MenuItem {
            text: qsTr("插入当前画布")
            visible: contextMenu.rowKind !== "folder"
            onTriggered: root.assetActivated(contextMenu.rowKind,
                                              contextMenu.assetId,
                                              contextMenu.assetTitle,
                                              contextMenu.scopeJson,
                                              contextMenu.targetJson)
        }
        MenuItem {
            text: qsTr("复制")
            visible: contextMenu.rowKind !== "folder"
            onTriggered: root.copyRequested(contextMenu.targetJson,
                                            contextMenu.scopeJson,
                                            contextMenu.assetTitle)
        }
        MenuItem {
            text: root.otherScopeJson === "" ? qsTr("移动归属")
                                             : qsTr("移动到另一素材库")
            visible: contextMenu.rowKind !== "folder"
            enabled: root.otherScopeJson !== ""
                     && contextMenu.targetJson !== ""
            onTriggered: {
                if (!root.starmapBackendRef || root.otherScopeJson === "")
                    return
                root.starmapBackendRef.move_starmap_library_entry_json(
                            contextMenu.entryId, root.otherScopeJson, "")
                root.reload()
                root.libraryChanged()
            }
        }
        MenuItem {
            text: qsTr("查看全部引用")
            visible: contextMenu.rowKind !== "folder"
            onTriggered: root.showReferences(contextMenu.targetJson, contextMenu.assetTitle)
        }
        MenuSeparator {
            visible: contextMenu.rowKind !== "folder"
        }
        MenuItem {
            text: qsTr("删除素材本体")
            visible: contextMenu.rowKind !== "folder"
            onTriggered: root.removeAsset(contextMenu.targetJson, contextMenu.assetTitle)
        }
    }

    Dialog {
        id: newFolderDialog
        title: qsTr("新建文件夹")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAccepted: {
            if (!root.starmapBackendRef)
                return
            root.starmapBackendRef.create_starmap_library_folder_json(
                        root.scopeJson, root.newFolderParentId, nameField.text)
            nameField.text = ""
            root.newFolderParentId = ""
            root.reload()
        }
        TextField {
            id: nameField
            placeholderText: qsTr("文件夹名称")
        }
    }

    Dialog {
        id: referencesDialog
        property string referenceTitle: ""
        property var references: []
        title: referenceTitle
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Close
        width: Math.min(520, Overlay.overlay ? Overlay.overlay.width - 48 : 520)
        contentItem: ListView {
            implicitHeight: Math.max(60, contentHeight)
            clip: true
            model: referencesDialog.references
            footer: Label {
                visible: referencesDialog.references.length === 0
                width: ListView.view ? ListView.view.width : 0
                text: qsTr("没有画布引用")
                wrapMode: Text.Wrap
            }
            delegate: ItemDelegate {
                width: ListView.view.width
                text: {
                    var ref = modelData || {}
                    return String(ref.projectId || qsTr("全局"))
                            + " / " + String(ref.starmapId || "")
                            + " / " + String(ref.instanceId || "")
                }
                onClicked: {
                    root.referenceJumpRequested(modelData)
                    referencesDialog.close()
                }
            }
        }
    }

    Dialog {
        id: messageDialog
        property string text: ""
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Close
        Label {
            text: messageDialog.text
            wrapMode: Text.Wrap
        }
    }
}
