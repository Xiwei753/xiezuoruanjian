// =============================================================================
// StarMapWorkspace.qml — 星图工作区
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图编辑工作区，包含工具栏和递归场景
//
// Issue #805 评论 5907045450 第 1 部分：删掉"下钻换整页 graph"的模型。
//   不再维护页面式状态: starmapPathStack, currentStarmapId, currentStarmapTitle,
//   enterChildStarmap(), returnToParentStarmap(), Canvas 的 canDrillUp,
//   onDrillDownRequested/onDrillUpRequested。顶层 Workspace 只保存根星图身份
//   并放一个根 StarMapScene。左上返回只退出星图工作区。
//
// 约束：
//   - 纯 UI 层，业务逻辑通过 StarMapScene 和 StarMapInspector 委托
//   - 保留原有属性和信号，供 main.qml 引用
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
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

    Rectangle {
        anchors.fill: parent
        color: dt.bg

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // 顶部工具栏：返回 + 标题 + 星图操作
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
                            // Issue #805 评论 5907045450 第 1 部分：
                            // 左上返回只退出星图工作区，不再返回父星图。
                            // 递归渲染由 StarMapScene 处理，没有页面栈。
                            root.backClicked()
                        }
                    }

                    AppText {
                        dt: root.dt
                        // 标题用根星图标题（外部输入）
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

            // 主体：递归场景占满（Inspector 改为按需编辑浮层，Issue #791 评论 5883783849）
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                // Issue #805 评论 5907045450 第 1/2 部分：放一个根 StarMapScene。
                // StarMapScene 递归渲染：每个 Embed 内部用 Loader 创建下一层
                // StarMapScene，每个 Scene 自己有 viewport 和手势状态。
                StarMapScene {
                    id: rootScene
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    dt: root.dt
                    starmapBackendRef: root.starmapBackendRef
                    rootStarmapId: root.starmapId
                    pathSegments: []
                    pathKey: "root"

                    // Issue #805 评论 5908703621 问题 5：editNodeRequested 带
                    // owner 上下文（ownerStarmapId/ownerPathKey），设置 Inspector
                    // 的 owner 后 open，更新/删除回到 owner Scene。
                    // Issue #805 评论 5912394108：ownerScene 是真正拥有该节点的
                    // Scene 引用，存到 inspectorPopup 供 Inspector 回写直接调用，
                    // 不再写死 rootScene，确保第二层及更深节点回写打到对应子 Scene。
                    onEditNodeRequested: function(ownerScene, ownerStarmapId, ownerPathKey, node) {
                        inspectorPopup.ownerScene = ownerScene
                        inspectorPopup.ownerStarmapId = ownerStarmapId
                        inspectorPopup.ownerPathKey = ownerPathKey
                        inspectorPopup.selectedNode = node
                        inspectorPopup.open()
                    }
                }
            }
        }

        // Issue #791 评论 5883783849: 按需编辑浮层，不常驻不挤画布
        Popup {
            id: inspectorPopup
            modal: true
            focus: true
            width: 360
            height: 360
            anchors.centerIn: parent

            // Issue #805 评论 5908703621 问题 5：Inspector 绑定 owner 上下文，
            // 更新/删除回到 owner Scene，不再写死根 starmapId。
            // Issue #805 评论 5912394108：ownerScene 持有真正拥有该节点的 Scene
            // 引用，Inspector 回写直接调 ownerScene.updateNodeFromInspector /
            // deleteNodeFromInspector，路由到对应子 Scene 的 Controller。
            property var ownerScene: null
            property string ownerStarmapId: ""
            property string ownerPathKey: ""
            property var selectedNode: null

            contentItem: StarMapInspector {
                dt: root.dt
                // Issue #805 评论 5908703621 问题 5：Inspector 用 ownerStarmapId，
                // 不再用根 starmapId。递归渲染后节点编辑回到 owner 层。
                starmapId: inspectorPopup.ownerStarmapId
                selectedNode: inspectorPopup.selectedNode
                selectedEdge: null

                onNodeUpdated: function(nodeId, patch) {
                    // Issue #805 评论 5912394108：用 ownerScene 直接回写到真正拥有
                    // 该节点的 Scene，StarMapScene 暴露 updateNodeFromInspector
                    // 转发到内部 Canvas。不再写死 rootScene，第二层及更深节点的
                    // 更新打到对应子 Scene 的 Controller。
                    if (inspectorPopup.ownerScene)
                        inspectorPopup.ownerScene.updateNodeFromInspector(nodeId, patch)
                }
                onNodeDeleted: function(nodeId) {
                    // Issue #805 评论 5912394108：用 ownerScene 直接删除真正拥有
                    // 该节点的 Scene 中的节点，StarMapScene 暴露
                    // deleteNodeFromInspector 转发到内部 Canvas。
                    if (inspectorPopup.ownerScene)
                        inspectorPopup.ownerScene.deleteNodeFromInspector(nodeId)
                    inspectorPopup.close()
                }
            }

            onClosed: {
                // Issue #805 评论 5912394108：关闭时清掉 ownerScene 引用，
                // 避免持有已销毁的子 Scene（Loader 切换 active 时 child Scene
                // 可能被回收），防止下次 open 误用到旧引用。
                ownerScene = null
                ownerStarmapId = ""
                ownerPathKey = ""
                selectedNode = null
            }
        }
    }
}
