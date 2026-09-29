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

    // 上下文菜单辅助状态
    property var selectedNodeForMenu: null
    property var selectedEdgeForMenu: null
    property real contextMenuWorldX: 0
    property real contextMenuWorldY: 0

    // Signals
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal selectionCleared()
    signal enterStarmapRequested(string starmapId, string title)
    signal editNodeRequested(var node)

    // Model data
    property var nodesModel: []
    property var edgesModel: []

    StarMapGraphController {
        id: graphController
        starmapId: canvasArea.starmapId
        starmapBackendRef: canvasArea.starmapBackendRef
        onGraphChanged: {
            canvasArea.nodesModel = graphController.nodesModel
            canvasArea.edgesModel = graphController.edgesModel
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
        TapHandler {
            id: backgroundLeftTap
            acceptedButtons: Qt.LeftButton
            onSingleTapped: function(eventPoint) {
                var mx = (eventPoint.position.x - panX) / zoomLevel
                var my = (eventPoint.position.y - panY) / zoomLevel
                if (findNodeAt(mx, my)) {
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
        TapHandler {
            id: backgroundRightTap
            acceptedButtons: Qt.RightButton
            onSingleTapped: function(eventPoint) {
                var mx = (eventPoint.position.x - panX) / zoomLevel
                var my = (eventPoint.position.y - panY) / zoomLevel
                if (findNodeAt(mx, my)) {
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
                    if (!findNodeAt(wx, wy)) {
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
            if (pointerMode === "connect" && connectFromNodeId !== "") {
                var startNode = getNode(connectFromNodeId)
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

                // Idle wobble 视觉偏移
                property real wobbleOffsetX: 0
                property real wobbleOffsetY: 0

                // 用 index 错开 phase，避免所有节点同步晃
                SequentialAnimation on wobbleOffsetX {
                    loops: Animation.Infinite
                    NumberAnimation { to: 2; duration: 2100 + (index % 7) * 300; easing.type: Easing.InOutSine }
                    NumberAnimation { to: -2; duration: 2100 + (index % 7) * 300; easing.type: Easing.InOutSine }
                }
                SequentialAnimation on wobbleOffsetY {
                    loops: Animation.Infinite
                    NumberAnimation { to: 1.2; duration: 2800 + (index % 5) * 200; easing.type: Easing.InOutSine }
                    NumberAnimation { to: -1.2; duration: 2800 + (index % 5) * 200; easing.type: Easing.InOutSine }
                }

                // 拖动时停止 wobble，idle 时叠加偏移
                transform: Translate {
                    x: isBeingDragged ? 0 : wobbleOffsetX
                    y: isBeingDragged ? 0 : wobbleOffsetY
                }

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
                    connectFromNodeId = nd.id
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
                    if (pointerMode === "connect" && connectFromNodeId === nodeData.id) {
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
                    if (pointerMode === "connect" && connectFromNodeId === nodeData.id) {
                        var target = findNodeAt(connectMouseX, connectMouseY)
                        if (target && target.id !== connectFromNodeId) {
                            createEdge(connectFromNodeId, target.id)
                        }
                        pointerMode = "idle"
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

    // Issue #790 评论 5875963057: 新建子星图（右键空白处）
    function createSubStarmapAt(wx, wy) {
        if (!starmapBackendRef) {
            graphController.setError(qsTr("星图后端未初始化"))
            return
        }
        // 1. 创建目标星图（QJsonObject 版，返回 {success, data:{id,...}}）
        var createRes = graphController.normalizeBackendResult(
            starmapBackendRef.create_starmap(qsTr("子星图"), "", ""),
            qsTr("创建子星图失败")
        )
        if (!createRes.success) {
            graphController.setError(graphController.backendErrorText(createRes, qsTr("创建子星图失败")))
            return
        }
        var targetStarmapId = createRes.data && createRes.data.starmapId ? createRes.data.starmapId : ""
        if (!targetStarmapId) {
            graphController.setError(qsTr("创建子星图失败"))
            return
        }
        // 2. 在当前星图创建入口节点
        var nodeRes = graphController.normalizeBackendResult(
            starmapBackendRef.create_starmap_node(starmapId, qsTr("入口节点"), "Note", wx, wy),
            qsTr("创建入口节点失败")
        )
        if (!nodeRes.success) {
            // 回滚：删掉刚创建的目标星图，不留孤儿
            starmapBackendRef.delete_starmap(targetStarmapId)
            graphController.setError(graphController.backendErrorText(nodeRes, qsTr("创建入口节点失败")))
            return
        }
        var nodeId = nodeRes.data && nodeRes.data.id ? nodeRes.data.id : ""
        if (!nodeId) {
            starmapBackendRef.delete_starmap(targetStarmapId)
            graphController.setError(qsTr("创建入口节点失败"))
            return
        }
        // 3. 写 portal，指向目标星图
        var portalPatch = { portal: { destinationStarmapId: targetStarmapId, destinationTarget: null } }
        var updateRes = graphController.normalizeBackendResult(
            starmapBackendRef.update_starmap_node(starmapId, nodeId, JSON.stringify(portalPatch)),
            qsTr("写入子星图入口失败")
        )
        if (!updateRes.success) {
            starmapBackendRef.delete_starmap_node(starmapId, nodeId)
            starmapBackendRef.delete_starmap(targetStarmapId)
            graphController.setError(graphController.backendErrorText(updateRes, qsTr("写入子星图入口失败")))
            return
        }
        // 4. 成功，刷新
        graphController.clearError()
        graphController.loadGraph()
    }

    // Issue #790 评论 5875963057: 超链接转发给 graphController
    function addHyperlink(nodeId, url, label) {
        graphController.addHyperlink(nodeId, url, label)
    }

    function createEdge(fromId, toId) {
        graphController.createEdge(fromId, toId)
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

    // Helper to create node at world coordinates
    function createNodeAtWorld(wx, wy) {
        graphController.createNode(wx, wy)
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
            onTriggered: createNodeAtWorld(contextMenuWorldX, contextMenuWorldY)
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
            onTriggered: createSubStarmapAt(contextMenuWorldX, contextMenuWorldY)
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

    // 简易美观的重命名 Dialog
    Rectangle {
        id: renameDialog
        anchors.fill: parent
        color: _scrim
        visible: false
        z: 9999

        property string targetType: "" // "node" or "edge"
        property string targetId: ""
        property string initialText: ""

        // Prevent mouse clicks from propagating to canvas
        MouseArea { anchors.fill: parent }

        Rectangle {
            width: 300
            height: 160
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
            anchors.centerIn: parent

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 20
                spacing: 16

                AppText {
                    dt: canvasArea.dt
                    text: renameDialog.targetType === "node" ? qsTr("修改节点标题") : qsTr("修改连线标签")
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
            }
            close()
        }
    }

    // Issue #790 评论 5875963057: 超链接编辑 Dialog
    Rectangle {
        id: hyperlinkDialog
        anchors.fill: parent
        color: _scrim
        visible: false
        z: 9999

        property string targetNodeId: ""

        // Prevent mouse clicks from propagating to canvas
        MouseArea { anchors.fill: parent }

        Rectangle {
            width: 340
            height: 200
            color: _card
            border.color: _border
            border.width: 1.5
            radius: _dialogRadius
            anchors.centerIn: parent

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
