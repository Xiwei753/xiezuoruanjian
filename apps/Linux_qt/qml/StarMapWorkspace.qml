// =============================================================================
// StarMapWorkspace.qml — 星图工作区
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图编辑工作区，包含顶栏和唯一一块星图画布
//
// Issue #805 评论 5907045450 第 1 部分：删掉"下钻换整页 graph"的模型。
//
// Issue #929：独立画布协议。
//   Workspace 只创建一块 StarMapCanvas（它管理"当前活动画布"的身份、镜头和
//   返回入口）和顶栏。子星图不是递归塞进父图的坐标空间，也不是工作台维护的
//   另一层页面：进入 / 退出都由 Canvas 走 Core 导航路由完成，Workspace 只按
//   `canvas.focusIsRoot` 决定返回按钮是"退出星图工作区"还是"返回父星图"。
//   工作台不再持有任何子 Scene / 子 Content 引用，也不把焦点栈当画布身份。
//
//   Issue #822：节点标题编辑已经移进节点自身的内联编辑，删掉
//   inspectorPopup / ownerScene / ownerStarmapId / ownerPathKey 以及
//   editNodeRequested → Popup → ownerScene.updateNodeFromInspector 整条链路。
//   StarMapInspector.qml 随之删除。
//
// 约束：
//   - 纯 UI 层，业务逻辑通过 StarMapCanvas 和各画布的 StarMapGraphController 委托
//   - 保留原有属性和信号，供 main.qml 引用
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

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

    // Issue #814 评论 5935285879: 当前活动画布共享的唯一选中状态控制器。
    // 由 Workspace 创建，传给 Canvas；切换活动画布时选中身份带新的 routeKey。
    StarMapSelectionController {
        id: sharedSelectionController
    }

    Rectangle {
        anchors.fill: parent
        color: Kirigami.Theme.backgroundColor

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // 顶部工具栏：返回 + 标题 + 公共入口
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 56
                color: Kirigami.Theme.alternateBackgroundColor
                border.color: Kirigami.Theme.disabledTextColor
                border.width: 1

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.largeSpacing
                    anchors.rightMargin: Kirigami.Units.largeSpacing
                    spacing: Kirigami.Units.largeSpacing

                    ToolButton {
                        icon.name: "go-previous"
                        // Issue #832 评论 6013799805 / #373 / #929：按导航路由状态分流。
                        // focusIsRoot：退出星图工作区；否则 Core 退出路由返回父画布，
                        // 并恢复进入父画布时保存的镜头。
                        text: canvas.focusIsRoot ? qsTr("返回")
                                                 : qsTr("返回父星图")
                        display: AbstractButton.TextBesideIcon
                        onClicked: {
                            if (canvas.focusIsRoot)
                                root.backClicked()
                            else
                                canvas.focusParentScene()
                        }
                    }

                    Kirigami.Heading {
                        level: 3
                        text: root.starmapTitle
                        color: Kirigami.Theme.textColor
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
            // Canvas 管理当前活动画布的身份（Core 导航路由）与独立镜头；
            // 子星图入口在画布里只画微缩概览，进入后切换的是活动画布内容本身。
            StarMapCanvas {
                id: canvas
                Layout.fillWidth: true
                Layout.fillHeight: true
                dt: root.dt
                canvasBackgroundColor: Kirigami.Theme.backgroundColor
                starmapId: root.starmapId
                starmapBackendRef: root.starmapBackendRef
                // Issue #814 评论 5935285879: 传共享选中控制器给 Canvas。
                selectionController: sharedSelectionController

                onVisibleChanged: {
                    if (!visible)
                        canvas.resetInteraction()
                }
            }
        }
    }
}
