// =============================================================================
// StarMapCanvas.qml — 星图画布
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：星图可视化渲染、平移/缩放交互、节点选中/编辑/连线/移动、右键菜单
// 约束：
//   - 纯渲染和交互层，业务逻辑委托给 StarMapGraphController
//   - 鼠标行为由 pointerMode 状态机明确驱动，不再用单一 MouseArea 猜行为
//   - 节点只上抛点击类信号，决定权交回 Canvas
//   - 使用 Canvas 进行自定义绘制
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: canvasArea
    clip: true

    property string starmapId: ""
    required property var dt

    // Issue #801 评论 5894035036: 层级路径栈已移到 Workspace。
    // Canvas 的 starmapId 是只读输入绑定（由 Workspace.currentStarmapId 驱动），
    // 不再在内部赋值 starmapId，也不再维护 starmapPathStack。

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _accent: dt.accent
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _accentHover: dt.accentHover
    readonly property color _border: dt.border
    readonly property color _card: dt.card
    readonly property color _danger: dt.danger
    readonly property color _dangerContainer: dt.dangerContainer
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textSecondary: dt.textSecondary
    readonly property color _textMuted: dt.textMuted
    readonly property color _error: dt.error
    readonly property color _onError: dt.onError
    readonly property color _scrim: dt.scrim
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm
    readonly property int _dialogRadius: dt.dialogRadius

    property var starmapBackendRef: null
    property string errorMessage: graphController.errorMessage

    // View transform properties
    property real panX: 0
    property real panY: 0
    property real zoomLevel: 1.0

    // Issue #801 评论 5894035036: PinchHandler 以手势中心缩放的起点记录。
    property real _pinchStartZoom: 1.0
    property real _pinchStartPanX: 0
    property real _pinchStartPanY: 0

    // Issue #801 评论 5894639734: Pinch 连续 drillUp 去重 + 根星图可缩到最小。
    // canDrillUp 由 Workspace 绑定（starmapPathStack.length > 0）；
    // _pinchDrilledUp 在一次 Pinch 内只允许 drillUp 一次。
    property bool canDrillUp: false
    property bool _pinchDrilledUp: false
    // Issue #801 评论 5894639734: +/- 触屏按钮按需显示，鼠标模式不常驻。
    // 第一次收到 TouchScreen 事件时显示，切回 Mouse 时隐藏。
    property bool _touchInputActive: false

    // ---------------------------------------------------------------------------
    // 鼠标手势状态已拆到 StarMapInteractionController（interaction）：
    //   pointerMode / connectFrom* / connectMouseX/Y / pressedNodeId / pressedEmbedId
    // Canvas 只通过 interaction.* 读写瞬时状态，图操作仍留在 Canvas/GraphController。
    // ---------------------------------------------------------------------------

    // 上下文菜单辅助状态
    property var selectedNodeForMenu: null
    property var selectedEdgeForMenu: null
    // Issue #796 评论 5886483653: Embed 右键菜单辅助状态
    property var selectedEmbedForMenu: null
    property real contextMenuWorldX: 0
    property real contextMenuWorldY: 0

    // 新建对话框状态：先收集名字再写 Core（Issue #793 评论 5884923277）
    property string createMode: ""      // "node" / "starmap"
    property real createWorldX: 0
    property real createWorldY: 0

    // Signals
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal selectionCleared()
    signal enterStarmapRequested(string starmapId, string title)
    signal editNodeRequested(var node)
    // Issue #801 评论 5894035036: 层级切换请求上抛给 Workspace。
    //   drillDownRequested: 双击 portal/Embed 或下钻触发
    //   drillUpRequested: 滚轮/Pinch/按钮缩到最小以下触发
    // Issue #801 评论 5897793716: 下钻事件携带具体 segment，
    // Workspace 据此维护完整 currentPathSegments（层级身份）。
    //   正式 Embed -> { type: "enterEmbed", instanceId, nodeId: null }
    //   legacy portal -> { type: "enterPortal", instanceId: null, nodeId }
    signal drillDownRequested(string starmapId, string title, var segment)
    signal drillUpRequested()

    // Issue #798: Canvas 自身 starmapId 改变时清瞬时交互状态并重新加载，
    // 不可见 / 离开工作区时也 reset，避免旧 move/connect 状态泄漏。
    // Issue #801 评论 5894035036: starmapId 现在是只读输入，由 Workspace 驱动；
    // 切图后 resetInteraction + loadGraph 让新图正确加载。
    onStarmapIdChanged: {
        resetInteraction()
        if (starmapId.length > 0) loadGraph()
    }
    onVisibleChanged: { if (!visible) resetInteraction() }

    // Issue #798: 渲染层直接读 graphController 的模型，不再在 Canvas 维护副本。
    // graphController 是当前星图 canonical scene model 的唯一持有者。

    StarMapGraphController {
        id: graphController
        starmapId: canvasArea.starmapId
        starmapBackendRef: canvasArea.starmapBackendRef
        onGraphChanged: edgeCanvas.requestPaint()
        onSelectionCleared: canvasArea.selectionCleared()
        onNodeSelected: function(node) { canvasArea.nodeSelected(node) }
        onEdgeSelected: function(edge) { canvasArea.edgeSelected(edge) }
        onErrorMessageChanged: canvasArea.errorMessage = graphController.errorMessage
    }

    // Issue #798: 瞬时手势状态机（pan/connect/move），不读写 Core，不保存节点数据。
    StarMapInteractionController { id: interaction }

    function clearError() { graphController.clearError() }

    // Issue #798: 公开 reset 入口，供 Workspace 切图 / 不可见时清瞬时交互状态。
    function resetInteraction() {
        // Issue #798 评论 5892406254: reset 前若正在 move，edgeRenders 已被
        // transient 坐标更新。reset 后 delegate 回 canonical，edge cache 也要
        // 一起恢复 canonical，否则节点回去了线还停在拖动位置。
        var wasMove = interaction.pointerMode === "move"
        interaction.reset()
        if (wasMove) {
            graphController.computeEdgeRenders(null)
            edgeCanvas.requestPaint()
        }
    }

    // Issue #801 评论 5894035036: drillDown / drillUp / isAtRootStarmap / starmapTitle
    // 已移到 Workspace。层级栈由 Workspace 持有，Canvas 只通过 drillDownRequested /
    // drillUpRequested 信号上抛请求。父级标题由 Workspace 的 currentStarmapTitle 维护，
    // 不再从 graphController 反查（graphController 没有 starmapTitle 属性）。

    // Background Grid
    Rectangle {
        anchors.fill: parent
        color: "transparent"
        opacity: 0.3

        Canvas {
            id: gridCanvas
            anchors.fill: parent
            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.fillStyle = _textMuted

                var gridSpacing = 50 * zoomLevel
                var startX = (panX % gridSpacing)
                var startY = (panY % gridSpacing)

                for (var x = startX; x < width; x += gridSpacing) {
                    for (var y = startY; y < height; y += gridSpacing) {
                        ctx.fillRect(x - 1, y - 1, 2, 2)
                    }
                }
            }
            Connections {
                target: canvasArea
                function onPanXChanged() { gridCanvas.requestPaint() }
                function onPanYChanged() { gridCanvas.requestPaint() }
                function onZoomLevelChanged() { gridCanvas.requestPaint() }
            }
        }
    }

    // ---------------------------------------------------------------------------
    // 背景交互层：TapHandler 处理点击类，MouseArea 处理 pan 拖动与滚轮
    // TapHandler 与 MouseArea 共存：Handler 独立收到 tap/longPress 信号
    // Issue #801 评论 5894035036: 鼠标/触屏按 acceptedDevices 拆开：
    //   - 鼠标空白长按无操作（鼠标用右键打开菜单）
    //   - 触屏空白长按打开背景菜单
    //   - 触屏未长按在节点上滑动 → 画布 pan（节点没挂触屏 DragHandler，事件穿透）
    //   - 触屏长按后移动 → 更新 connect 坐标
    // ---------------------------------------------------------------------------
    Item {
        id: bgInteractionLayer
        anchors.fill: parent
        z: 0

        // 鼠标左键单击：边选中或清选区
        // Issue #796 评论 5886483653: 命中顺序统一成 Node/Embed → Edge → 空白，
        // 不让画布背景先吞掉对象点击。
        TapHandler {
            id: bgMouseLeftTap
            acceptedDevices: PointerDevice.Mouse
            acceptedButtons: Qt.LeftButton
            onSingleTapped: function(eventPoint) {
                _touchInputActive = false
                var mx = (eventPoint.position.x - panX) / zoomLevel
                var my = (eventPoint.position.y - panY) / zoomLevel
                if (findNodeAt(mx, my)) {
                    return
                }
                if (findEmbedAt(mx, my)) {
                    return
                }
                var clickedEdge = graphController.hitTestEdge(mx, my)
                if (clickedEdge) {
                    graphController.selectEdge(clickedEdge.id)
                } else {
                    clearSelection()
                }
            }
            // 鼠标空白长按无操作（鼠标用右键打开菜单）。
            onLongPressed: {
            }
        }

        // 触屏左键单击：边选中或清选区；长按打开背景菜单
        TapHandler {
            id: bgTouchLeftTap
            acceptedDevices: PointerDevice.TouchScreen
            acceptedButtons: Qt.LeftButton
            onSingleTapped: function(eventPoint) {
                _touchInputActive = true
                var mx = (eventPoint.position.x - panX) / zoomLevel
                var my = (eventPoint.position.y - panY) / zoomLevel
                if (findNodeAt(mx, my)) {
                    return
                }
                if (findEmbedAt(mx, my)) {
                    return
                }
                var clickedEdge = graphController.hitTestEdge(mx, my)
                if (clickedEdge) {
                    graphController.selectEdge(clickedEdge.id)
                } else {
                    clearSelection()
                }
            }
            // Issue #801 评论 5894035036: 触屏空白长按打开背景菜单。
            // TapHandler.longPressed 信号无参数，用 point.position 拿当前点
            // （TapHandler 继承自 SinglePointHandler，有 point 属性）。
            // Issue #801 评论 5894639734: 长按前先判命中，节点/Embed/边上的长按
            // 不弹背景菜单（Qt TapHandler 是 passive grab，背景和对象 Handler 会
            // 同时观察同一个 press，不能假设背景自动收不到）。
            onLongPressed: {
                _touchInputActive = true
                var px = bgTouchLeftTap.point.position.x
                var py = bgTouchLeftTap.point.position.y
                var wx = (px - panX) / zoomLevel
                var wy = (py - panY) / zoomLevel

                if (findNodeAt(wx, wy)) return
                if (findEmbedAt(wx, wy)) return
                if (graphController.hitTestEdge(wx, wy)) return

                contextMenuWorldX = wx
                contextMenuWorldY = wy
                bgContextMenu.popup(px, py)
            }
        }

        // 右键单击：边菜单或画布菜单
        // Issue #796 评论 5886483653: 命中顺序 Node/Embed → Edge → 空白。
        TapHandler {
            id: backgroundRightTap
            acceptedDevices: PointerDevice.Mouse
            acceptedButtons: Qt.RightButton
            onSingleTapped: function(eventPoint) {
                _touchInputActive = false
                var mx = (eventPoint.position.x - panX) / zoomLevel
                var my = (eventPoint.position.y - panY) / zoomLevel
                if (findNodeAt(mx, my)) {
                    return
                }
                if (findEmbedAt(mx, my)) {
                    return
                }
                var clickedEdge = graphController.hitTestEdge(mx, my)
                if (clickedEdge) {
                    selectedEdgeForMenu = clickedEdge
                    edgeContextMenu.popup(eventPoint.position.x, eventPoint.position.y)
                } else {
                    contextMenuWorldX = mx
                    contextMenuWorldY = my
                    bgContextMenu.popup(eventPoint.position.x, eventPoint.position.y)
                }
            }
        }

        // 触屏背景拖动：触屏未长按在节点上滑动 → 画布 pan；
        // 触屏长按后移动 → 更新 connect 坐标，超过阈值转 connect。
        // Issue #801 评论 5894035036: 节点没挂触屏 DragHandler，事件穿透到背景。
        DragHandler {
            id: bgTouchDrag
            acceptedDevices: PointerDevice.TouchScreen
            acceptedButtons: Qt.LeftButton
            target: null
            property real lastTx: 0
            property real lastTy: 0
            // Issue #801 评论 5895310100: 标记当前 move 由触屏发起，
            // 用于 onLeftReleased 区分鼠标 move（nodeDragHandler 驱动）和触屏 move（bgTouchDrag 驱动），
            // 避免两者重复 commit。
            property bool _wasTouchMove: false
            onActiveChanged: {
                if (active) {
                    lastTx = 0
                    lastTy = 0
                    _touchInputActive = true
                    // 触屏在 move 模式下开始拖动 → 标记，commit 由 bgTouchDrag 独占
                    if (interaction.pointerMode === "move") {
                        _wasTouchMove = true
                    }
                } else {
                    // Issue #801 评论 5895310100: 触屏 move 手势结束 → 提交位置。
                    // 鼠标 move 不走 bgTouchDrag（acceptedDevices 限定 TouchScreen），
                    // 其 commit 由 Node/Embed 的 onLeftReleased 负责。
                    if (_wasTouchMove && interaction.pointerMode === "move") {
                        if (interaction.pressedNodeId !== "") {
                            graphController.commitNodeMove(interaction.pressedNodeId, interaction.moveX, interaction.moveY)
                        } else if (interaction.pressedEmbedId !== "") {
                            graphController.commitEmbedMove(interaction.pressedEmbedId, interaction.moveX, interaction.moveY)
                        }
                        interaction.endMove()
                        graphController.computeEdgeRenders(null)
                        edgeCanvas.requestPaint()
                    }
                    _wasTouchMove = false
                }
            }
            onActiveTranslationChanged: {
                var dx = activeTranslation.x - lastTx
                var dy = activeTranslation.y - lastTy
                lastTx = activeTranslation.x
                lastTy = activeTranslation.y
                if (interaction.pointerMode === "idle") {
                    // 触屏未长按滑动 = 画布 pan（屏幕坐标增量直接加到 panX/panY）
                    panX += dx
                    panY += dy
                } else if (interaction.pointerMode === "contextPending") {
                    // 触屏长按后移动，更新 connect 坐标（世界坐标，除以 zoomLevel）
                    interaction.connectMouseX += dx / zoomLevel
                    interaction.connectMouseY += dy / zoomLevel
                    // 移动总距离超过阈值则转 connect
                    if (Math.sqrt(activeTranslation.x * activeTranslation.x + activeTranslation.y * activeTranslation.y) > interaction._moveThreshold) {
                        interaction.contextPendingToConnect()
                        // Issue #801 评论 5895310100: 继续移动变连线，关闭长按菜单视觉层
                        touchContextPreview.hide()
                    }
                    edgeCanvas.requestPaint()
                } else if (interaction.pointerMode === "connect") {
                    interaction.updateConnect(interaction.connectMouseX + dx / zoomLevel, interaction.connectMouseY + dy / zoomLevel)
                    edgeCanvas.requestPaint()
                } else if (interaction.pointerMode === "move") {
                    // Issue #801 评论 5895310100: 触屏菜单"移动"后再拖 → 更新 transient 坐标。
                    // Node/Embed 的 DragHandler 限定 Mouse，触屏拖动穿透到背景层，
                    // 由 bgTouchDrag 统一驱动 move。delegate 的 x/y binding 自动跟随 moveX/moveY。
                    interaction.updateMove(interaction.moveX + dx / zoomLevel, interaction.moveY + dy / zoomLevel)
                    graphController.computeEdgeRenders(currentMoveOverride())
                    edgeCanvas.requestPaint()
                }
            }
        }

        // Issue #801 评论 5894035036: 触屏双指 Pinch 缩放。
        PinchHandler {
            id: canvasPinch
            acceptedDevices: PointerDevice.TouchScreen
            target: null
            onActiveChanged: {
                if (active) {
                    _pinchStartZoom = zoomLevel
                    _pinchStartPanX = panX
                    _pinchStartPanY = panY
                    // Issue #801 评论 5894639734: 每次 Pinch 开始时重置去重标记。
                    _pinchDrilledUp = false
                    _touchInputActive = true
                }
            }
            onActiveScaleChanged: {
                var rawZoom = _pinchStartZoom * activeScale
                // Issue #801 评论 5894639734: 一次 Pinch 只 drillUp 一次，
                // 避免捏合不松手时连续 pop 多层；根星图（canDrillUp=false）
                // 时正常 clamp 到 0.35，不 return。
                if (rawZoom < 0.35 && canDrillUp && !_pinchDrilledUp) {
                    _pinchDrilledUp = true
                    drillUpRequested()
                    return
                }
                zoomLevel = Math.max(0.35, Math.min(2.5, rawZoom))
                // 以手势中心缩放
                var cx = centroid.position.x
                var cy = centroid.position.y
                panX = cx - (cx - _pinchStartPanX) * (zoomLevel / _pinchStartZoom)
                panY = cy - (cy - _pinchStartPanY) * (zoomLevel / _pinchStartZoom)
            }
        }

        // pan 拖动 + 滚轮缩放：只在 pan 模式时处理拖动，滚轮始终处理
        MouseArea {
            id: bgDragArea
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
            hoverEnabled: true

            property real lastX: 0
            property real lastY: 0

            onPressed: function(mouse) {
                _touchInputActive = false
                lastX = mouse.x
                lastY = mouse.y
                if (mouse.button === Qt.LeftButton) {
                    var wx = (mouse.x - panX) / zoomLevel
                    var wy = (mouse.y - panY) / zoomLevel
                    if (!findNodeAt(wx, wy) && !findEmbedAt(wx, wy)) {
                        interaction.beginPan()
                    }
                }
                // 中键直接进入 pan（不依赖长按）
                if (mouse.button === Qt.MiddleButton) {
                    interaction.beginPan()
                }
            }

            onPositionChanged: function(mouse) {
                if (interaction.pointerMode === "pan") {
                    var dx = mouse.x - lastX
                    var dy = mouse.y - lastY
                    panX += dx
                    panY += dy
                    lastX = mouse.x
                    lastY = mouse.y
                }
            }

            onReleased: function(mouse) {
                if (interaction.pointerMode === "pan") {
                    interaction.endPan()
                }
            }

            onWheel: function(wheel) {
                _touchInputActive = false
                var oldZoom = zoomLevel
                var delta = wheel.angleDelta.y / 120
                var newZoom = zoomLevel + delta * 0.1
                // Issue #801 评论 5894639734: 只有 canDrillUp 时越过下限才返回父级；
                // 已经在根图就正常 clamp 到 0.35。
                if (newZoom < 0.35 && canDrillUp) {
                    drillUpRequested()
                    return
                }
                zoomLevel = Math.max(0.35, Math.min(2.5, newZoom))

                var mx = wheel.x
                var my = wheel.y
                panX = mx - (mx - panX) * (zoomLevel / oldZoom)
                panY = my - (my - panY) * (zoomLevel / oldZoom)
            }
        }
    }

    // Edges canvas (now fullscreen, translated/scaled dynamically to prevent panning drifts)
    Canvas {
        id: edgeCanvas
        anchors.fill: parent
        z: 1

        Connections {
            target: canvasArea
            function onPanXChanged() { edgeCanvas.requestPaint() }
            function onPanYChanged() { edgeCanvas.requestPaint() }
            function onZoomLevelChanged() { edgeCanvas.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)

            ctx.save()
            ctx.translate(panX, panY)
            ctx.scale(zoomLevel, zoomLevel)
            ctx.lineWidth = 2

            // Draw all edges using Linux platform render data
            var renders = graphController.edgeRenders
            for (var i = 0; i < renders.length; i++) {
                var r = renders[i]
                var edge = null
                for (var ei = 0; ei < graphController.edgesModel.length; ei++) {
                    if (graphController.edgesModel[ei].id === r.edgeId) { edge = graphController.edgesModel[ei]; break }
                }
                if (!edge) continue

                var color = edge.isSelected ? _accent : _border

                // Draw line
                ctx.beginPath()
                ctx.moveTo(r.startX, r.startY)
                ctx.lineTo(r.endX, r.endY)
                ctx.strokeStyle = color
                ctx.stroke()

                // Draw arrow head
                ctx.beginPath()
                ctx.moveTo(r.arrowTipX, r.arrowTipY)
                ctx.lineTo(r.arrowLeftX, r.arrowLeftY)
                ctx.lineTo(r.arrowRightX, r.arrowRightY)
                ctx.closePath()
                ctx.fillStyle = color
                ctx.fill()

                // Label
                if (edge.label) {
                    ctx.fillStyle = _surfaceContainer
                    var tw = ctx.measureText(edge.label).width
                    ctx.fillRect(r.labelX - tw/2 - 4, r.labelY - 10, tw + 8, 20)

                    ctx.fillStyle = _textPrimary
                    ctx.font = "12px sans-serif"
                    ctx.textAlign = "center"
                    ctx.textBaseline = "middle"
                    ctx.fillText(edge.label, r.labelX, r.labelY)
                }
            }

            // Draw connecting preview line while in connect mode
            // Issue #796 评论 5887280405: 起点根据 connectFromKind 查 Node 或 Embed；
            // 兼容旧 connectFromNodeId 路径（旧代码未设 connectFromKind 时回退）。
            if (interaction.pointerMode === "connect" && (interaction.connectFromId !== "" || interaction.connectFromNodeId !== "")) {
                var startNode = null
                var startKind = interaction.connectFromKind
                var startId = interaction.connectFromId
                if (startId === "" && interaction.connectFromNodeId !== "") {
                    // 兼容旧路径：仅 connectFromNodeId 被设
                    startKind = "node"
                    startId = interaction.connectFromNodeId
                }
                if (startKind === "node") {
                    startNode = getNode(startId)
                } else if (startKind === "embed") {
                    // Embed 起点用 graphController.getEmbed 拿坐标
                    startNode = graphController.getEmbed(startId)
                }
                if (startNode) {
                    ctx.beginPath()
                    ctx.moveTo(startNode.x + startNode.width/2, startNode.y + startNode.height/2)
                    ctx.lineTo(interaction.connectMouseX, interaction.connectMouseY)
                    ctx.strokeStyle = _accent
                    ctx.lineWidth = 2
                    ctx.stroke()
                }
            }

            ctx.restore()
        }
    }

    // Main transform container
    Item {
        id: container
        x: panX
        y: panY
        scale: zoomLevel
        transformOrigin: Item.TopLeft
        z: 2

        Repeater {
            model: graphController.nodesModel
            delegate: StarMapNode {
                // Issue #798: Qt 6.11 Repeater 要求 delegate 用显式 required property
                // 接模型上下文，不能靠隐式 index。
                required property var modelData
                required property int index
                dt: canvasArea.dt
                property var nodeData: modelData

                // Issue #798: 显示坐标从 transient 状态派生，不再被命令式赋值打断 binding。
                // 当前节点处于 move 时读 interaction.moveX/moveY，否则读 canonical nodeData.x/y。
                x: interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id ? interaction.moveX : nodeData.x
                y: interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id ? interaction.moveY : nodeData.y
                width: nodeData.width
                height: nodeData.height
                title: nodeData.title
                isSelected: nodeData.isSelected
                // wobble 交给 StarMapNode 内部驱动，用 index 错开 phase
                wobbleIndex: index

                // Issue #798: 不再原地篡改 nodeData.x/y，拖动用 StarMapNode 自己的 x/y
                // 作为临时显示坐标（命令式赋值打破初始绑定），松手提交 Controller。
                onXChanged: edgeCanvas.requestPaint()
                onYChanged: edgeCanvas.requestPaint()

                // -------------------------------------------------------------------
                // 节点上抛信号 → Canvas 状态机决定行为
                // -------------------------------------------------------------------
                // Issue #801 评论 5894981235: 鼠标点击 Node 时切回鼠标模式，
                // 隐藏触屏 +/- 按钮。
                onMouseInteracted: _touchInputActive = false

                onSingleClicked: {
                    graphController.selectNode(nodeData.id)
                }

                onDoubleClicked: {
                    // Issue #801 评论 5895625744: 旧 portal Node 已在 Controller
                    // buildModels() 归一到 Embed，Node 双击统一走编辑。
                    var nd = nodeData
                    graphController.selectNode(nd.id)
                    editNodeRequested(nd)
                }

                // Issue #801 评论 5894035036: 鼠标长按直接进 connect
                // （#373 鼠标规则：长按后拖 = 拉线）
                onMouseLongPressed: {
                    var nd = nodeData
                    if (!interaction.beginConnect("node", nd.id, nodePath(nd.id), nd.x + nd.width / 2, nd.y + nd.height / 2)) {
                        return
                    }
                    isBeingDragged = true
                    edgeCanvas.requestPaint()
                }

                // Issue #801 评论 5894035036: 触屏长按进 contextPending
                // （不移动则松手弹菜单，移动超过阈值才转 connect）
                // Issue #801 评论 5895310100: 触屏长按当场显示菜单视觉层（#373：长按先出菜单反馈）。
                // 不等 onLeftReleased 才 popup；手指继续移动超过阈值时视觉层关闭转 connect，
                // 手指松开时视觉层关闭并弹出真正可点击的 nodeContextMenu。
                onTouchLongPressed: {
                    var nd = nodeData
                    if (!interaction.beginContextPending("node", nd.id, nodePath(nd.id), nd.x + nd.width / 2, nd.y + nd.height / 2)) {
                        return
                    }
                    isBeingDragged = true
                    var sceneX = (nd.x + nd.width / 2) * zoomLevel + panX
                    var sceneY = (nd.y + nd.height / 2) * zoomLevel + panY
                    touchContextPreview.show("node", sceneX, sceneY)
                    edgeCanvas.requestPaint()
                }

                onContextMenuRequested: function(sceneX, sceneY) {
                    var nd = nodeData
                    graphController.selectNode(nd.id)
                    selectedNodeForMenu = nd
                    // sceneX/sceneY 是场景坐标，菜单用屏幕坐标
                    nodeContextMenu.popup(sceneX, sceneY)
                }

                onMoveDelta: function(dx, dy) {
                    // Issue #801: contextPending 状态下移动超过阈值则转 connect
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === nodeData.id) {
                        interaction.connectMouseX += dx
                        interaction.connectMouseY += dy
                        var totalDx = interaction.connectMouseX - (nodeData.x + nodeData.width / 2)
                        var totalDy = interaction.connectMouseY - (nodeData.y + nodeData.height / 2)
                        if (Math.sqrt(totalDx * totalDx + totalDy * totalDy) > interaction._moveThreshold) {
                            interaction.contextPendingToConnect()
                        }
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "connect" && interaction.connectFromId === nodeData.id) {
                        interaction.updateConnect(interaction.connectMouseX + dx, interaction.connectMouseY + dy)
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "idle") {
                        interaction.beginMove(nodeData.id, x, y)
                    }

                    if (interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id) {
                        // Issue #798: 不再命令式 x+=dx/y+=dy 打断 binding，
                        // 只更新 transient 坐标，delegate 的 x/y binding 自动跟随。
                        interaction.updateMove(interaction.moveX + dx, interaction.moveY + dy)
                        isBeingDragged = true
                        graphController.computeEdgeRenders(currentMoveOverride())
                        edgeCanvas.requestPaint()
                    }
                }

                onLeftReleased: {
                    // 统一结束当前节点交互：无论长按后拖动还是直接松手，
                    // 都由此出口闭环 connect/move/contextPending 状态（Issue #788 评论 5868205321）。
                    isBeingDragged = false
                    // Issue #801 评论 5895310100: contextPending 松手不移动，关闭视觉层并弹出可点击菜单
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === nodeData.id) {
                        touchContextPreview.hide()
                        var pendingResult = interaction.endContextPending()
                        if (pendingResult && pendingResult.kind === "node") {
                            var nd = graphController.getNode(pendingResult.id)
                            if (nd) {
                                graphController.selectNode(nd.id)
                                selectedNodeForMenu = nd
                                // 用节点中心位置弹出菜单
                                var sceneX = (nd.x + nd.width / 2) * zoomLevel + panX
                                var sceneY = (nd.y + nd.height / 2) * zoomLevel + panY
                                nodeContextMenu.popup(sceneX, sceneY)
                            }
                        }
                        edgeCanvas.requestPaint()
                        return
                    }
                    if (interaction.pointerMode === "connect" && interaction.connectFromId === nodeData.id) {
                        // Issue #796 评论 5887280405: 松手时 Node 和 Embed 都参与命中，
                        // 用 path 版建边支持 Embed 端点。
                        var targetNode = findNodeAt(interaction.connectMouseX, interaction.connectMouseY)
                        if (targetNode && targetNode.id !== interaction.connectFromId) {
                            createEdgeWithPaths(interaction.connectFromPath, nodePath(targetNode.id))
                        } else {
                            var targetEmbed = findEmbedAt(interaction.connectMouseX, interaction.connectMouseY)
                            if (targetEmbed && targetEmbed.instanceId !== interaction.connectFromId) {
                                createEdgeWithPaths(interaction.connectFromPath, embedConnectPath(targetEmbed))
                            }
                        }
                        interaction.endConnect()
                        edgeCanvas.requestPaint()
                    } else if (interaction.pointerMode === "move" && interaction.pressedNodeId === nodeData.id) {
                        // Issue #801 评论 5895310100: 触屏 move 由 bgTouchDrag.onActiveChanged 独占 commit；
                        // 这里只处理鼠标 move（nodeDragHandler 驱动）。用 _wasTouchMove 区分，
                        // 无论 onLeftReleased 与 bgTouchDrag.onActiveChanged 的触发顺序如何都不会重复 commit。
                        if (!bgTouchDrag._wasTouchMove) {
                            graphController.commitNodeMove(nodeData.id, interaction.moveX, interaction.moveY)
                            interaction.endMove()
                        }
                    }
                }
            }
        }

        // Issue #796 评论 5886483653: Embed Repeater，用 StarMapEmbed.qml 渲染。
        // Node 和 Embed 都走同一套画布坐标转换（都在 container 里，受 panX/panY/zoomLevel 影响）。
        Repeater {
            model: graphController.embedsModel
            delegate: StarMapEmbed {
                // Issue #798: Qt 6.11 Repeater 显式 required property 模型契约。
                required property var modelData
                required property int index
                dt: canvasArea.dt
                property var embedData: modelData

                // Issue #798: 显示坐标从 transient 状态派生，不再被命令式赋值打断 binding。
                x: interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId ? interaction.moveX : embedData.x
                y: interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId ? interaction.moveY : embedData.y
                width: embedData.width
                height: embedData.height
                instanceId: embedData.instanceId
                targetStarmapId: embedData.targetStarmapId
                label: embedData.label
                isSelected: embedData.isSelected
                wobbleIndex: index

                // Issue #798: 不再原地篡改 embedData.x/y，拖动用 StarMapEmbed 自己的 x/y
                // 作为临时显示坐标，松手提交 Controller。
                onXChanged: edgeCanvas.requestPaint()
                onYChanged: edgeCanvas.requestPaint()

                // 单击只选中
                // Issue #801 评论 5894981235: 鼠标点击 Embed 时切回鼠标模式，
                // 隐藏触屏 +/- 按钮。
                onMouseInteracted: _touchInputActive = false

                onClicked: function(instId) {
                    graphController.selectEmbed(instId)
                }

                // 双击进入 targetStarmapId
                onDoubleClicked: function(tgtStarmapId) {
                    if (tgtStarmapId) {
                        var ed = embedData
                        // Issue #801 评论 5894035036: 上抛 drillDownRequested 给 Workspace，
                        // Canvas 不再自己 drillDown。
                        // Issue #801 评论 5895625744: title fallback 不再用"子星图"。
                        // Issue #801 评论 5897793716: 下钻事件携带具体 segment，
                        // 正式 Embed -> EnterEmbed(instanceId)，legacy portal -> EnterPortal(nodeId)。
                        var segment
                        if (ed.legacyPortalNodeId) {
                            segment = { type: "enterPortal", instanceId: null, nodeId: ed.legacyPortalNodeId }
                        } else {
                            segment = { type: "enterEmbed", instanceId: ed.instanceId, nodeId: null }
                        }
                        drillDownRequested(tgtStarmapId, ed.label || qsTr("未命名"), segment)
                        enterStarmapRequested(tgtStarmapId, ed.label || qsTr("未命名"))
                    }
                }

                // Issue #801 评论 5894035036: 鼠标长按直接进 connect
                onMouseLongPressed: function(instId) {
                    var ed = embedData
                    if (!interaction.beginConnect("embed", instId, embedConnectPath(ed), ed.x + ed.width / 2, ed.y + ed.height / 2)) {
                        return
                    }
                    isBeingDragged = true
                    edgeCanvas.requestPaint()
                }

                // Issue #801 评论 5894035036: 触屏长按进 contextPending
                // Issue #801 评论 5895310100: 触屏长按当场显示菜单视觉层（与 Node 对称）。
                onTouchLongPressed: function(instId) {
                    var ed = embedData
                    if (!interaction.beginContextPending("embed", instId, embedConnectPath(ed), ed.x + ed.width / 2, ed.y + ed.height / 2)) {
                        return
                    }
                    isBeingDragged = true
                    var sceneX = (ed.x + ed.width / 2) * zoomLevel + panX
                    var sceneY = (ed.y + ed.height / 2) * zoomLevel + panY
                    touchContextPreview.show("embed", sceneX, sceneY)
                    edgeCanvas.requestPaint()
                }

                // 右键上抛菜单
                onContextMenuRequested: function(instId, sceneX, sceneY) {
                    graphController.selectEmbed(instId)
                    selectedEmbedForMenu = graphController.getEmbed(instId)
                    embedContextMenu.popup(sceneX, sceneY)
                }

                // Issue #796 评论 5888480054: Embed 拖动改上抛 moveDelta 增量，
                // 和 Node 的 onMoveDelta 对称。connect 模式更新预览线终点；
                // idle 转 move 移动 Embed position。
                onMoveDelta: function(dx, dy) {
                    // Issue #801: contextPending 状态下移动超过阈值则转 connect
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === embedData.instanceId) {
                        interaction.connectMouseX += dx
                        interaction.connectMouseY += dy
                        var totalDx = interaction.connectMouseX - (embedData.x + embedData.width / 2)
                        var totalDy = interaction.connectMouseY - (embedData.y + embedData.height / 2)
                        if (Math.sqrt(totalDx * totalDx + totalDy * totalDy) > interaction._moveThreshold) {
                            interaction.contextPendingToConnect()
                        }
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "connect" && interaction.connectFromId === embedData.instanceId) {
                        interaction.updateConnect(interaction.connectMouseX + dx, interaction.connectMouseY + dy)
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (interaction.pointerMode === "idle") {
                        interaction.beginEmbedMove(embedData.instanceId, x, y)
                    }

                    if (interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId) {
                        // Issue #798: 不再命令式 x+=dx/y+=dy 打断 binding，
                        // 只更新 transient 坐标，delegate 的 x/y binding 自动跟随。
                        interaction.updateMove(interaction.moveX + dx, interaction.moveY + dy)
                        isBeingDragged = true
                        graphController.computeEdgeRenders(currentMoveOverride())
                        edgeCanvas.requestPaint()
                    }
                }

                onLeftReleased: {
                    isBeingDragged = false
                    // Issue #801 评论 5895310100: contextPending 松手不移动，关闭视觉层并弹出可点击菜单
                    if (interaction.pointerMode === "contextPending" && interaction.connectFromId === embedData.instanceId) {
                        touchContextPreview.hide()
                        var pendingResult = interaction.endContextPending()
                        if (pendingResult && pendingResult.kind === "embed") {
                            var ed = graphController.getEmbed(pendingResult.id)
                            if (ed) {
                                graphController.selectEmbed(ed.instanceId)
                                selectedEmbedForMenu = ed
                                // 用 Embed 中心位置弹出菜单
                                var sceneX = (ed.x + ed.width / 2) * zoomLevel + panX
                                var sceneY = (ed.y + ed.height / 2) * zoomLevel + panY
                                embedContextMenu.popup(sceneX, sceneY)
                            }
                        }
                        edgeCanvas.requestPaint()
                        return
                    }
                    // Issue #796 评论 5887280405: connect 模式下松手，Node 和 Embed 都参与命中，
                    // 用 path 版建边；否则走原拖动结束保存位置逻辑。
                    if (interaction.pointerMode === "connect" && interaction.connectFromId === embedData.instanceId) {
                        var targetNode = findNodeAt(interaction.connectMouseX, interaction.connectMouseY)
                        if (targetNode && targetNode.id !== interaction.connectFromId) {
                            createEdgeWithPaths(interaction.connectFromPath, nodePath(targetNode.id))
                        } else {
                            var targetEmbed = findEmbedAt(interaction.connectMouseX, interaction.connectMouseY)
                            if (targetEmbed && targetEmbed.instanceId !== interaction.connectFromId) {
                                createEdgeWithPaths(interaction.connectFromPath, embedConnectPath(targetEmbed))
                            }
                        }
                        interaction.endConnect()
                        edgeCanvas.requestPaint()
                    } else if (interaction.pointerMode === "move" && interaction.pressedEmbedId === embedData.instanceId) {
                        // Issue #801 评论 5895310100: 触屏 move 由 bgTouchDrag.onActiveChanged 独占 commit；
                        // 这里只处理鼠标 move。用 _wasTouchMove 区分避免重复 commit。
                        if (!bgTouchDrag._wasTouchMove) {
                            graphController.commitEmbedMove(embedData.instanceId, interaction.moveX, interaction.moveY)
                            interaction.endMove()
                        }
                    }
                }
            }
        }
    }

    AppText {
        dt: canvasArea.dt
        anchors.centerIn: parent
        text: qsTr("右键空白处新建节点或子星图")
        color: _textSecondary
        font.pointSize: dt.fontLgPt
        visible: graphController.nodesModel.length === 0 && graphController.embedsModel.length === 0
    }

    // Issue #801 评论 5894639734: 触屏缩放 +/- 按钮（右下角浮层）。
    // 按需显示：第一次收到 TouchScreen 事件时显示，切回 Mouse 时隐藏。
    // 不做硬件探测，靠 _touchInputActive 跟踪最近一次输入设备。
    RowLayout {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: 16
        anchors.bottomMargin: 16
        spacing: 8
        z: 50
        visible: _touchInputActive

        AppButton {
            dt: canvasArea.dt
            text: qsTr("+")
            onClicked: {
                // 单击放大：直接调到上限，不触发 drillUp
                zoomLevel = Math.min(2.5, zoomLevel + 0.15)
            }
        }

        AppButton {
            dt: canvasArea.dt
            text: qsTr("−")
            onClicked: {
                var newZoom = zoomLevel - 0.15
                // Issue #801 评论 5894639734: 只有 canDrillUp 时越过下限才返回父级；
                // 已经在根图就正常 clamp 到 0.35。
                if (newZoom < 0.35 && canDrillUp) {
                    drillUpRequested()
                    return
                }
                zoomLevel = Math.max(0.35, newZoom)
            }
        }
    }

    Rectangle {
        id: errorBanner
        width: parent.width - 32
        height: 40
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        anchors.horizontalCenter: parent.horizontalCenter
        color: _error
        radius: _radiusSm
        visible: errorMessage.length > 0
        z: 100

        AppText {
            dt: canvasArea.dt
            anchors.centerIn: parent
            text: errorMessage
            color: _onError
            font.pointSize: dt.bodyPt
        }
        MouseArea {
            anchors.fill: parent
            onClicked: clearError()
        }
    }

    // Issue #801 评论 5895310100: 触屏长按菜单视觉层（不抢 pointer grab）。
    // #373 要求长按时菜单先出现作为视觉反馈；手指继续移动超过阈值则关闭转连线，
    // 手指松开则关闭视觉层并弹出真正可点击的 Menu。
    // 此组件纯视觉，无任何 TapHandler/MouseArea/Handler，不会抢走正在进行的触摸手势。
    Item {
        id: touchContextPreview
        visible: false
        z: 60

        property string previewKind: ""   // "node" / "embed"
        // 屏幕坐标锚点
        property real anchorX: 0
        property real anchorY: 0

        x: anchorX - width / 2
        y: anchorY + 8
        width: 150
        height: 120

        Rectangle {
            anchors.fill: parent
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 8
            spacing: 4

            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: touchContextPreview.previewKind === "embed" ? qsTr("编辑名称") : qsTr("编辑")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: qsTr("移动")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
            AppText {
                dt: canvasArea.dt
                Layout.fillWidth: true
                text: qsTr("删除")
                color: _textPrimary
                font.pointSize: dt.labelPt
                leftPadding: 4
            }
        }

        function show(kind, sx, sy) {
            previewKind = kind
            anchorX = sx
            anchorY = sy
            visible = true
        }

        function hide() {
            visible = false
        }
    }

    function currentMoveOverride() {
        if (interaction.pointerMode === "move") {
            if (interaction.pressedNodeId !== "") {
                return { kind: "node", id: interaction.pressedNodeId, x: interaction.moveX, y: interaction.moveY }
            } else if (interaction.pressedEmbedId !== "") {
                return { kind: "embed", id: interaction.pressedEmbedId, x: interaction.moveX, y: interaction.moveY }
            }
        }
        return null
    }

    function loadGraph() {
        graphController.loadGraph()
    }

    function buildModels() {
        graphController.buildModels()
    }

    function getNode(id) {
        if (container) {
            for (var i = 0; i < container.children.length; i++) {
                var child = container.children[i];
                if (child && child.nodeData && child.nodeData.id === id) {
                    return child;
                }
            }
        }
        // Issue #798: 回退到 Controller 的 canonical 模型。
        return graphController.getNode(id);
    }

    function clearSelection() {
        graphController.clearSelection()
    }

    // Issue #796 评论 5886483653: 新建子星图改回正式 Embed 语义。
    // 流程：create_starmap → create_starmap_embed；失败时删除刚创建的目标 StarMap，
    // 成功后 reload graph 并选中新 Embed。
    // 旧 portal Node 仍按已有数据正常显示/进入，不再用它创建新的子星图。
    // title 由 createDialog 收集后传入，不再写死默认名。
    // Issue #798: 图操作移进 GraphController，Canvas 只转发。
    function createSubStarmapAt(title, wx, wy) {
        graphController.createSubStarmapAt(title, wx, wy)
    }

    function createEdge(fromId, toId) {
        graphController.createEdge(fromId, toId)
    }

    // Issue #796 评论 5887280405: Node 端点的 StarMapTargetPathDto JS 对象。
    // JSON 字段名遵循 DTO serde rename：starmapId（camelCase）、segments、target.type/nodeId。
    function nodePath(nodeId) {
        return {
            starmapId: starmapId,
            segments: [],
            target: { type: "node", nodeId: nodeId }
        }
    }

    // Issue #796 评论 5887280405: Embed 端点的 StarMapTargetPathDto JS 对象。
    // segments 用 enterEmbed 段指向 instanceId，target.type 为 "starmap"。
    function embedPath(instanceId) {
        return {
            starmapId: starmapId,
            segments: [
                { type: "enterEmbed", instanceId: instanceId, nodeId: null }
            ],
            target: { type: "starmap" }
        }
    }

    // Issue #801 评论 5897793716: legacy portal 拉线端点用 EnterPortal 段。
    // Core 已正式定义 EnterPortal { nodeId } = 通过 portal 节点进入子星图空间。
    // 不再返回普通 nodePath——视觉上是子星图入口，底层语义也必须是 enterPortal。
    function portalPath(nodeId) {
        return {
            starmapId: starmapId,
            segments: [
                { type: "enterPortal", instanceId: null, nodeId: nodeId }
            ],
            target: { type: "starmap" }
        }
    }

    // Issue #801 评论 5896594591: Embed 拉线端点路径。
    // 旧 portal（legacyPortalNodeId 存在）真实身份是 Node，端点用 portalPath（EnterPortal 段），
    // 不能构造 EnterEmbed(fakeInstanceId)。正式 Embed 继续用 embedPath。
    function embedConnectPath(embed) {
        if (embed && embed.legacyPortalNodeId) {
            return portalPath(embed.legacyPortalNodeId)
        }
        return embedPath(embed.instanceId)
    }

    // Issue #796 评论 5887280405: 用 fromPath/toPath 建边，支持 Node 和 Embed 端点。
    function createEdgeWithPaths(fromPath, toPath) {
        graphController.createEdgeWithPaths(fromPath, toPath)
    }

    function updateNodeFromInspector(nodeId, patch) {
        graphController.updateNode(nodeId, patch)
    }

    function deleteNodeFromInspector(nodeId) {
        graphController.deleteNode(nodeId)
    }

    function updateEdgeFromInspector(edgeId, patch) {
        graphController.updateEdge(edgeId, patch)
    }

    function deleteEdgeFromInspector(edgeId) {
        graphController.deleteEdge(edgeId)
    }

    // Helper function to find a node at world coordinates
    function findNodeAt(wx, wy) {
        return graphController.findNodeAt(wx, wy)
    }

    // Issue #796 评论 5886483653: 按世界坐标命中 Embed
    function findEmbedAt(wx, wy) {
        return graphController.findEmbedAt(wx, wy)
    }

    // Issue #796 评论 5886483653: Embed 增删改转发给 graphController
    function updateEmbedFromInspector(instanceId, patch) {
        graphController.updateEmbed(instanceId, patch)
    }

    function deleteEmbedFromInspector(instanceId) {
        graphController.deleteEmbed(instanceId)
    }

    // Issue #793 评论 5884923277: 新节点落点选择，避免压在已有节点上。
    // 从 (wx,wy) 起按 ring 扩张枚举候选格点，第一个不与现有节点矩形相交的即返回。
    function findFreeSpawnPoint(wx, wy) {
        var step = 24
        var candidates = [{x: wx, y: wy}]

        for (var ring = 1; ring <= 8; ring++) {
            for (var dx = -ring; dx <= ring; dx++) {
                candidates.push({ x: wx + dx * step, y: wy - ring * step })
                candidates.push({ x: wx + dx * step, y: wy + ring * step })
            }
            for (var dy = -ring + 1; dy <= ring - 1; dy++) {
                candidates.push({ x: wx - ring * step, y: wy + dy * step })
                candidates.push({ x: wx + ring * step, y: wy + dy * step })
            }
        }

        for (var i = 0; i < candidates.length; i++) {
            if (!overlapsExistingNode(candidates[i].x, candidates[i].y, 150, 60, 12))
                return candidates[i]
        }

        return { x: wx, y: wy }
    }

    // 用 graphController.nodesModel 当前的 x/y/width/height 做矩形相交，四边多留 padding。
    function overlapsExistingNode(x, y, w, h, padding) {
        var nodes = graphController.nodesModel
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            var nx = n.x - padding
            var ny = n.y - padding
            var nw = n.width + padding * 2
            var nh = n.height + padding * 2
            if (x < nx + nw && x + w > nx && y < ny + nh && y + h > ny)
                return true
        }
        return false
    }

    // Context Menus
    Menu {
        id: bgContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: bgMenuItem1
            text: qsTr("新建节点")
            contentItem: AppText {
                dt: canvasArea.dt
                text: bgMenuItem1.text
                color: bgMenuItem1.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                font.bold: true
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: bgMenuItem1.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: createDialog.open("node", contextMenuWorldX, contextMenuWorldY)
        }

        MenuItem {
            id: bgMenuItem2
            text: qsTr("新建子星图")
            contentItem: AppText {
                dt: canvasArea.dt
                text: bgMenuItem2.text
                color: bgMenuItem2.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                font.bold: true
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: bgMenuItem2.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: createDialog.open("starmap", contextMenuWorldX, contextMenuWorldY)
        }
    }

    Menu {
        id: nodeContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: nodeMenuItemEdit
            text: qsTr("编辑")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemEdit.text
                color: nodeMenuItemEdit.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemEdit.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                // Issue #791: 右键"编辑"走 editNodeRequested 打开按需浮层
                if (selectedNodeForMenu) {
                    graphController.selectNode(selectedNodeForMenu.id)
                    editNodeRequested(selectedNodeForMenu)
                }
            }
        }

        MenuItem {
            id: nodeMenuItemMove
            text: qsTr("移动")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemMove.text
                color: nodeMenuItemMove.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemMove.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedNodeForMenu) {
                    interaction.beginMove(selectedNodeForMenu.id, selectedNodeForMenu.x, selectedNodeForMenu.y)
                }
            }
        }

        MenuItem {
            id: nodeMenuItemDelete
            text: qsTr("删除")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemDelete.text
                color: nodeMenuItemDelete.hovered ? _danger : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemDelete.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedNodeForMenu) {
                    deleteNodeFromInspector(selectedNodeForMenu.id)
                }
            }
        }
    }

    Menu {
        id: edgeContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: edgeMenuItem1
            text: qsTr("重命名连线")
            contentItem: AppText {
                dt: canvasArea.dt
                text: edgeMenuItem1.text
                color: edgeMenuItem1.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem1.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEdgeForMenu) {
                    renameDialog.open("edge", selectedEdgeForMenu.id, selectedEdgeForMenu.label || "")
                }
            }
        }

        MenuItem {
            id: edgeMenuItem2
            text: qsTr("删除连线")
            contentItem: AppText {
                dt: canvasArea.dt
                text: edgeMenuItem2.text
                color: edgeMenuItem2.hovered ? _danger : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem2.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEdgeForMenu) {
                    deleteEdgeFromInspector(selectedEdgeForMenu.id)
                }
            }
        }
    }

    // Issue #796 评论 5886483653: Embed 右键菜单：编辑名称 / 移动 / 删除
    Menu {
        id: embedContextMenu

        background: Rectangle {
            implicitWidth: 150
            color: _card
            border.color: _border
            border.width: 1
            radius: _radiusSm
        }

        MenuItem {
            id: embedMenuItemRename
            text: qsTr("编辑名称")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemRename.text
                color: embedMenuItemRename.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemRename.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu) {
                    renameDialog.open("embed", selectedEmbedForMenu.instanceId, selectedEmbedForMenu.label || "")
                }
            }
        }

        // Issue #796 评论 5887280405: Embed 移动菜单项，与 Node 移动对称。
        // 触发后进入 move 模式，pressedEmbedId 记录待移动 Embed。
        MenuItem {
            id: embedMenuItemMove
            text: qsTr("移动")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemMove.text
                color: embedMenuItemMove.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemMove.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu) {
                    interaction.beginEmbedMove(selectedEmbedForMenu.instanceId, selectedEmbedForMenu.x, selectedEmbedForMenu.y)
                }
            }
        }

        MenuItem {
            id: embedMenuItemDelete
            text: qsTr("删除")
            contentItem: AppText {
                dt: canvasArea.dt
                text: embedMenuItemDelete.text
                color: embedMenuItemDelete.hovered ? _danger : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: embedMenuItemDelete.hovered ? _dangerContainer : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedEmbedForMenu) {
                    deleteEmbedFromInspector(selectedEmbedForMenu.instanceId)
                }
            }
        }
    }

    // Issue #796 评论 5886483653: 弹窗改 Qt Quick Controls Popup，不再手搓整屏 Rectangle。
    // modal + focus + closePolicy 交给 Popup，遮罩用 Overlay.modal 做半透明 dim。
    Popup {
        id: renameDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 300
        height: 160
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        property string targetType: "" // "node" or "edge"
        property string targetId: ""
        property string initialText: ""

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            AppText {
                dt: canvasArea.dt
                text: renameDialog.targetType === "node" ? qsTr("修改节点标题")
                     : renameDialog.targetType === "embed" ? qsTr("修改子星图名称")
                     : qsTr("修改连线标签")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            TextField {
                id: renameInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
                font.pointSize: dt.bodyPt
                focus: renameDialog.visible
                text: renameDialog.initialText

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: renameInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: renameDialog.confirm()
                Keys.onEscapePressed: renameDialog.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: cancelBtn
                    text: qsTr("取消")
                    onClicked: renameDialog.close()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: cancelBtn.text
                        color: _textSecondary
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: cancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: confirmBtn
                    text: qsTr("确定")
                    onClicked: renameDialog.confirm()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: confirmBtn.text
                        color: _onPrimary
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: confirmBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(type, id, text) {
            targetType = type
            targetId = id
            initialText = text
            renameInput.text = text
            visible = true
            renameInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            if (targetType === "node") {
                updateNodeFromInspector(targetId, { title: renameInput.text })
            } else if (targetType === "edge") {
                updateEdgeFromInspector(targetId, { label: renameInput.text })
            } else if (targetType === "embed") {
                updateEmbedFromInspector(targetId, { label: renameInput.text })
            }
            close()
        }
    }

    // Issue #793 评论 5884923277: 新建节点/子星图 Dialog
    // Issue #796 评论 5886483653: 改用 Qt Quick Controls Popup，不再手搓整屏 Rectangle。
    // 右键空白处不再直接创建，先收集名字再写 Core。
    Popup {
        id: createDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 300
        height: 160
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            AppText {
                dt: canvasArea.dt
                text: createMode === "starmap" ? qsTr("新建子星图") : qsTr("新建节点")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            TextField {
                id: createInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
                font.pointSize: dt.bodyPt
                placeholderText: qsTr("名称")
                focus: createDialog.visible
                text: ""

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: createInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: createDialog.confirm()
                Keys.onEscapePressed: createDialog.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: createCancelBtn
                    text: qsTr("取消")
                    onClicked: createDialog.close()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: createCancelBtn.text
                        color: _textSecondary
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: createCancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: createConfirmBtn
                    text: qsTr("确定")
                    onClicked: createDialog.confirm()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: createConfirmBtn.text
                        color: _onPrimary
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: createConfirmBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(mode, wx, wy) {
            createMode = mode
            createWorldX = wx
            createWorldY = wy
            createInput.text = ""
            visible = true
            createInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            var name = createInput.text.trim()
            if (name.length === 0) {
                close()
                return
            }
            // 先算 spawn 落点，避免新节点压在旧节点上
            var spawn = findFreeSpawnPoint(createWorldX, createWorldY)
            if (createMode === "node") {
                graphController.createNode(name, spawn.x, spawn.y)
            } else if (createMode === "starmap") {
                createSubStarmapAt(name, spawn.x, spawn.y)
            }
            close()
        }
    }

}
