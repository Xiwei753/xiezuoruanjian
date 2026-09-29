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

    // ---------------------------------------------------------------------------
    // 鼠标状态机：idle / pan / connect / move
    //   idle    — 无活跃拖拽手势
    //   pan     — 长按空白后拖动，平移画布
    //   connect — 长按节点后拖动，拉线预览
    //   move    — 节点右键菜单"移动节点"后左键拖动，仅移动指定节点
    // ---------------------------------------------------------------------------
    property string pointerMode: "idle"
    property string pressedNodeId: ""
    property string connectFromNodeId: ""
    property real connectMouseX: 0
    property real connectMouseY: 0
    // Issue #796 评论 5887280405: connect 模式扩展，支持 Node 和 Embed 作为连线端点。
    // connectFromKind: "node" 或 "embed"；connectFromId: nodeId 或 instanceId；
    // connectFromPath: 源端 StarMapTargetPathDto JS 对象，由 nodePath()/embedPath() 构造。
    // connectFromNodeId 保留给预览线绘制兼容路径，新代码逐步用 connectFromKind/Id 替代。
    property string connectFromKind: ""
    property string connectFromId: ""
    property var connectFromPath: null
    // Embed 移动模式用：右键菜单"移动"后左键拖动指定 Embed。
    property string pressedEmbedId: ""

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

    // Model data
    property var nodesModel: []
    property var edgesModel: []
    // Issue #796 评论 5886483653: Embed 显示模型，从 graphController 同步。
    property var embedsModel: []

    StarMapGraphController {
        id: graphController
        starmapId: canvasArea.starmapId
        starmapBackendRef: canvasArea.starmapBackendRef
        onGraphChanged: {
            canvasArea.nodesModel = graphController.nodesModel
            canvasArea.edgesModel = graphController.edgesModel
            canvasArea.embedsModel = graphController.embedsModel
            edgeCanvas.requestPaint()
        }
        onSelectionCleared: canvasArea.selectionCleared()
        onNodeSelected: function(node) { canvasArea.nodeSelected(node) }
        onEdgeSelected: function(edge) { canvasArea.edgeSelected(edge) }
        onErrorMessageChanged: canvasArea.errorMessage = graphController.errorMessage
    }

    function clearError() { graphController.clearError() }

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
    // ---------------------------------------------------------------------------
    Item {
        id: bgInteractionLayer
        anchors.fill: parent
        z: 0

        // 左键单击：边选中或清选区
        // Issue #796 评论 5886483653: 命中顺序统一成 Node/Embed → Edge → 空白，
        // 不让画布背景先吞掉对象点击。
        TapHandler {
            id: backgroundLeftTap
            acceptedButtons: Qt.LeftButton
            onSingleTapped: function(eventPoint) {
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
            // pan 已由 bgDragArea.onPressed 直接处理（#373 桌面规则），
            // long press 不再负责进入 pan。
            onLongPressed: {
            }
        }

        // 右键单击：边菜单或画布菜单
        // Issue #796 评论 5886483653: 命中顺序 Node/Embed → Edge → 空白。
        TapHandler {
            id: backgroundRightTap
            acceptedButtons: Qt.RightButton
            onSingleTapped: function(eventPoint) {
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

        // pan 拖动 + 滚轮缩放：只在 pan 模式时处理拖动，滚轮始终处理
        MouseArea {
            id: bgDragArea
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
            hoverEnabled: true

            property real lastX: 0
            property real lastY: 0

            onPressed: function(mouse) {
                lastX = mouse.x
                lastY = mouse.y
                if (mouse.button === Qt.LeftButton) {
                    var wx = (mouse.x - panX) / zoomLevel
                    var wy = (mouse.y - panY) / zoomLevel
                    if (!findNodeAt(wx, wy) && !findEmbedAt(wx, wy)) {
                        pointerMode = "pan"
                    }
                }
                // 中键直接进入 pan（不依赖长按）
                if (mouse.button === Qt.MiddleButton) {
                    pointerMode = "pan"
                }
            }

            onPositionChanged: function(mouse) {
                if (pointerMode === "pan") {
                    var dx = mouse.x - lastX
                    var dy = mouse.y - lastY
                    panX += dx
                    panY += dy
                    lastX = mouse.x
                    lastY = mouse.y
                }
            }

            onReleased: function(mouse) {
                if (pointerMode === "pan") {
                    pointerMode = "idle"
                }
            }

            onWheel: function(wheel) {
                var oldZoom = zoomLevel
                var delta = wheel.angleDelta.y / 120
                zoomLevel += delta * 0.1
                zoomLevel = Math.max(0.35, Math.min(2.5, zoomLevel))

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
            graphController.computeEdgeRenders()
            var renders = graphController.edgeRenders
            for (var i = 0; i < renders.length; i++) {
                var r = renders[i]
                var edge = null
                for (var ei = 0; ei < edgesModel.length; ei++) {
                    if (edgesModel[ei].id === r.edgeId) { edge = edgesModel[ei]; break }
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
            if (pointerMode === "connect" && (connectFromId !== "" || connectFromNodeId !== "")) {
                var startNode = null
                var startKind = connectFromKind
                var startId = connectFromId
                if (startId === "" && connectFromNodeId !== "") {
                    // 兼容旧路径：仅 connectFromNodeId 被设
                    startKind = "node"
                    startId = connectFromNodeId
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
                    ctx.lineTo(connectMouseX, connectMouseY)
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
            model: nodesModel.length
            delegate: StarMapNode {
                dt: canvasArea.dt
                property var nodeData: nodesModel[index]

                x: nodeData.x
                y: nodeData.y
                width: nodeData.width
                height: nodeData.height
                title: nodeData.title
                kind: nodeData.kind
                isSelected: nodeData.isSelected
                // Issue #793 评论 5884923277: portal 节点展示标记，
                // 双击仍沿用现有 destinationStarmapId 进入，不另外发明类型。
                isPortal: !!(nodeData.portal && nodeData.portal.destinationStarmapId)
                // wobble 交给 StarMapNode 内部驱动，用 index 错开 phase
                wobbleIndex: index

                onXChanged: {
                    if (nodeData) {
                        nodeData.x = x
                    }
                    edgeCanvas.requestPaint()
                }

                onYChanged: {
                    if (nodeData) {
                        nodeData.y = y
                    }
                    edgeCanvas.requestPaint()
                }

                // -------------------------------------------------------------------
                // 节点上抛信号 → Canvas 状态机决定行为
                // -------------------------------------------------------------------
                onSingleClicked: {
                    graphController.selectNode(nodesModel[index].id)
                }

                onDoubleClicked: {
                    var nd = nodesModel[index]
                    if (nd.portal && nd.portal.destinationStarmapId) {
                        enterStarmapRequested(
                            nd.portal.destinationStarmapId,
                            nd.title || qsTr("子星图")
                        )
                    } else {
                        graphController.selectNode(nd.id)
                        editNodeRequested(nd)
                    }
                }

                onLongPressed: {
                    var nd = nodesModel[index]
                    // 只允许 idle 时长按进入 connect；避免右键菜单"移动节点"
                    // 已选 move 后，左键按住稍久被 long press 覆盖成 connect
                    // （Issue #788 评论 5868205321）。
                    if (pointerMode !== "idle") {
                        return
                    }
                    pointerMode = "connect"
                    // Issue #796 评论 5887280405: 记录源端类型+id+path，松手时按类型建边。
                    connectFromKind = "node"
                    connectFromId = nd.id
                    connectFromPath = nodePath(nd.id)
                    connectFromNodeId = nd.id  // 保留给预览线绘制兼容路径
                    // 预览线起点：节点中心（世界坐标）
                    connectMouseX = nd.x + nd.width / 2
                    connectMouseY = nd.y + nd.height / 2
                    isBeingDragged = true
                    edgeCanvas.requestPaint()
                }

                onContextMenuRequested: function(sceneX, sceneY) {
                    var nd = nodesModel[index]
                    graphController.selectNode(nd.id)
                    selectedNodeForMenu = nd
                    // sceneX/sceneY 是场景坐标，菜单用屏幕坐标
                    nodeContextMenu.popup(sceneX, sceneY)
                }

                onMoveDelta: function(dx, dy) {
                    if (pointerMode === "connect" && connectFromId === nodeData.id) {
                        connectMouseX += dx
                        connectMouseY += dy
                        edgeCanvas.requestPaint()
                        return
                    }

                    if (pointerMode === "idle") {
                        pointerMode = "move"
                        pressedNodeId = nodeData.id
                    }

                    if (pointerMode === "move" && pressedNodeId === nodeData.id) {
                        x += dx
                        y += dy
                        isBeingDragged = true
                        edgeCanvas.requestPaint()
                    }
                }

                onLeftReleased: {
                    // 统一结束当前节点交互：无论长按后拖动还是直接松手，
                    // 都由此出口闭环 connect/move 状态（Issue #788 评论 5868205321）。
                    isBeingDragged = false
                    if (pointerMode === "connect" && connectFromId === nodeData.id) {
                        // Issue #796 评论 5887280405: 松手时 Node 和 Embed 都参与命中，
                        // 用 path 版建边支持 Embed 端点。
                        var targetNode = findNodeAt(connectMouseX, connectMouseY)
                        if (targetNode && targetNode.id !== connectFromId) {
                            createEdgeWithPaths(connectFromPath, nodePath(targetNode.id))
                        } else {
                            var targetEmbed = findEmbedAt(connectMouseX, connectMouseY)
                            if (targetEmbed && targetEmbed.instanceId !== connectFromId) {
                                createEdgeWithPaths(connectFromPath, embedPath(targetEmbed.instanceId))
                            }
                        }
                        pointerMode = "idle"
                        connectFromKind = ""
                        connectFromId = ""
                        connectFromPath = null
                        connectFromNodeId = ""
                        edgeCanvas.requestPaint()
                    } else if (pointerMode === "move" && pressedNodeId === nodeData.id) {
                        saveLayout()
                        pointerMode = "idle"
                        pressedNodeId = ""
                    }
                }
            }
        }

        // Issue #796 评论 5886483653: Embed Repeater，用 StarMapEmbed.qml 渲染 embedsModel。
        // Node 和 Embed 都走同一套画布坐标转换（都在 container 里，受 panX/panY/zoomLevel 影响）。
        Repeater {
            model: embedsModel.length
            delegate: StarMapEmbed {
                dt: canvasArea.dt
                property var embedData: embedsModel[index]

                x: embedData.x
                y: embedData.y
                width: embedData.width
                height: embedData.height
                instanceId: embedData.instanceId
                targetStarmapId: embedData.targetStarmapId
                label: embedData.label
                isSelected: embedData.isSelected
                wobbleIndex: index
                // Issue #796 评论 5887280405: connect 模式下阻止 DragHandler 移动 Embed。
                isConnectMode: pointerMode === "connect" && connectFromId === embedData.instanceId

                onXChanged: {
                    if (embedData) {
                        embedData.x = x
                    }
                    edgeCanvas.requestPaint()
                }

                onYChanged: {
                    if (embedData) {
                        embedData.y = y
                    }
                    edgeCanvas.requestPaint()
                }

                // 单击只选中
                onClicked: function(instId) {
                    graphController.selectEmbed(instId)
                }

                // 双击进入 targetStarmapId
                onDoubleClicked: function(tgtStarmapId) {
                    if (tgtStarmapId) {
                        var ed = embedsModel[index]
                        enterStarmapRequested(tgtStarmapId, ed.label || qsTr("子星图"))
                    }
                }

                // Issue #796 评论 5887280405: Embed 长按进入 connect 模式，
                // 与 Node 长按对称。源端类型记为 "embed"，path 用 embedPath()。
                onLongPressed: function(instId) {
                    if (pointerMode !== "idle") {
                        return
                    }
                    var ed = embedsModel[index]
                    pointerMode = "connect"
                    connectFromKind = "embed"
                    connectFromId = instId
                    connectFromPath = embedPath(instId)
                    connectFromNodeId = ""  // Embed 没有 nodeId
                    connectMouseX = ed.x + ed.width / 2
                    connectMouseY = ed.y + ed.height / 2
                    isBeingDragged = true
                    edgeCanvas.requestPaint()
                }

                // 右键上抛菜单
                onContextMenuRequested: function(instId, sceneX, sceneY) {
                    graphController.selectEmbed(instId)
                    selectedEmbedForMenu = graphController.getEmbed(instId)
                    embedContextMenu.popup(sceneX, sceneY)
                }

                // 拖动只改 Embed 的 position
                onDragged: function(instId, newX, newY) {
                    // Issue #796 评论 5887280405: connect 模式下不移动 Embed，
                    // 仅刷新预览线（起点固定，终点跟 connectMouseX/Y）。
                    if (pointerMode === "connect" && connectFromId === instId) {
                        edgeCanvas.requestPaint()
                        return
                    }
                    isBeingDragged = true
                    edgeCanvas.requestPaint()
                }

                onLeftReleased: {
                    isBeingDragged = false
                    // Issue #796 评论 5887280405: connect 模式下松手，Node 和 Embed 都参与命中，
                    // 用 path 版建边；否则走原拖动结束保存位置逻辑。
                    if (pointerMode === "connect" && connectFromId === embedData.instanceId) {
                        var targetNode = findNodeAt(connectMouseX, connectMouseY)
                        if (targetNode && targetNode.id !== connectFromId) {
                            createEdgeWithPaths(connectFromPath, nodePath(targetNode.id))
                        } else {
                            var targetEmbed = findEmbedAt(connectMouseX, connectMouseY)
                            if (targetEmbed && targetEmbed.instanceId !== connectFromId) {
                                createEdgeWithPaths(connectFromPath, embedPath(targetEmbed.instanceId))
                            }
                        }
                        pointerMode = "idle"
                        connectFromKind = ""
                        connectFromId = ""
                        connectFromPath = null
                        connectFromNodeId = ""
                        edgeCanvas.requestPaint()
                    } else {
                        // 拖动结束后保存 Embed 新位置到后端
                        var ed = embedsModel[index]
                        if (ed) {
                            graphController.updateEmbed(ed.instanceId, { position: { x: ed.x, y: ed.y } })
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
        visible: nodesModel.length === 0
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

    function loadGraph() {
        graphController.loadGraph()
    }

    function buildModels() {
        graphController.buildModels()
    }

    function autoLayout() {
        graphController.autoLayout()
    }

    function getLayoutNode(id) {
        return graphController.getLayoutNode(id)
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
        for (var j = 0; j < nodesModel.length; j++) {
            if (nodesModel[j].id === id) return nodesModel[j];
        }
        return null;
    }

    function clearSelection() {
        graphController.clearSelection()
    }

    // Issue #796 评论 5886483653: 新建子星图改回正式 Embed 语义。
    // 流程：create_starmap → create_starmap_embed；失败时删除刚创建的目标 StarMap，
    // 成功后 reload graph 并选中新 Embed。
    // 旧 portal Node 仍按已有数据正常显示/进入，不再用它创建新的子星图。
    // title 由 createDialog 收集后传入，不再写死默认名。
    function createSubStarmapAt(title, wx, wy) {
        if (!starmapBackendRef) {
            graphController.setError(qsTr("星图后端未初始化"))
            return
        }
        // 1. 创建目标子星图
        var createRes = graphController.normalizeBackendResult(
            starmapBackendRef.create_starmap(title, "", ""),
            qsTr("创建子星图失败")
        )
        if (!createRes.success) {
            graphController.setError(graphController.backendErrorText(createRes, qsTr("创建子星图失败")))
            return
        }
        var newStarmapId = createRes.data && createRes.data.starmapId ? createRes.data.starmapId : ""
        if (!newStarmapId) {
            graphController.setError(qsTr("创建子星图失败"))
            return
        }
        // 2. 在当前星图创建 Embed，指向新子星图
        var embedRes = graphController.normalizeBackendResult(
            starmapBackendRef.create_starmap_embed(starmapId, newStarmapId, title, wx, wy),
            qsTr("创建子星图入口失败")
        )
        if (!embedRes.success) {
            // 3. create_starmap_embed 失败，删除刚创建的目标 StarMap 清理
            starmapBackendRef.delete_starmap(newStarmapId)
            graphController.setError(graphController.backendErrorText(embedRes, qsTr("创建子星图入口失败")))
            return
        }
        var instanceId = embedRes.data && embedRes.data.instanceId ? embedRes.data.instanceId : ""
        // 4. 成功，reload graph 并选中新 Embed
        graphController.clearError()
        graphController.loadGraph()
        if (instanceId) {
            graphController.selectEmbed(instanceId)
        }
    }

    // Issue #790 评论 5875963057: 超链接转发给 graphController
    function addHyperlink(nodeId, url, label) {
        graphController.addHyperlink(nodeId, url, label)
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

    // Issue #796 评论 5887280405: 用 fromPath/toPath 建边，支持 Node 和 Embed 端点。
    function createEdgeWithPaths(fromPath, toPath) {
        graphController.createEdgeWithPaths(fromPath, toPath)
    }

    function saveLayout() {
        graphController.saveLayout()
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

    // 用 nodesModel 当前的 x/y/width/height 做矩形相交，四边多留 padding。
    function overlapsExistingNode(x, y, w, h, padding) {
        for (var i = 0; i < nodesModel.length; i++) {
            var n = nodesModel[i]
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
                    pointerMode = "move"
                    pressedNodeId = selectedNodeForMenu.id
                }
            }
        }

        MenuItem {
            id: nodeMenuItemHyperlink
            text: qsTr("超链接")
            contentItem: AppText {
                dt: canvasArea.dt
                text: nodeMenuItemHyperlink.text
                color: nodeMenuItemHyperlink.hovered ? _accent : _textPrimary
                font.pointSize: dt.labelPt
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItemHyperlink.hovered ? _accentSoft : "transparent"
                radius: _radiusXs
            }
            onTriggered: {
                if (selectedNodeForMenu) {
                    hyperlinkDialog.open(selectedNodeForMenu.id)
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
                    pointerMode = "move"
                    pressedEmbedId = selectedEmbedForMenu.instanceId
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

    // Issue #790 评论 5875963057: 超链接编辑 Dialog
    // Issue #796 评论 5886483653: 改用 Qt Quick Controls Popup，不再手搓整屏 Rectangle。
    Popup {
        id: hyperlinkDialog
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        width: 340
        height: 200
        anchors.centerIn: Overlay.overlay
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
        }

        property string targetNodeId: ""

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 12

            AppText {
                dt: canvasArea.dt
                text: qsTr("添加超链接")
                font.pointSize: dt.fontLgPt
                font.bold: true
                color: _textPrimary
            }

            TextField {
                id: hyperlinkUrlInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
                font.pointSize: dt.bodyPt
                placeholderText: qsTr("URL")
                text: ""

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: hyperlinkUrlInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: hyperlinkDialog.confirm()
                Keys.onEscapePressed: hyperlinkDialog.close()
            }

            TextField {
                id: hyperlinkLabelInput
                Layout.fillWidth: true
                height: 36
                color: _textPrimary
                font.pointSize: dt.bodyPt
                placeholderText: qsTr("标签（可选）")
                text: ""

                background: Rectangle {
                    color: _surfaceContainer
                    border.color: hyperlinkLabelInput.activeFocus ? _accent : _border
                    border.width: 1.5
                    radius: _radiusXs
                }

                Keys.onReturnPressed: hyperlinkDialog.confirm()
                Keys.onEscapePressed: hyperlinkDialog.close()
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 12

                Button {
                    id: hyperlinkCancelBtn
                    text: qsTr("取消")
                    onClicked: hyperlinkDialog.close()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: hyperlinkCancelBtn.text
                        color: _textSecondary
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: hyperlinkCancelBtn.hovered ? _surfaceContainer : "transparent"
                        border.color: _border
                        radius: _radiusXs
                    }
                }

                Button {
                    id: hyperlinkConfirmBtn
                    text: qsTr("确定")
                    onClicked: hyperlinkDialog.confirm()
                    contentItem: AppText {
                        dt: canvasArea.dt
                        text: hyperlinkConfirmBtn.text
                        color: _onPrimary
                        font.bold: true
                        font.pointSize: dt.labelPt
                    }
                    background: Rectangle {
                        color: hyperlinkConfirmBtn.hovered ? _accentHover : _accent
                        radius: _radiusXs
                    }
                }
            }
        }

        function open(nodeId) {
            targetNodeId = nodeId
            hyperlinkUrlInput.text = ""
            hyperlinkLabelInput.text = ""
            visible = true
            hyperlinkUrlInput.forceActiveFocus()
        }

        function close() {
            visible = false
        }

        function confirm() {
            if (targetNodeId && hyperlinkUrlInput.text.trim().length > 0) {
                addHyperlink(targetNodeId, hyperlinkUrlInput.text.trim(), hyperlinkLabelInput.text.trim())
            }
            close()
        }
    }
}
