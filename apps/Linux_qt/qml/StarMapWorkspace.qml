// =============================================================================
// StarMapWorkspace.qml — 星图工作区
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图编辑工作区，只承载唯一一块星图画布
//
// Issue #805 评论 5907045450 第 1 部分：删掉"下钻换整页 graph"的模型。
//
// Issue #929：独立画布协议。
//   Workspace 只创建一块 StarMapCanvas（它管理"当前活动画布"的身份、镜头和
//   返回入口）。子星图不是递归塞进父图的坐标空间，也不是工作台维护的另一层
//   页面：进入 / 退出都由 Canvas 走 Core 导航路由完成，Workspace 只按
//   `canvas.focusIsRoot` 决定返回动作是"退出星图工作区"还是"返回父星图"。
//   工作台不再持有任何子 Scene / 子 Content 引用，也不把焦点栈当画布身份。
//
// Issue #822：节点标题编辑已经移进节点自身的内联编辑，删掉 inspector 链路。
//
// Issue #977：删除本页自绘的 56px 顶栏和四边 border；返回 / 标题 / 公共入口
//   统一由 main.qml 的 DesktopTopBar 负责。Workspace 保留导航语义，暴露
//   focusIsRoot 与 navigateBack() 给壳层调用。
//
// 约束：
//   - 纯 UI 层，业务逻辑通过 StarMapCanvas 和各画布的 StarMapGraphController 委托
//   - 保留原有属性和信号，供 main.qml / 其他入口引用
// =============================================================================

import QtQuick
import org.kde.kirigami as Kirigami

Item {
    id: root
    width: parent ? parent.width : 800
    height: parent ? parent.height : 600

    property string starmapId: ""
    property string starmapTitle: qsTr("星图")
    required property var dt
    property var starmapBackendRef: null
    // Issue #790 评论 5875963057: 公共入口收口后的应用状态。
    property var appState: ({})

    // Issue #977：顶栏需要知道当前是根画布还是子画布，以决定返回文案与动作。
    readonly property bool focusIsRoot: canvas.focusIsRoot

    signal backClicked()
    signal requestSync()
    signal requestSearch()
    signal openSettings()

    // Issue #977：壳层统一返回入口。根画布退出星图工作区；子画布走 Core
    // 导航路由返回父画布，并恢复进入父画布时保存的镜头。
    function navigateBack() {
        if (canvas.focusIsRoot)
            root.backClicked()
        else
            canvas.focusParentScene()
    }

    // Issue #982：全局搜索点击星图结果时定位到具体节点 / 边 / 嵌入。
    function revealSearchTarget(target) {
        return canvas.revealSearchTarget(target)
    }

    // Issue #814 评论 5935285879: 当前活动画布共享的唯一选中状态控制器。
    // 由 Workspace 创建，传给 Canvas；切换活动画布时选中身份带新的 routeKey。
    StarMapSelectionController {
        id: sharedSelectionController
    }

    Rectangle {
        anchors.fill: parent
        color: Kirigami.Theme.backgroundColor

        // 主体：唯一一块星图画布占满顶栏以下的全部空间。
        StarMapCanvas {
            id: canvas
            anchors.fill: parent
            dt: root.dt
            canvasBackgroundColor: Kirigami.Theme.backgroundColor
            starmapId: root.starmapId
            starmapBackendRef: root.starmapBackendRef
            selectionController: sharedSelectionController

            onVisibleChanged: {
                if (!visible)
                    canvas.resetInteraction()
            }
        }
    }
}
