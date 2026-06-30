// =============================================================================
// StarMapPage.qml — 星图列表页
// =============================================================================
//
// 层级：Desktop UI 层（QML 页面）
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
    property var starMapController: null
    property var appState: ({})
    property var starmaps: []
    property string filterProjectId: ""

    signal openStarmap(string starmapId, string title)

    color: dt.bg

    function loadStarmaps() {
        if (!root.starMapController) {
            starmaps = []
            refreshGridModel()
            return
        }
        try {
            starmaps = root.starMapController.listStarmaps() || []
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
                spacing: dt.sp8

                StarMapCard {
                    id: starMapCard
                    dt: root.dt
                    starmapData: starmapObj
                    width: parent.width
                    height: 184

                    // 漂浮动画
                    property real floatOffset: 0
                    SequentialAnimation on floatOffset {
                        loops: Animation.Infinite
                        NumberAnimation { to: 2; duration: 2100; easing.type: Easing.InOutSine }
                        NumberAnimation { to: -2; duration: 2100; easing.type: Easing.InOutSine }
                    }

                    transform: Translate { y: starMapCard.floatOffset }

                    onClicked: function(starmapId, title) { root.openStarmap(starmapId, title) }
                    onMenuRequested: function(starmapId, title) {
                        starmapContextMenu.starmapId = starmapId
                        starmapContextMenu.starmapTitle = title
                        starmapContextMenu.popup()
                    }
                }

                Flow {
                    width: parent.width
                    spacing: dt.sp4
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
                            radius: dt.radiusPill
                            color: dt.surfaceVariant
                            border.color: dt.border
                            border.width: 1

                            // chip 漂浮动画
                            property real chipFloatOffset: 0
                            SequentialAnimation on chipFloatOffset {
                                loops: Animation.Infinite
                                NumberAnimation { to: 1; duration: 2800; easing.type: Easing.InOutSine }
                                NumberAnimation { to: -1; duration: 2800; easing.type: Easing.InOutSine }
                            }

                            transform: Translate { y: chipFloatOffset }

                            AppText {
                                anchors.centerIn: parent
                                text: modelData.title || ""
                                color: dt.onSurfaceVariant
                                font.pixelSize: dt.caption
                                font.family: dt.fontFamily
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
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp12

            AppText {
                text: qsTr("新建星图")
                color: dt.onSurface
                font.pixelSize: dt.subtitle
                font.family: dt.fontFamily
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
                    dt: dt
                    variant: "text"
                    onClicked: createStarmapDialog.close()
                }
                AppButton {
                    id: createStarmapButton
                    text: qsTr("创建")
                    dt: dt
                    variant: "primary"
                    onClicked: {
                        var title = starmapTitleField.text.trim()
                        if (title === "") return
                        if (root.starMapController && root.starMapController.createStarmap(title, starmapDescField.text.trim())) {
                            createStarmapDialog.close()
                            starmapTitleField.text = ""
                            starmapDescField.text = ""
                            loadStarmaps()
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
        MenuItem { text: qsTr("打开"); onTriggered: root.openStarmap(starmapContextMenu.starmapId, starmapContextMenu.starmapTitle) }
        MenuItem { text: qsTr("新建子星图"); onTriggered: { createChildStarmapDialog.parentId = starmapContextMenu.starmapId; createChildStarmapDialog.open() } }
        MenuSeparator {}
        MenuItem { text: qsTr("重命名"); onTriggered: { renameStarmapDialog.starmapId = starmapContextMenu.starmapId; renameStarmapDialog.currentTitle = starmapContextMenu.starmapTitle; renameStarmapDialog.open() } }
        MenuItem { text: qsTr("删除"); onTriggered: { if (root.starMapController && root.starMapController.deleteStarmap(starmapContextMenu.starmapId)) loadStarmaps() } }
    }

    Dialog {
        id: renameStarmapDialog
        property string starmapId: ""
        property string currentTitle: ""
        modal: true
        width: 360
        height: 208
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp12
            AppText { text: qsTr("重命名星图"); color: dt.onSurface; font.pixelSize: dt.subtitle; font.family: dt.fontFamily; font.weight: Font.DemiBold }
            AppTextField { id: renameField; Layout.fillWidth: true; theme: dt; text: renameStarmapDialog.currentTitle; placeholderText: qsTr("星图名称"); onAccepted: renameStarmapButton.clicked() }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton { text: qsTr("取消"); dt: dt; variant: "text"; onClicked: renameStarmapDialog.close() }
                AppButton { id: renameStarmapButton; text: qsTr("确定"); dt: dt; variant: "primary"; onClicked: { var t = renameField.text.trim(); if (t === "") return; if (root.starMapController && root.starMapController.renameStarmap(renameStarmapDialog.starmapId, t)) { renameStarmapDialog.close(); loadStarmaps() } } }
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
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp12
            AppText { text: qsTr("新建子星图"); color: dt.onSurface; font.pixelSize: dt.subtitle; font.family: dt.fontFamily; font.weight: Font.DemiBold }
            AppTextField { id: childTitleField; Layout.fillWidth: true; theme: dt; placeholderText: qsTr("子星图名称"); onAccepted: createChildStarmapButton.clicked() }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton { text: qsTr("取消"); dt: dt; variant: "text"; onClicked: createChildStarmapDialog.close() }
                AppButton { id: createChildStarmapButton; text: qsTr("创建"); dt: dt; variant: "primary"; onClicked: { var t = childTitleField.text.trim(); if (t === "") return; if (root.starMapController && root.starMapController.createChildStarmap(createChildStarmapDialog.parentId, t, "")) { createChildStarmapDialog.close(); childTitleField.text = ""; loadStarmaps() } } }
            }
        }
    }
}
