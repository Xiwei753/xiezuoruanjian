import QtQuick 2.15
import QtQuick.Controls 2.15

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})
    property var starmaps: []
    property string filterProjectId: ""

    signal openStarmap(string starmapId, string title)

    color: dt ? dt.bg : "#111318"

    function loadStarmaps() {
        if (!backendRef) return
        try {
            starmaps = JSON.parse(backendRef.list_starmaps_json()) || []
        } catch (e) {
            starmaps = []
        }
        refreshGridModel()
    }

    function refreshGridModel() {
        var roots = getRootStarmaps()
        gridModel.clear()
        for (var i = 0; i < roots.length; i++) {
            gridModel.append({ starmapObj: roots[i], childCount: roots[i].child_starmap_count || 0 })
        }
    }

    function getRootStarmaps() {
        var roots = []
        for (var i = 0; i < starmaps.length; i++) {
            if (!starmaps[i].parent_starmap_id && (filterProjectId === "" || starmaps[i].project_id === filterProjectId)) roots.push(starmaps[i])
        }
        return roots
    }

    Component.onCompleted: loadStarmaps()
    onFilterProjectIdChanged: refreshGridModel()
    onStarmapsChanged: refreshGridModel()
    onVisibleChanged: if (visible) loadStarmaps()

    CardCollectionPage {
        anchors.fill: parent
        dt: root.dt
        title: "星图"
        subtitle: "构建你的创作宇宙，可视化人物关系与故事脉络"
        actionText: "+ 新建星图"
        model: gridModel
        cardHeight: 260
        minCardWidth: 280
        emptyIcon: "⭐"
        emptyTitle: "还没有星图"
        emptySubtitle: "创建你的第一个星图，构建角色关系与故事脉络"
        onActionClicked: createStarmapDialog.open()

        delegate: Item {
            width: GridView.view.gridRoot.cardWidth
            height: GridView.view.gridRoot.cardHeight

            Column {
                anchors.fill: parent
                spacing: dt ? dt.sp8 : 8

                StarMapCard {
                    dt: root.dt
                    starmapData: starmapObj
                    width: parent.width
                    height: 184
                    onClicked: function(starmapId, title) { root.openStarmap(starmapId, title) }
                    onMenuRequested: function(starmapId, title) {
                        starmapContextMenu.starmapId = starmapId
                        starmapContextMenu.starmapTitle = title
                        starmapContextMenu.popup()
                    }
                }

                Flow {
                    width: parent.width
                    spacing: dt ? dt.sp4 : 4
                    visible: childCount > 0

                    Repeater {
                        model: {
                            var children = []
                            for (var i = 0; i < root.starmaps.length; i++) {
                                if (root.starmaps[i].parent_starmap_id === starmapObj.starmap_id) children.push(root.starmaps[i])
                            }
                            return children
                        }

                        Rectangle {
                            width: Math.min(120, parent ? parent.width : 120)
                            height: 28
                            radius: dt ? dt.radiusSm : 8
                            color: dt ? dt.cardHover : "#22262E"
                            border.color: dt ? dt.border : "#2A2E36"
                            border.width: 1
                            Text {
                                anchors.centerIn: parent
                                text: modelData.title || ""
                                color: dt ? dt.textSecondary : "#9CA0AB"
                                font.pixelSize: dt ? dt.fontXs : 11
                                elide: Text.ElideRight
                                width: parent.width - 12
                                horizontalAlignment: Text.AlignHCenter
                            }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.openStarmap(modelData.starmap_id || "")
                            }
                        }
                    }
                }
            }
        }
    }

    ListModel { id: gridModel }

    Dialog {
        id: createStarmapDialog
        title: "新建星图"
        modal: true
        width: 360
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt ? dt.surface : "#1A1D23"; border.color: dt ? dt.border : "#2A2E36"; radius: dt ? dt.radiusMd : 12; border.width: 1 }
        Column {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp20 : 20
            spacing: dt ? dt.sp12 : 12
            TextField { id: starmapTitleField; width: parent.width; placeholderText: "例如：人物关系图" }
            TextField { id: starmapDescField; width: parent.width; placeholderText: "简要描述这个星图的内容" }
            Button {
                text: "创建"
                anchors.right: parent.right
                onClicked: {
                    var title = starmapTitleField.text.trim()
                    if (title === "") return
                    backendRef.create_starmap_json(title, starmapDescField.text.trim(), "")
                    createStarmapDialog.close()
                    starmapTitleField.text = ""
                    starmapDescField.text = ""
                    loadStarmaps()
                }
            }
        }
    }

    Menu {
        id: starmapContextMenu
        property string starmapId: ""
        property string starmapTitle: ""
        MenuItem { text: "打开"; onTriggered: root.openStarmap(starmapContextMenu.starmapId) }
        MenuItem { text: "新建子星图"; onTriggered: { createChildStarmapDialog.parentId = starmapContextMenu.starmapId; createChildStarmapDialog.open() } }
        MenuSeparator {}
        MenuItem { text: "重命名"; onTriggered: { renameStarmapDialog.starmapId = starmapContextMenu.starmapId; renameStarmapDialog.currentTitle = starmapContextMenu.starmapTitle; renameStarmapDialog.open() } }
        MenuItem { text: "删除"; onTriggered: { backendRef.delete_starmap_json(starmapContextMenu.starmapId); loadStarmaps() } }
    }

    Dialog {
        id: renameStarmapDialog
        property string starmapId: ""
        property string currentTitle: ""
        title: "重命名星图"
        modal: true
        width: 320
        anchors.centerIn: Overlay.overlay
        Column {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 8
            TextField { id: renameField; width: parent.width; text: renameStarmapDialog.currentTitle }
            Button { text: "确定"; anchors.right: parent.right; onClicked: { var t = renameField.text.trim(); if (t === "") return; backendRef.rename_starmap_json(renameStarmapDialog.starmapId, t); renameStarmapDialog.close(); loadStarmaps() } }
        }
    }

    Dialog {
        id: createChildStarmapDialog
        property string parentId: ""
        title: "新建子星图"
        modal: true
        width: 320
        anchors.centerIn: Overlay.overlay
        Column {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 8
            TextField { id: childTitleField; width: parent.width; placeholderText: "子星图名称" }
            Button { text: "创建"; anchors.right: parent.right; onClicked: { var t = childTitleField.text.trim(); if (t === "") return; backendRef.create_child_starmap_json(createChildStarmapDialog.parentId, t, "", ""); createChildStarmapDialog.close(); childTitleField.text = ""; loadStarmaps() } }
        }
    }
}
