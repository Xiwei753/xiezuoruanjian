// =============================================================================
// StarMapPage.qml — 星图列表页
// =============================================================================
//
// 层级：Linux UI 层（QML 页面）
// 职责：星图卡片网格展示、项目筛选、新建/重命名/删除操作入口
// 约束：
//   - 纯展示层，业务逻辑通过 signal 传递给 main.qml
//   - 不直接操作文件系统或 Core 层
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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
            starmaps = backendRef.list_starmaps() || []
        } catch (e) {
            starmaps = []
        }
        refreshGridModel()
    }

    function refreshGridModel() {
        var roots = getRootStarmaps()
        gridModel.clear()
        for (var i = 0; i < roots.length; i++) {
            gridModel.append({ starmapObj: roots[i], childCount: roots[i].childStarmapCount || 0 })
        }
    }

    function getRootStarmaps() {
        var roots = []
        for (var i = 0; i < starmaps.length; i++) {
            var parentId = starmaps[i].parentStarmapId;
            var projId = starmaps[i].projectId;
            if (!parentId && (filterProjectId === "" || projId === filterProjectId)) roots.push(starmaps[i])
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
        title: qsTr("星图")
        subtitle: gridModel.count > 0 ? qsTr("%1 个星图").arg(gridModel.count) : qsTr("构建你的创作宇宙")
        actionText: qsTr("+ 新建星图")
        model: gridModel
        cardHeight: 260
        minCardWidth: 280
        emptyTitle: qsTr("还没有星图")
        emptySubtitle: qsTr("创建你的第一个星图，构建角色关系与故事脉络")
        onActionClicked: createStarmapDialog.open()

        delegate: Item {
            id: delegateItem
            property var currentStarmap: model.starmapObj
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
                            var currentObj = delegateItem.currentStarmap
                            if (currentObj) {
                                for (var i = 0; i < root.starmaps.length; i++) {
                                    var pId = root.starmaps[i].parentStarmapId;
                                    var sId = currentObj.starmapId;
                                    if (pId === sId) children.push(root.starmaps[i])
                                }
                            }
                            return children
                        }

                        Rectangle {
                            width: Math.min(120, parent ? parent.width : 120)
                            height: 28
                            radius: dt ? dt.radiusPill : 999
                            color: dt ? dt.surfaceVariant : "#DFE3EB"
                            border.color: dt ? dt.border : "#2A2E36"
                            border.width: 1
                            AppText {
                                anchors.centerIn: parent
                                text: modelData.title || ""
                                color: dt ? dt.onSurfaceVariant : "#42474E"
                                font.pixelSize: dt ? dt.caption : 12
                                font.family: dt ? dt.fontFamily : "sans-serif"
                                elide: Text.ElideRight
                                width: parent.width - 12
                                horizontalAlignment: Text.AlignHCenter
                            }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.openStarmap(modelData.starmapId || "", modelData.title || "")
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
        modal: true
        width: 360
        height: 300
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt ? dt.surface : "#FCFCFF"; border.color: dt ? dt.border : "#CBD5E1"; radius: dt ? dt.radiusXl : 24; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp24 : 24
            spacing: dt ? dt.sp12 : 12

            AppText {
                text: qsTr("新建星图")
                color: dt ? dt.onSurface : "#E2E2E5"
                font.pixelSize: dt ? dt.subtitle : 18
                font.family: dt ? dt.fontFamily : "sans-serif"
                font.weight: Font.DemiBold
            }
            AppTextField {
                id: starmapTitleField
                Layout.fillWidth: true
                theme: dt
                label: qsTr("星图名称")
                placeholderText: qsTr("例如：人物关系图")
                onAccepted: createStarmapButton.clicked()
            }
            AppTextField {
                id: starmapDescField
                Layout.fillWidth: true
                theme: dt
                label: qsTr("描述（可选）")
                placeholderText: qsTr("简要描述这个星图的内容")
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    theme: dt
                    variant: "text"
                    onClicked: createStarmapDialog.close()
                }
                AppButton {
                    id: createStarmapButton
                    text: qsTr("创建")
                    theme: dt
                    variant: "primary"
                    onClicked: {
                        var title = starmapTitleField.text.trim()
                        if (title === "") return
                        var result = backendRef.create_starmap(title, starmapDescField.text.trim(), "")
                        if (result && result.success) {
                            createStarmapDialog.close()
                            starmapTitleField.text = ""
                            starmapDescField.text = ""
                            loadStarmaps()
                        } else {
                            console.warn("[WriterDebug] create_starmap failed:", result ? result.message : "empty result")
                        }
                    }
                }
            }
        }
    }

    Menu {
        id: starmapContextMenu
        property string starmapId: ""
        property string starmapTitle: ""
        MenuItem { text: qsTr("打开"); onTriggered: root.openStarmap(starmapContextMenu.starmapId) }
        MenuItem { text: qsTr("新建子星图"); onTriggered: { createChildStarmapDialog.parentId = starmapContextMenu.starmapId; createChildStarmapDialog.open() } }
        MenuSeparator {}
        MenuItem { text: qsTr("重命名"); onTriggered: { renameStarmapDialog.starmapId = starmapContextMenu.starmapId; renameStarmapDialog.currentTitle = starmapContextMenu.starmapTitle; renameStarmapDialog.open() } }
        MenuItem { text: qsTr("删除"); onTriggered: { backendRef.delete_starmap_json(starmapContextMenu.starmapId); loadStarmaps() } }
    }

    Dialog {
        id: renameStarmapDialog
        property string starmapId: ""
        property string currentTitle: ""
        modal: true
        width: 360
        height: 208
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt ? dt.surface : "#FCFCFF"; border.color: dt ? dt.border : "#CBD5E1"; radius: dt ? dt.radiusXl : 24; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp24 : 24
            spacing: dt ? dt.sp12 : 12
            AppText { text: qsTr("重命名星图"); color: dt ? dt.onSurface : "#E2E2E5"; font.pixelSize: dt ? dt.subtitle : 18; font.family: dt ? dt.fontFamily : "sans-serif"; font.weight: Font.DemiBold }
            AppTextField { id: renameField; Layout.fillWidth: true; theme: dt; text: renameStarmapDialog.currentTitle; placeholderText: qsTr("星图名称"); onAccepted: renameStarmapButton.clicked() }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton { text: qsTr("取消"); theme: dt; variant: "text"; onClicked: renameStarmapDialog.close() }
                AppButton { id: renameStarmapButton; text: qsTr("确定"); theme: dt; variant: "primary"; onClicked: { var t = renameField.text.trim(); if (t === "") return; backendRef.rename_starmap_json(renameStarmapDialog.starmapId, t); renameStarmapDialog.close(); loadStarmaps() } }
            }
        }
    }

    Dialog {
        id: createChildStarmapDialog
        property string parentId: ""
        modal: true
        width: 360
        height: 208
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt ? dt.surface : "#FCFCFF"; border.color: dt ? dt.border : "#CBD5E1"; radius: dt ? dt.radiusXl : 24; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt ? dt.sp24 : 24
            spacing: dt ? dt.sp12 : 12
            AppText { text: qsTr("新建子星图"); color: dt ? dt.onSurface : "#E2E2E5"; font.pixelSize: dt ? dt.subtitle : 18; font.family: dt ? dt.fontFamily : "sans-serif"; font.weight: Font.DemiBold }
            AppTextField { id: childTitleField; Layout.fillWidth: true; theme: dt; placeholderText: qsTr("子星图名称"); onAccepted: createChildStarmapButton.clicked() }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton { text: qsTr("取消"); theme: dt; variant: "text"; onClicked: createChildStarmapDialog.close() }
                AppButton { id: createChildStarmapButton; text: qsTr("创建"); theme: dt; variant: "primary"; onClicked: { var t = childTitleField.text.trim(); if (t === "") return; backendRef.create_child_starmap_legacy_json(createChildStarmapDialog.parentId, t, "", ""); createChildStarmapDialog.close(); childTitleField.text = ""; loadStarmaps() } }
            }
        }
    }
}
