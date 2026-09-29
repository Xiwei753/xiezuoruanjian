// =============================================================================
// StarMapPage.qml — 星图列表页
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图列表展示、新建/重命名/删除操作，点击卡片进入星图工作区
// 约束：
//   - 纯展示层，业务逻辑通过 StarMapController 委托给后端
//   - 保留原有属性和信号，供其他 QML 文件引用
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt
    property var starmapBackendRef: null
    property var starMapController: null
    property var appState: ({})
    property var starmaps: []
    property string filterProjectId: ""

    signal openStarmap(string starmapId, string title)

    color: dt.bg

    // 刷新星图列表：通过 StarMapController 拉取后端数据
    function refreshStarmaps() {
        if (!starMapController) return
        var list = starMapController.listStarmaps() || []
        starmapModel.clear()
        for (var i = 0; i < list.length; i++) {
            // ListModel 仅用于驱动网格数量与 count 判断；
            // 完整数据通过 root.starmaps[index] 在 delegate 中取回。
            starmapModel.append({ __idx: i })
        }
        root.starmaps = list
    }

    ListModel { id: starmapModel }

    Component.onCompleted: refreshStarmaps()
    onVisibleChanged: if (visible) refreshStarmaps()
    onStarMapControllerChanged: refreshStarmaps()

    HubPageFrame {
        id: pageFrame
        anchors.fill: parent
        dt: root.dt

        // 顶部 header：标题 + 新建星图按钮
        // Issue #796 评论 5886483653: 删除一级星图页自己的"← 返回"按钮。
        // 它已经在 Hub 的一级 Tab 里，当前 root.visible=false 只会把自己藏掉，属于错误路由。
        headerData: RowLayout {
            spacing: dt.sp12
            Layout.fillWidth: true
            Layout.fillHeight: true

            AppText {
                dt: root.dt
                text: qsTr("星图")
                color: dt.onSurface
                font.pointSize: dt.fontTitlePt
                font.family: dt.fontFamily
                font.weight: Font.Bold
                Layout.alignment: Qt.AlignVCenter
            }

            Item { Layout.fillWidth: true }

            AppButton {
                dt: root.dt
                variant: "primary"
                text: qsTr("+ 新建星图")
                onClicked: {
                    createDialog.starmapTitle = ""
                    createDialog.open()
                }
            }
        }

        // 内容区：响应式卡片网格
        contentData: HubContentGrid {
            dt: root.dt
            Layout.fillWidth: true
            Layout.fillHeight: true
            dataModel: starmapModel
            cardHeight: 180
            minCardWidth: 260
            emptyTitle: qsTr("暂无星图")
            emptySubtitle: qsTr("点击「新建星图」开始构建")

            delegate: Item {
                id: cardWrapper
                width: GridView.view.gridRoot.cardWidth
                height: GridView.view.gridRoot.cardHeight

                StarMapCard {
                    anchors.fill: parent
                    dt: root.dt
                    starmapData: root.starmaps[index] || ({})
                    onClicked: function(smId, smTitle) {
                        root.openStarmap(smId, smTitle)
                    }
                    onMenuRequested: function(smId, smTitle) {
                        starmapContextMenu.starmapId = smId
                        starmapContextMenu.starmapTitle = smTitle
                        starmapContextMenu.popup()
                    }
                }
            }
        }
    }

    // 右键菜单：打开 / 重命名 / 删除
    Menu {
        id: starmapContextMenu
        property string starmapId: ""
        property string starmapTitle: ""

        MenuItem {
            text: qsTr("打开")
            onTriggered: root.openStarmap(starmapContextMenu.starmapId, starmapContextMenu.starmapTitle)
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("重命名")
            onTriggered: {
                renameDialog.starmapId = starmapContextMenu.starmapId
                renameDialog.currentTitle = starmapContextMenu.starmapTitle
                renameDialog.open()
            }
        }
        MenuItem {
            text: qsTr("删除")
            onTriggered: {
                deleteDialog.starmapId = starmapContextMenu.starmapId
                deleteDialog.starmapTitle = starmapContextMenu.starmapTitle
                deleteDialog.open()
            }
        }
    }

    // 新建星图对话框
    Dialog {
        id: createDialog
        property string starmapTitle: ""
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

            AppText {
                dt: root.dt
                text: qsTr("新建星图")
                color: dt.onSurface
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
            AppTextField {
                id: createField
                Layout.fillWidth: true
                dt: root.dt
                text: createDialog.starmapTitle
                placeholderText: qsTr("星图标题")
                onAccepted: createConfirmButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    dt: root.dt
                    variant: "text"
                    onClicked: createDialog.close()
                }
                AppButton {
                    id: createConfirmButton
                    text: qsTr("确定")
                    dt: root.dt
                    variant: "primary"
                    onClicked: {
                        var t = createField.text.trim()
                        if (t === "") return
                        if (root.starMapController && root.starMapController.createStarmap(t, "")) {
                            root.refreshStarmaps()
                        }
                        createDialog.close()
                    }
                }
            }
        }
        onOpened: {
            createField.text = createDialog.starmapTitle
            createField.forceActiveFocus()
        }
    }

    // 重命名对话框
    Dialog {
        id: renameDialog
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

            AppText {
                dt: root.dt
                text: qsTr("重命名星图")
                color: dt.onSurface
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
            AppTextField {
                id: renameField
                Layout.fillWidth: true
                dt: root.dt
                text: renameDialog.currentTitle
                placeholderText: qsTr("星图标题")
                onAccepted: renameConfirmButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    dt: root.dt
                    variant: "text"
                    onClicked: renameDialog.close()
                }
                AppButton {
                    id: renameConfirmButton
                    text: qsTr("确定")
                    dt: root.dt
                    variant: "primary"
                    onClicked: {
                        var t = renameField.text.trim()
                        if (t === "") return
                        if (root.starMapController && root.starMapController.renameStarmap(renameDialog.starmapId, t)) {
                            root.refreshStarmaps()
                        }
                        renameDialog.close()
                    }
                }
            }
        }
        onOpened: {
            renameField.text = renameDialog.currentTitle
            renameField.forceActiveFocus()
        }
    }

    // 删除确认对话框
    Dialog {
        id: deleteDialog
        property string starmapId: ""
        property string starmapTitle: ""
        modal: true
        width: 360
        height: 180
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp12

            AppText {
                dt: root.dt
                text: qsTr("删除星图")
                color: dt.onSurface
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
            AppText {
                dt: root.dt
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: qsTr("确定删除「%1」吗？此操作不可撤销。").arg(deleteDialog.starmapTitle)
                color: dt.onSurfaceVariant
                font.pointSize: dt.bodyPt
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    dt: root.dt
                    variant: "text"
                    onClicked: deleteDialog.close()
                }
                AppButton {
                    text: qsTr("删除")
                    dt: root.dt
                    variant: "danger"
                    onClicked: {
                        if (root.starMapController && root.starMapController.deleteStarmap(deleteDialog.starmapId)) {
                            root.refreshStarmaps()
                        }
                        deleteDialog.close()
                    }
                }
            }
        }
    }
}
