// =============================================================================
// StarMapWorkspace.qml — 星图工作区
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图编辑工作区，包含顶栏和唯一一块星图画布
//
// Issue #805 评论 5907045450 第 1 部分：删掉"下钻换整页 graph"的模型。
//
// Issue #822：整棵星图只有一个全局视口。
//   Workspace 只创建根 StarMapCanvas（它自己就是那个全局 viewport）、
//   整棵树共享的选中控制器和顶栏。
//   递归的是内容，不是视口：子星图由 Canvas 内部的 StarMapSceneContent
//   递归渲染，Workspace 不再持有任何子 Scene / 子 Content 引用。
//
//   Issue #822：节点标题编辑已经移进节点自身的内联编辑，删掉
//   inspectorPopup / ownerScene / ownerStarmapId / ownerPathKey 以及
//   editNodeRequested → Popup → ownerScene.updateNodeFromInspector 整条链路。
//   StarMapInspector.qml 随之删除。
//
// 约束：
//   - 纯 UI 层，业务逻辑通过 StarMapCanvas 和各层 StarMapGraphController 委托
//   - 保留原有属性和信号，供 main.qml 引用
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Item {
    id: root
    width: parent ? parent.width : 800
    height: parent ? parent.height : 600

    property string starmapId: ""
    property string starmapTitle: qsTr("星图")
    required property var dt
    property var starmapBackendRef: null
    // Issue #790 评论 5875963057: 顶栏收口后的同步/搜索/设置入口
    property var appState: ({})

    signal backClicked()
    signal requestSync()
    signal requestSearch()
    signal openSettings()

    // Issue #814 评论 5935285879: 整棵递归树共享的唯一选中状态控制器。
    // 由 Workspace 创建，传给根 Canvas，子层沿用同一个实例。
    StarMapSelectionController {
        id: sharedSelectionController
    }

    Rectangle {
        anchors.fill: parent
        color: dt.bg

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // 顶部工具栏：返回 + 标题 + 公共入口
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 56
                color: dt.surface
                border.color: dt.border
                border.width: 1

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: dt.sp16
                    anchors.rightMargin: dt.sp16
                    spacing: dt.sp12

                    AppButton {
                        dt: root.dt
                        variant: "text"
                        text: qsTr("← 返回")
                        onClicked: {
                            // 左上返回只退出星图工作区，不返回父星图。
                            // 递归渲染由 StarMapSceneContent 处理，没有页面栈。
                            root.backClicked()
                        }
                    }

                    AppText {
                        dt: root.dt
                        text: root.starmapTitle
                        color: dt.onSurface
                        font.pointSize: dt.fontLgPt
                        font.family: dt.fontFamily
                        font.weight: Font.DemiBold
                        Layout.alignment: Qt.AlignVCenter
                    }

                    Item { Layout.fillWidth: true }

                    // Issue #790 评论 5875963057: 右侧公共入口收口到 GlobalTopActions
                    GlobalTopActions {
                        dt: root.dt
                        appState: root.appState
                        onRequestSync: root.requestSync()
                        onRequestSearch: root.requestSearch()
                        onOpenSettings: root.openSettings()
                    }
                }
            }

            // 主体：唯一一块星图画布占满。
            // Canvas 就是整棵星图的全局 viewport；子星图内容在它内部递归渲染，
            // 空子星图也能直接右键新建，不用"进入"另一个页面。
            StarMapCanvas {
                id: canvas
                Layout.fillWidth: true
                Layout.fillHeight: true
                dt: root.dt
                starmapId: root.starmapId
                starmapBackendRef: root.starmapBackendRef
                // Issue #814 评论 5935285879: 传共享选中控制器给根 Canvas。
                selectionController: sharedSelectionController

                onVisibleChanged: {
                    if (!visible)
                        canvas.resetInteraction()
                }
            }
        }
    }
}
