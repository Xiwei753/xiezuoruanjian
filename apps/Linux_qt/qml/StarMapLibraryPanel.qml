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

    signal assetActivated(string kind, string assetId, string title, string scopeJson, string targetJson)
    signal copyRequested(string targetJson, string scopeJson, string title)
    signal libraryChanged()

    implicitHeight: contentColumn.implicitHeight
    color: "transparent"

    StarmapLibraryModel {
        id: libraryModel
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
        var lines = []
        for (var i = 0; i < refs.length; ++i) {
            var ref = refs[i] || {}
            lines.push(String(ref.projectId || qsTr("全局"))
                       + " / " + String(ref.starmapId || "")
                       + " / " + String(ref.instanceId || ""))
        }
        referencesDialog.referenceText = lines.length > 0
                ? lines.join("\n")
                : qsTr("没有画布引用")
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

                Drag.active: dragArea.drag.active
                Drag.hotSpot.x: dragArea.mouseX
                Drag.hotSpot.y: dragArea.mouseY
                Drag.mimeData: delegateRoot.rowKind === "folder"
                               ? {}
                               : {
                                     "application/x-sujian-starmap-asset": JSON.stringify({
                                         "kind": delegateRoot.rowKind,
                                         "assetId": delegateRoot.assetId,
                                         "entryId": delegateRoot.entryId,
                                         "title": delegateRoot.title,
                                         "scopeJson": delegateRoot.scopeJson,
                                         "targetJson": delegateRoot.targetJson
                                     })
                                 }

                MouseArea {
                    id: dragArea
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    drag.target: delegateRoot.rowKind === "folder" ? null : delegateRoot

                    onClicked: function(mouse) {
                        if (delegateRoot.rowKind === "folder") {
                            if (mouse.button === Qt.LeftButton)
                                libraryModel.toggle_folder(delegateRoot.folderId)
                            return
                        }
                        if (mouse.button === Qt.LeftButton) {
                            root.showReferences(delegateRoot.targetJson,
                                                delegateRoot.title)
                        } else if (mouse.button === Qt.RightButton) {
                            contextMenu.rowKind = delegateRoot.rowKind
                            contextMenu.assetId = delegateRoot.assetId
                            contextMenu.title = delegateRoot.title
                            contextMenu.targetJson = delegateRoot.targetJson
                            contextMenu.scopeJson = delegateRoot.scopeJson
                            contextMenu.entryId = delegateRoot.entryId
                            contextMenu.popup()
                        }
                    }

                    onDoubleClicked: {
                        if (delegateRoot.rowKind !== "folder") {
                            root.assetActivated(delegateRoot.rowKind,
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
            if (!payload || !payload.entryId || !payload.scopeJson
                    || payload.scopeJson === root.scopeJson)
                return
            root.starmapBackendRef.move_starmap_library_entry_json(
                        String(payload.entryId), root.scopeJson, "")
            root.reload()
            root.libraryChanged()
        }
    }

    Menu {
        id: contextMenu
        property string rowKind: ""
        property string assetId: ""
        property string title: ""
        property string targetJson: ""
        property string scopeJson: ""
        property string entryId: ""

        MenuItem {
            text: qsTr("插入当前画布")
            onTriggered: root.assetActivated(contextMenu.rowKind,
                                              contextMenu.assetId,
                                              contextMenu.title,
                                              contextMenu.scopeJson,
                                              contextMenu.targetJson)
        }
        MenuItem {
            text: qsTr("复制")
            onTriggered: root.copyRequested(contextMenu.targetJson,
                                            contextMenu.scopeJson,
                                            contextMenu.title)
        }
        MenuItem {
            text: root.otherScopeJson === "" ? qsTr("移动归属")
                                             : qsTr("移动到另一素材库")
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
            onTriggered: root.showReferences(contextMenu.targetJson, contextMenu.title)
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("删除素材本体")
            onTriggered: root.removeAsset(contextMenu.targetJson, contextMenu.title)
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
                        root.scopeJson, "", nameField.text)
            nameField.text = ""
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
        property string referenceText: ""
        title: referenceTitle
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Close
        Label {
            text: referencesDialog.referenceText
            wrapMode: Text.Wrap
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
