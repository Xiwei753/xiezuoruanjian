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
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property var starmapBackendRef: null
    property var starMapController: null
    property var appState: ({})
    property var starmaps: []
    property string filterProjectId: ""

    signal openStarmap(string starmapId, string title)

    color: Kirigami.Theme.backgroundColor

    // 刷新星图列表：通过 StarMapController 拉取后端数据
    function refreshStarmaps() {
        if (!starMapController) return
        // Issue #798: 直接用真实星图对象列表驱动 GridView，
        // 不再维护 ListModel + __idx 双模型。
        root.starmaps = starMapController.listStarmaps() || []
    }

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
            spacing: Kirigami.Units.largeSpacing
            Layout.fillWidth: true
            Layout.fillHeight: true

            Kirigami.Heading {
                level: 2
                text: qsTr("星图")
                color: Kirigami.Theme.textColor
                Layout.alignment: Qt.AlignVCenter
            }

            Item { Layout.fillWidth: true }

            Button {
                text: qsTr("新建星图")
                icon.name: "list-add"
                highlighted: true
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
            dataModel: root.starmaps
            cardHeight: 180
            minCardWidth: 260
            emptyTitle: qsTr("暂无星图")
            emptySubtitle: qsTr("点击「新建星图」开始构建")

            delegate: Item {
                id: cardWrapper
                // Issue #798: Qt 6.11 GridView 显式 required property 模型契约。
                required property var modelData
                required property int index
                width: GridView.view.gridRoot.cardWidth
                height: GridView.view.gridRoot.cardHeight

                StarMapCard {
                    anchors.fill: parent
                    dt: root.dt
                    starmapData: cardWrapper.modelData || ({})
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
        background: Rectangle { color: Kirigami.Theme.alternateBackgroundColor; border.color: Kirigami.Theme.disabledTextColor; radius: Kirigami.Units.cornerRadius; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                level: 4
                text: qsTr("新建星图")
                color: Kirigami.Theme.textColor
            }
            TextField {
                id: createField
                Layout.fillWidth: true
                text: createDialog.starmapTitle
                placeholderText: qsTr("星图标题")
                onAccepted: createConfirmButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                Button {
                    text: qsTr("取消")
                    flat: true
                    onClicked: createDialog.close()
                }
                Button {
                    id: createConfirmButton
                    text: qsTr("确定")
                    highlighted: true
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
        background: Rectangle { color: Kirigami.Theme.alternateBackgroundColor; border.color: Kirigami.Theme.disabledTextColor; radius: Kirigami.Units.cornerRadius; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                level: 4
                text: qsTr("重命名星图")
                color: Kirigami.Theme.textColor
            }
            TextField {
                id: renameField
                Layout.fillWidth: true
                text: renameDialog.currentTitle
                placeholderText: qsTr("星图标题")
                onAccepted: renameConfirmButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                Button {
                    text: qsTr("取消")
                    flat: true
                    onClicked: renameDialog.close()
                }
                Button {
                    id: renameConfirmButton
                    text: qsTr("确定")
                    highlighted: true
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
        background: Rectangle { color: Kirigami.Theme.alternateBackgroundColor; border.color: Kirigami.Theme.disabledTextColor; radius: Kirigami.Units.cornerRadius; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                level: 4
                text: qsTr("删除星图")
                color: Kirigami.Theme.textColor
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: qsTr("确定删除「%1」吗？此操作不可撤销。").arg(deleteDialog.starmapTitle)
                color: Kirigami.Theme.disabledTextColor
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                Button {
                    text: qsTr("取消")
                    flat: true
                    onClicked: deleteDialog.close()
                }
                Button {
                    text: qsTr("删除")
                    palette.buttonText: Kirigami.Theme.negativeTextColor
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
