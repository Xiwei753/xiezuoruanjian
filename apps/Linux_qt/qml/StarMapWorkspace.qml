// =============================================================================
// StarMapWorkspace.qml — 星图工作区
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图编辑工作区，包含工具栏、画布和检查器
// 约束：
//   - 纯 UI 层，业务逻辑通过 StarMapCanvas 和 StarMapInspector 委托
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

    // Issue #801 评论 5894035036: 层级状态从 Canvas 收口到 Workspace。
    //   starmapPathStack 记录从根星图到当前层的路径，每项 { starmapId, title }。
    //   currentStarmapId / currentStarmapTitle 是 Canvas 实际渲染的星图。
    //   根 starmapId / starmapTitle 只作为外部输入（来自 main.qml / AppController），
    //   不再被 Canvas 内部反向赋值打断。
    property var starmapPathStack: []
    property string currentStarmapId: ""
    property string currentStarmapTitle: qsTr("星图")

    // Issue #801 评论 5897793716: 完整层级身份路径段。
    // currentPathSegments 记录从根星图到当前层的所有穿越段
    // （EnterEmbed{instanceId} 或 EnterPortal{nodeId}），根层为 []。
    // 同一个 StarMap 被两个不同 Embed 实例嵌入时，下钻后实例身份不再丢失。
    // currentStarmapId 继续作为当前实际加载图，但不再充当完整层级身份。
    property var currentPathSegments: []

    signal backClicked()
    signal enterStarmapRequested(string starmapId, string title)
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
                            // Issue #801 评论 5894035036: 优先返回父星图，
                            // 根星图（路径栈空）时才退出工作区回 Hub。
                            if (root.starmapPathStack.length > 0) {
                                root.returnToParentStarmap()
                            } else {
                                root.backClicked()
                            }
                        }
                    }

                    AppText {
                        dt: root.dt
                        // Issue #801 评论 5894035036: 顶部标题用 currentStarmapTitle，
                        // 不再读 root.starmapTitle（那是外部输入，下钻时不更新）。
                        text: root.currentStarmapTitle
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

            // 主体：画布占满（Inspector 改为按需编辑浮层，Issue #791 评论 5883783849）
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                StarMapCanvas {
                    id: canvas
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    dt: root.dt
                    // Issue #801 评论 5894035036: Canvas 的 starmapId 只读绑定到
                    // Workspace 的 currentStarmapId，Canvas 不再自己赋值 starmapId。
                    starmapId: root.currentStarmapId
                    starmapBackendRef: root.starmapBackendRef
                    // Issue #801 评论 5894639734: 告知 Canvas 当前是否有父级可返回，
                    // 根星图时 canDrillUp=false，缩到最小只 clamp 不 drillUp。
                    canDrillUp: root.starmapPathStack.length > 0

                    // Issue #801 评论 5894035036: Canvas 上抛下钻/上钻请求，
                    // 由 Workspace 统一管理层级栈。
                    onDrillDownRequested: function(smId, smTitle, segment) {
                        root.enterChildStarmap(smId, smTitle, segment)
                    }
                    onDrillUpRequested: {
                        root.returnToParentStarmap()
                    }
                    onEnterStarmapRequested: function(smId, smTitle) {
                        // Issue #801 评论 5894035036: 双击 portal/Embed 时 Canvas
                        // 仍上抛此信号用于标题同步；层级切换已由 drillDownRequested
                        // 触发，这里只更新 currentStarmapTitle。
                        root.currentStarmapTitle = smTitle
                    }
                    onEditNodeRequested: function(node) {
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

            property var selectedNode: null

            contentItem: StarMapInspector {
                dt: root.dt
                // Issue #801 评论 5894035036: Inspector 用 currentStarmapId，
                // 跟随层级栈切换，不再读根 starmapId。
                starmapId: root.currentStarmapId
                selectedNode: inspectorPopup.selectedNode
                selectedEdge: null

                onNodeUpdated: function(nodeId, patch) {
                    canvas.updateNodeFromInspector(nodeId, patch)
                }
                onNodeDeleted: function(nodeId) {
                    canvas.deleteNodeFromInspector(nodeId)
                    inspectorPopup.close()
                }
            }

            onClosed: {
                selectedNode = null
            }
        }
    }

    // 星图切换时先清瞬时交互状态再重新加载。
    // Issue #798: 返回父图 / 进入子图复用同一个 Workspace 实例，
    // 必须经过 resetInteraction 清掉旧 move/connect 状态，否则新图会继承旧 pointerMode。
    // Issue #801 评论 5894035036: 根 starmapId/starmapTitle 变化时（外部切换星图），
    // 重置层级栈，currentStarmapId/Title 同步成根，并触发 Canvas 重新加载。
    // Issue #801 评论 5894639734: 只改 currentStarmapId/Title 和层级栈，
    // reset+reload 统一由 Canvas.onStarmapIdChanged 负责，避免重复 loadGraph。
    onStarmapIdChanged: {
        currentStarmapId = starmapId
        currentStarmapTitle = starmapTitle
        starmapPathStack = []
        currentPathSegments = []
    }

    // Issue #801 评论 5894035036: 外部标题变化时（如 AppController 更新），
    // 若还在根星图，同步到 currentStarmapTitle。
    onStarmapTitleChanged: {
        if (starmapPathStack.length === 0) {
            currentStarmapTitle = starmapTitle
        }
    }

    // Issue #801 评论 5894639734: 只设 current，Canvas.onStarmapIdChanged 会触发
    // 首次 reset+loadGraph（currentStarmapId 从 "" 变成 starmapId 时）。
    Component.onCompleted: {
        currentStarmapId = starmapId
        currentStarmapTitle = starmapTitle
    }

    // Issue #801 评论 5894035036: 下钻到子星图——push 当前层到栈，切换 current。
    // Issue #801 评论 5894639734: 只改栈和 current，Canvas.onStarmapIdChanged 负责 reset+reload。
    function enterChildStarmap(targetId, title, segment) {
        // Issue #801 评论 5894981235: QML var 原地 push 不触发 change notification，
        // 必须重新赋值数组才能让 starmapPathStackChanged 发出，
        // canDrillUp binding 才会跟着层级正确更新。
        // Issue #801 评论 5897793716: 栈元素保存进入本层的 segment，
        // currentPathSegments 同步 append，保留完整层级身份。
        starmapPathStack = starmapPathStack.concat([
            { starmapId: currentStarmapId, title: currentStarmapTitle, segment: segment }
        ])
        currentPathSegments = currentPathSegments.concat([segment])
        currentStarmapId = targetId
        currentStarmapTitle = title
    }

    // Issue #801 评论 5894035036: 返回父星图——pop 栈并切换 current。
    // Issue #801 评论 5894639734: 只改栈和 current，Canvas.onStarmapIdChanged 负责 reset+reload。
    function returnToParentStarmap() {
        if (starmapPathStack.length === 0)
            return
        // Issue #801 评论 5894981235: QML var 原地 pop 不触发 change notification，
        // 必须重新赋值数组才能让 starmapPathStackChanged 发出，
        // canDrillUp binding 才会跟着层级正确更新。
        // Issue #801 评论 5897793716: currentPathSegments 同步 pop。
        var parent = starmapPathStack[starmapPathStack.length - 1]
        starmapPathStack = starmapPathStack.slice(0, starmapPathStack.length - 1)
        currentPathSegments = currentPathSegments.slice(0, currentPathSegments.length - 1)
        currentStarmapId = parent.starmapId
        currentStarmapTitle = parent.title
    }
}
