import QtQuick 2.15
import QtQuick.Controls 2.15

Item {
    id: canvasArea
    clip: true

    property string starmapId: ""
    property var dt: null
    property var backendRef: null
    property string errorMessage: ""
    property var graphData: null
    property var layoutData: null

    // View transform properties
    property real panX: 0
    property real panY: 0
    property real zoomLevel: 1.0

    // Interaction states
    property bool isConnectingMode: false
    property string connectingFromNodeId: ""
    property real mouseWorldX: 0
    property real mouseWorldY: 0

    // Signals
    signal nodeSelected(var node)
    signal edgeSelected(var edge)
    signal selectionCleared()

    // Model data
    property var nodesModel: []
    property var edgesModel: []

    function setError(msg) { errorMessage = msg || ""; console.warn("[StarMapCanvas]", errorMessage) }
    function clearError() { errorMessage = "" }
    function parseBackendJson(raw, fallbackMessage) {
        try {
            return JSON.parse(raw)
        } catch(e) {
            setError(fallbackMessage + ": " + e);
            return { success: false, message: fallbackMessage }
        }
    }
    function ensureBackend() {
        if (!backendRef) { setError("星图后端未初始化"); return false; }
        return true;
    }

    // Background Grid
    Rectangle {
        anchors.fill: parent
        color: "transparent"
        opacity: 0.3

        Canvas {
            anchors.fill: parent
            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.fillStyle = dt ? dt.textMuted : "#606470"

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
                function onPanXChanged() { parent.requestPaint() }
                function onPanYChanged() { parent.requestPaint() }
                function onZoomLevelChanged() { parent.requestPaint() }
            }
        }
    }

    // Main transform container
    Item {
        id: container
        x: panX
        y: panY
        scale: zoomLevel
        transformOrigin: Item.TopLeft

        // Edges canvas
        Canvas {
            id: edgeCanvas
            x: -panX / zoomLevel
            y: -panY / zoomLevel
            width: canvasArea.width / zoomLevel
            height: canvasArea.height / zoomLevel

            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.lineWidth = 2

                // Draw all edges
                for (var i = 0; i < edgesModel.length; i++) {
                    var edge = edgesModel[i]
                    var n1 = getNode(edge.from)
                    var n2 = getNode(edge.to)
                    if (!n1 || !n2) continue

                    var x1 = n1.x + n1.width/2
                    var y1 = n1.y + n1.height/2
                    var x2 = n2.x + n2.width/2
                    var y2 = n2.y + n2.height/2

                    ctx.beginPath()
                    ctx.moveTo(x1, y1)

                    // Simple straight line for V1
                    ctx.lineTo(x2, y2)

                    ctx.strokeStyle = edge.isSelected ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
                    ctx.stroke()

                    // Label
                    if (edge.label) {
                        ctx.fillStyle = dt ? dt.surface : "#1A1D23"
                        var midX = (x1 + x2)/2
                        var midY = (y1 + y2)/2
                        var tw = ctx.measureText(edge.label).width
                        ctx.fillRect(midX - tw/2 - 4, midY - 10, tw + 8, 20)

                        ctx.fillStyle = dt ? dt.textPrimary : "#E2E4E9"
                        ctx.font = "12px sans-serif"
                        ctx.textAlign = "center"
                        ctx.textBaseline = "middle"
                        ctx.fillText(edge.label, midX, midY)
                    }
                }

                // Draw connecting line if in progress
                if (isConnectingMode && connectingFromNodeId !== "") {
                    var startNode = getNode(connectingFromNodeId)
                    if (startNode) {
                        ctx.beginPath()
                        ctx.moveTo(startNode.x + startNode.width/2, startNode.y + startNode.height/2)
                        ctx.lineTo(mouseWorldX, mouseWorldY)
                        ctx.strokeStyle = dt ? dt.accent : "#7B8CDE"
                        ctx.setLineDash([5, 5])
                        ctx.stroke()
                        ctx.setLineDash([])
                    }
                }
            }
        }

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

                onPositionChanged: function(newX, newY) {
                    nodesModel[index].x = newX
                    nodesModel[index].y = newY
                    edgeCanvas.requestPaint()
                }

                onPositionChangeFinished: {
                    saveLayout()
                }

                onClicked: {
                    if (isConnectingMode) {
                        if (connectingFromNodeId === "") {
                            connectingFromNodeId = nodeData.id
                        } else if (connectingFromNodeId !== nodeData.id) {
                            createEdge(connectingFromNodeId, nodeData.id)
                            connectingFromNodeId = ""
                            isConnectingMode = false
                        }
                    } else {
                        clearSelection()
                        nodesModel[index].isSelected = true
                        nodeSelected(nodesModel[index])
                        nodesModelChanged()
                    }
                }
            }
        }
    }

    // Input handlers for panning/zooming
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
        property real lastMouseX: 0
        property real lastMouseY: 0

        onPressed: function(mouse) {
            lastMouseX = mouse.x
            lastMouseY = mouse.y

            if (mouse.button === Qt.LeftButton) {
                if (!isConnectingMode) {
                    clearSelection()
                }
            }
        }

        onPositionChanged: function(mouse) {
            mouseWorldX = (mouse.x - panX) / zoomLevel
            mouseWorldY = (mouse.y - panY) / zoomLevel

            if (pressedButtons & Qt.MiddleButton || (pressedButtons & Qt.LeftButton && !isConnectingMode)) {
                var dx = mouse.x - lastMouseX
                var dy = mouse.y - lastMouseY
                panX += dx
                panY += dy
                lastMouseX = mouse.x
                lastMouseY = mouse.y
            }

            if (isConnectingMode && connectingFromNodeId !== "") {
                edgeCanvas.requestPaint()
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

    // Edge clicking logic can be tricky with canvas, so we do a simple distance check on click
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton
        propagateComposedEvents: true
        onClicked: function(mouse) {
            if (isConnectingMode) return;

            mouse.accepted = false; // let background selection clearing happen

            var mx = (mouse.x - panX) / zoomLevel
            var my = (mouse.y - panY) / zoomLevel

            // Check edges
            for (var i = 0; i < edgesModel.length; i++) {
                var edge = edgesModel[i]
                var n1 = getNode(edge.from)
                var n2 = getNode(edge.to)
                if (!n1 || !n2) continue

                var x1 = n1.x + n1.width/2
                var y1 = n1.y + n1.height/2
                var x2 = n2.x + n2.width/2
                var y2 = n2.y + n2.height/2

                // distance from point to line segment
                var l2 = (x1-x2)*(x1-x2) + (y1-y2)*(y1-y2)
                var dist = 1000;
                if (l2 === 0) dist = Math.sqrt((mx-x1)*(mx-x1) + (my-y1)*(my-y1))
                else {
                    var t = ((mx - x1) * (x2 - x1) + (my - y1) * (y2 - y1)) / l2
                    t = Math.max(0, Math.min(1, t))
                    var projX = x1 + t * (x2 - x1)
                    var projY = y1 + t * (y2 - y1)
                    dist = Math.sqrt((mx - projX)*(mx - projX) + (my - projY)*(my - projY))
                }

                if (dist < 10) { // 10px threshold
                    clearSelection()
                    edgesModel[i].isSelected = true
                    edgeSelected(edgesModel[i])
                    edgeCanvas.requestPaint()
                    mouse.accepted = true
                    return
                }
            }
        }
    }

    Text {
        anchors.centerIn: parent
        text: "还没有节点，点击新增节点开始构建星图"
        color: dt ? dt.textSecondary : "#9CA0AB"
        font.pixelSize: 16
        visible: nodesModel.length === 0
    }

    Rectangle {
        id: errorBanner
        width: parent.width - 32
        height: 40
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        anchors.horizontalCenter: parent.horizontalCenter
        color: "#E53935" // explicit error color or use dt equivalent if defined
        radius: 8
        visible: errorMessage.length > 0
        z: 100

        Text {
            anchors.centerIn: parent
            text: errorMessage
            color: "white"
            font.pixelSize: 14
        }
        MouseArea {
            anchors.fill: parent
            onClicked: clearError()
        }
    }

    function loadGraph() {
        if (starmapId === "") return;
        if (!ensureBackend()) return;

        var resStr = backendRef.get_starmap_graph_json(starmapId)
        var res = parseBackendJson(resStr, "加载星图数据失败")
        if (res.success) {
            clearError()
            graphData = res.data.graph
            layoutData = res.data.layout

            buildModels()
        } else {
            setError(res.userMessage || res.message || "加载星图数据失败")
        }
    }

    function buildModels() {
        var newNodes = []
        for (var i = 0; i < graphData.nodes.length; i++) {
            var gn = graphData.nodes[i]
            var ln = getLayoutNode(gn.id)
            newNodes.push({
                id: gn.id,
                title: gn.title,
                kind: gn.kind,
                x: ln ? ln.x : 0,
                y: ln ? ln.y : 0,
                width: ln ? ln.width : 150,
                height: ln ? ln.height : 60,
                isSelected: false,
                payload: gn.payload,
                tags: gn.tags
            })
        }
        nodesModel = newNodes

        var newEdges = []
        for (var j = 0; j < graphData.edges.length; j++) {
            var ge = graphData.edges[j]
            newEdges.push({
                id: ge.id,
                from: ge.from,
                to: ge.to,
                kind: ge.kind,
                label: ge.label,
                isSelected: false
            })
        }
        edgesModel = newEdges

        // Auto layout if needed
        if (nodesModel.length > 0 && (!layoutData || !layoutData.nodes || layoutData.nodes.length === 0)) {
            autoLayout()
        }

        edgeCanvas.requestPaint()
    }

    function autoLayout() {
        var curX = 100
        var curY = 100
        for (var i = 0; i < nodesModel.length; i++) {
            nodesModel[i].x = curX
            nodesModel[i].y = curY
            curX += 200
            if (curX > 800) {
                curX = 100
                curY += 100
            }
        }
        nodesModelChanged()
        saveLayout()
    }

    function getLayoutNode(id) {
        if (!layoutData || !layoutData.nodes) return null;
        for (var i = 0; i < layoutData.nodes.length; i++) {
            if (layoutData.nodes[i].node_id === id) return layoutData.nodes[i];
        }
        return null;
    }

    function getNode(id) {
        for (var i = 0; i < nodesModel.length; i++) {
            if (nodesModel[i].id === id) return nodesModel[i];
        }
        return null;
    }

    function clearSelection() {
        for (var i = 0; i < nodesModel.length; i++) nodesModel[i].isSelected = false;
        for (var j = 0; j < edgesModel.length; j++) edgesModel[j].isSelected = false;
        nodesModelChanged()
        edgeCanvas.requestPaint()
        selectionCleared()
    }

    function createNodeAtCenter() {
        if (!ensureBackend()) return;
        var wx = (width/2 - panX) / zoomLevel
        var wy = (height/2 - panY) / zoomLevel

        var resStr = backendRef.create_starmap_node_json(starmapId, "新节点", "Note", wx, wy)
        var res = parseBackendJson(resStr, "创建节点失败")
        if (res.success) {
            clearError()
            loadGraph()
            // Select new node
            for (var i = 0; i < nodesModel.length; i++) {
                if (nodesModel[i].id === res.data.id) {
                    clearSelection()
                    nodesModel[i].isSelected = true
                    nodeSelected(nodesModel[i])
                    nodesModelChanged()
                    break
                }
            }
        } else {
            setError(res.userMessage || res.message || "创建节点失败")
        }
    }

    function createEdge(fromId, toId) {
        if (!ensureBackend()) return;
        var resStr = backendRef.create_starmap_edge_json(starmapId, fromId, toId, "RelatedTo", "")
        var res = parseBackendJson(resStr, "创建连线失败")
        if (res.success) {
            clearError()
            loadGraph()
        } else {
            setError(res.userMessage || res.message || "创建连线失败")
        }
    }

    function saveLayout() {
        if (!ensureBackend()) return;
        var lNodes = []
        for (var i = 0; i < nodesModel.length; i++) {
            var n = nodesModel[i]
            lNodes.push({
                nodeId: n.id,
                x: n.x,
                y: n.y,
                width: n.width,
                height: n.height,
                radius: 30,
                collapsed: false,
                zIndex: 0
            })
        }
        var lj = {
            kind: "Freeform",
            nodes: lNodes
        }
        var resStr = backendRef.save_starmap_layout_json(starmapId, JSON.stringify(lj))
        var res = parseBackendJson(resStr, "保存布局失败")
        if (res.success) {
            clearError()
        } else {
            setError(res.userMessage || res.message || "保存布局失败")
        }
    }

    function updateNodeFromInspector(nodeId, patch) {
        if (!ensureBackend()) return;
        var resStr = backendRef.update_starmap_node_json(starmapId, nodeId, JSON.stringify(patch))
        var res = parseBackendJson(resStr, "更新节点失败")
        if (res.success) {
            clearError()
            for (var i = 0; i < nodesModel.length; i++) {
                if (nodesModel[i].id === nodeId) {
                    if (patch.title !== undefined) nodesModel[i].title = patch.title;
                    if (patch.kind !== undefined) nodesModel[i].kind = patch.kind;
                    nodesModelChanged()
                    break
                }
            }
        } else {
            setError(res.userMessage || res.message || "更新节点失败")
        }
    }

    function deleteNodeFromInspector(nodeId) {
        if (!ensureBackend()) return;
        var resStr = backendRef.delete_starmap_node_json(starmapId, nodeId)
        var res = parseBackendJson(resStr, "删除节点失败")
        if (res.success) {
            clearError()
            loadGraph()
            clearSelection()
        } else {
            setError(res.userMessage || res.message || "删除节点失败")
        }
    }

    function updateEdgeFromInspector(edgeId, patch) {
        if (!ensureBackend()) return;
        var resStr = backendRef.update_starmap_edge_json(starmapId, edgeId, JSON.stringify(patch))
        var res = parseBackendJson(resStr, "更新连线失败")
        if (res.success) {
            clearError()
            for (var i = 0; i < edgesModel.length; i++) {
                if (edgesModel[i].id === edgeId) {
                    if (patch.label !== undefined) edgesModel[i].label = patch.label;
                    if (patch.kind !== undefined) edgesModel[i].kind = patch.kind;
                    edgeCanvas.requestPaint()
                    break
                }
            }
        } else {
            setError(res.userMessage || res.message || "更新连线失败")
        }
    }

    function deleteEdgeFromInspector(edgeId) {
        if (!ensureBackend()) return;
        var resStr = backendRef.delete_starmap_edge_json(starmapId, edgeId)
        var res = parseBackendJson(resStr, "删除连线失败")
        if (res.success) {
            clearError()
            loadGraph()
            clearSelection()
        } else {
            setError(res.userMessage || res.message || "删除连线失败")
        }
    }
}
