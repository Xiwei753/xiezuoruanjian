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
    //   starmapPathStack 记录从根星图到当前层的路径，每项 { starmapId, title, segment }。
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
    property var currentPathSegments: []

    // Issue #801 评论 5900350140: 当前层身份是 root starmap + currentPathSegments。
    // 加载前先交给 Core resolver 逐段解析出 finalStarmapId，currentStarmapId
    // 只保存解析结果；点击事件传来的裸 targetStarmapId 不再直接决定加载。
    // 解析失败时 layerResolveError 非空（工具栏显示），层级状态不前进。
    property string rootStarmapId: ""
    property string layerResolveError: ""

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

                    // Issue #801 评论 5900350140: root + 路径段解析失败的报错出口。
                    // 解析失败时层级不切换，用户在这里看到原因。
                    AppText {
                        dt: root.dt
                        visible: root.layerResolveError.length > 0
                        text: root.layerResolveError
                        color: dt.error
                        font.pointSize: dt.fontSmPt
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
    // Issue #801 评论 5894639734: reset+reload 统一由 Canvas.onStarmapIdChanged
    // 负责（只在 currentStarmapId 真正变化时触发），避免重复 loadGraph。
    // Issue #801 评论 5900350140: 根层同样先经 resolver 解析再切换 currentStarmapId。
    onStarmapIdChanged: {
        rootStarmapId = starmapId
        currentStarmapTitle = starmapTitle
        starmapPathStack = []
        currentPathSegments = []
        refreshCurrentStarmap()
    }

    // Issue #801 评论 5900350140: starmapBackendRef 与 starmapId 的注入顺序不保证，
    // 后端晚到时补一次解析（不产生假报错）。
    onStarmapBackendRefChanged: {
        if (rootStarmapId !== "" && currentStarmapId === "")
            refreshCurrentStarmap()
    }

    // Issue #801 评论 5894035036: 外部标题变化时（如 AppController 更新），
    // 若还在根星图，同步到 currentStarmapTitle。
    onStarmapTitleChanged: {
        if (starmapPathStack.length === 0) {
            currentStarmapTitle = starmapTitle
        }
    }

    // Issue #801 评论 5900350140: 根层加载走同一条解析入口；
    // 首次成功后 currentStarmapId 变化触发 Canvas reset+loadGraph。
    Component.onCompleted: {
        rootStarmapId = starmapId
        currentStarmapTitle = starmapTitle
        refreshCurrentStarmap()
    }

    // Issue #801 评论 5900350140: 当前层加载入口——root starmap + 路径段交给
    // 后端/Core resolver 逐段解析出 finalStarmapId。解析失败时报错并返回 "",
    // 调用方不得回退到点击事件传来的裸 target id 继续加载。
    function resolveStarmapPath(segments) {
        if (rootStarmapId === "")
            return ""
        // 后端未注入时不报错：创建顺序不保证，等 onStarmapBackendRefChanged 补解析。
        if (!starmapBackendRef)
            return ""
        var res = starmapBackendRef.resolve_starmap_path(rootStarmapId, JSON.stringify(segments))
        if (!res || res.success !== true) {
            layerResolveError = qsTr("解析星图层级路径失败")
                    + (res && res.errorCode ? " (" + res.errorCode + ")" : "")
            return ""
        }
        var finalId = res.data && res.data.finalStarmapId ? res.data.finalStarmapId : ""
        if (finalId === "") {
            layerResolveError = qsTr("解析星图层级路径失败")
            return ""
        }
        layerResolveError = ""
        return finalId
    }

    // 按当前 root + currentPathSegments 重算 currentStarmapId；成功返回 true。
    function refreshCurrentStarmap() {
        var resolvedId = resolveStarmapPath(currentPathSegments)
        if (resolvedId === "")
            return false
        currentStarmapId = resolvedId
        return true
    }

    // Issue #801 评论 5894035036: 下钻到子星图——push 当前层到栈，切换 current。
    // Issue #801 评论 5894639734: 只改栈和 current，Canvas.onStarmapIdChanged 负责 reset+reload。
    // Issue #801 评论 5900350140: 先用 root + 追加 segment 后的路径解析目标层，
    // 解析失败直接报错返回；targetId 只作调用方诊断信息，不参与加载决策。
    function enterChildStarmap(targetId, title, segment) {
        // Issue #801 评论 5894981235: QML var 原地 push 不触发 change notification，
        // 必须重新赋值数组才能让 starmapPathStackChanged 发出，
        // canDrillUp binding 才会跟着层级正确更新。
        // Issue #801 评论 5897793716: 栈元素保存进入本层的 segment，
        // currentPathSegments 同步 append，保留完整层级身份。
        var nextSegments = currentPathSegments.concat([segment])
        var resolvedId = resolveStarmapPath(nextSegments)
        if (resolvedId === "")
            return
        starmapPathStack = starmapPathStack.concat([
            { starmapId: currentStarmapId, title: currentStarmapTitle, segment: segment }
        ])
        currentPathSegments = nextSegments
        currentStarmapId = resolvedId
        currentStarmapTitle = title
    }

    // Issue #801 评论 5894035036: 返回父星图——pop 栈并切换 current。
    // Issue #801 评论 5894639734: 只改栈和 current，Canvas.onStarmapIdChanged 负责 reset+reload。
    // Issue #801 评论 5900350140: 父层同样按 root + 剩余路径解析后再切换。
    function returnToParentStarmap() {
        if (starmapPathStack.length === 0)
            return
        // Issue #801 评论 5894981235: QML var 原地 pop 不触发 change notification，
        // 必须重新赋值数组才能让 starmapPathStackChanged 发出，
        // canDrillUp binding 才会跟着层级正确更新。
        // Issue #801 评论 5897793716: currentPathSegments 同步 pop。
        var parent = starmapPathStack[starmapPathStack.length - 1]
        var parentSegments = currentPathSegments.slice(0, currentPathSegments.length - 1)
        var resolvedId = resolveStarmapPath(parentSegments)
        if (resolvedId === "")
            return
        starmapPathStack = starmapPathStack.slice(0, starmapPathStack.length - 1)
        currentPathSegments = parentSegments
        currentStarmapId = resolvedId
        currentStarmapTitle = parent.title
    }
}
