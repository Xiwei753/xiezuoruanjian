// =============================================================================
// StarMapCanvas.qml — 星图画布
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
// 职责：星图可视化渲染、平移/缩放交互、节点拖拽、连线模式、右键菜单
// 约束：
//   - 纯渲染和交互层，业务逻辑委托给 StarMapGraphController
//   - 节点/边增删改通过 signal 传递给 StarMapGraphController
//   - 使用 Canvas 进行自定义绘制
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: canvasArea
    clip: true

    property string starmapId: ""
    property var dt: null
    property var backendRef: null
    property string errorMessage: graphController.errorMessage

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

    // New right-click gesture & context menu properties
    property real rightPressStartX: 0
    property real rightPressStartY: 0
    property bool isRightDraggingGesture: false
    property string gestureStartNodeId: ""
    property var selectedNodeForMenu: null
    property var selectedEdgeForMenu: null
    property real contextMenuWorldX: 0
    property real contextMenuWorldY: 0

    StarMapGraphController {
        id: graphController
        starmapId: canvasArea.starmapId
        backendRef: canvasArea.backendRef
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
                function onPanXChanged() { gridCanvas.requestPaint() }
                function onPanYChanged() { gridCanvas.requestPaint() }
                function onZoomLevelChanged() { gridCanvas.requestPaint() }
            }
        }
    }

    // Single unified Fullscreen MouseArea for Panning, Zooming, and Edge selection/right-clicks
    MouseArea {
        id: canvasMouseArea
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
        property real lastMouseX: 0
        property real lastMouseY: 0

        onPressed: function(mouse) {
            lastMouseX = mouse.x
            lastMouseY = mouse.y

            if (mouse.button === Qt.RightButton) {
                rightPressStartX = mouse.x
                rightPressStartY = mouse.y
            }
        }

        onPositionChanged: function(mouse) {
            mouseWorldX = (mouse.x - panX) / zoomLevel
            mouseWorldY = (mouse.y - panY) / zoomLevel

            // Pan canvas if left button or middle button is dragged
            if (pressedButtons & Qt.MiddleButton || (pressedButtons & Qt.LeftButton && !isConnectingMode)) {
                var dx = mouse.x - lastMouseX
                var dy = mouse.y - lastMouseY
                panX += dx
                panY += dy
                lastMouseX = mouse.x
                lastMouseY = mouse.y
            }
        }

        onReleased: function(mouse) {
            if (isConnectingMode) return;

            var dx = mouse.x - rightPressStartX
            var dy = mouse.y - rightPressStartY
            var dist = Math.sqrt(dx * dx + dy * dy)

            // We only process clicks (drag distance < 8px)
            if (dist < 8) {
                var mx = (mouse.x - panX) / zoomLevel
                var my = (mouse.y - panY) / zoomLevel

                // 1. Check if clicking on/near an edge
                var clickedEdge = null
                for (var i = 0; i < edgesModel.length; i++) {
                    var edge = edgesModel[i]
                    var n1 = getNode(edge.from)
                    var n2 = getNode(edge.to)
                    if (!n1 || !n2) continue

                    var x1 = n1.x + n1.width/2
                    var y1 = n1.y + n1.height/2
                    var x2 = n2.x + n2.width/2
                    var y2 = n2.y + n2.height/2

                    var l2 = (x1-x2)*(x1-x2) + (y1-y2)*(y1-y2)
                    var edgeDist = 1000;
                    if (l2 === 0) edgeDist = Math.sqrt((mx-x1)*(mx-x1) + (my-y1)*(my-y1))
                    else {
                        var t = ((mx - x1) * (x2 - x1) + (my - y1) * (y2 - y1)) / l2
                        t = Math.max(0, Math.min(1, t))
                        var projX = x1 + t * (x2 - x1)
                        var projY = y1 + t * (y2 - y1)
                        edgeDist = Math.sqrt((mx - projX)*(mx - projX) + (my - projY)*(my - projY))
                    }

                    if (edgeDist < 10) {
                        clickedEdge = edge
                        break
                    }
                }

                if (clickedEdge) {
                    if (mouse.button === Qt.RightButton) {
                        selectedEdgeForMenu = clickedEdge
                        edgeContextMenu.popup(mouse.x, mouse.y)
                    } else if (mouse.button === Qt.LeftButton) {
                        graphController.selectEdge(clickedEdge.id)
                    }
                } else {
                    // Clicked on empty background
                    if (mouse.button === Qt.RightButton) {
                        contextMenuWorldX = mx
                        contextMenuWorldY = my
                        bgContextMenu.popup(mouse.x, mouse.y)
                    } else if (mouse.button === Qt.LeftButton) {
                        clearSelection()
                    }
                }
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



    // Edges canvas (now fullscreen, translated/scaled dynamically to prevent panning drifts)
    Canvas {
        id: edgeCanvas
        anchors.fill: parent

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

                var dx = x2 - x1
                var dy = y2 - y1
                var len = Math.sqrt(dx * dx + dy * dy)
                if (len === 0) continue

                // Check if a bidirectional edge exists
                var hasBi = false
                for (var k = 0; k < edgesModel.length; k++) {
                    if (edgesModel[k].from === edge.to && edgesModel[k].to === edge.from) {
                        hasBi = true
                        break
                    }
                }

                var offset = hasBi ? 12 : 0
                var ox = 0
                var oy = 0
                if (hasBi) {
                    var px = -dy / len
                    var py = dx / len
                    ox = px * offset
                    oy = py * offset
                }

                // Stop the arrow slightly away from the target node boundaries so the arrowhead is fully visible
                var arrowDist = 42
                var sx = x1 + ox + (dx / len) * arrowDist
                var sy = y1 + oy + (dy / len) * arrowDist
                var ax = x2 + ox - (dx / len) * arrowDist
                var ay = y2 + oy - (dy / len) * arrowDist

                // Draw line
                ctx.beginPath()
                ctx.moveTo(sx, sy)
                ctx.lineTo(ax, ay)
                ctx.strokeStyle = edge.isSelected ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
                ctx.stroke()

                // Draw arrow head at (ax, ay) pointing towards the node center
                var angle = Math.atan2(dy, dx)
                var arrowLength = 10
                ctx.beginPath()
                ctx.moveTo(ax, ay)
                ctx.lineTo(ax - arrowLength * Math.cos(angle - Math.PI / 6), ay - arrowLength * Math.sin(angle - Math.PI / 6))
                ctx.lineTo(ax - arrowLength * Math.cos(angle + Math.PI / 6), ay - arrowLength * Math.sin(angle + Math.PI / 6))
                ctx.closePath()
                ctx.fillStyle = edge.isSelected ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
                ctx.fill()

                // Label
                if (edge.label) {
                    ctx.fillStyle = dt ? dt.surface : "#1A1D23"
                    var midX = (sx + ax)/2
                    var midY = (sy + ay)/2
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
                    ctx.strokeStyle = "white"
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

                onPositionChangeFinished: {
                    saveLayout()
                }

                onClicked: {
                    graphController.selectNode(nodesModel[index].id)
                }

                onRightPressed: function(mouseX, mouseY) {
                    graphController.selectNode(nodesModel[index].id)

                    rightPressStartX = mouseX
                    rightPressStartY = mouseY
                    isRightDraggingGesture = false
                    gestureStartNodeId = nodeData.id
                }

                onRightDragged: function(worldX, worldY) {
                    var dx = worldX - (nodeData.x + rightPressStartX)
                    var dy = worldY - (nodeData.y + rightPressStartY)
                    var dist = Math.sqrt(dx * dx + dy * dy)
                    if (dist > 8) {
                        isRightDraggingGesture = true
                        isConnectingMode = true
                        connectingFromNodeId = gestureStartNodeId
                        mouseWorldX = worldX
                        mouseWorldY = worldY
                        edgeCanvas.requestPaint()
                    }
                }

                onRightReleased: function(worldX, worldY) {
                    if (isRightDraggingGesture) {
                        var targetNode = findNodeAt(worldX, worldY)
                        if (targetNode && targetNode.id !== gestureStartNodeId) {
                            createEdge(gestureStartNodeId, targetNode.id)
                        }
                        isConnectingMode = false
                        connectingFromNodeId = ""
                        isRightDraggingGesture = false
                        edgeCanvas.requestPaint()
                    } else {
                        selectedNodeForMenu = nodeData
                        nodeContextMenu.popup(worldX * zoomLevel + panX, worldY * zoomLevel + panY)
                    }
                }
            }
        }
    }



    AppText {
        anchors.centerIn: parent
        text: qsTr("还没有节点，点击新增节点开始构建星图")
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

        AppText {
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

    function createNodeAtCenter() {
        var wx = (width/2 - panX) / zoomLevel
        var wy = (height/2 - panY) / zoomLevel
        graphController.createNode(wx, wy)
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
            color: dt ? dt.card : "#1E2128"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1
            radius: 8
        }

        MenuItem {
            id: bgMenuItem1
            text: qsTr("新建节点")
            contentItem: AppText {
                text: bgMenuItem1.text
                color: bgMenuItem1.hovered ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.textPrimary : "#E2E4E9")
                font.pixelSize: 13
                font.bold: true
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: bgMenuItem1.hovered ? Qt.rgba(123, 140, 222, 0.15) : "transparent"
                radius: 6
            }
            onTriggered: createNodeAtWorld(contextMenuWorldX, contextMenuWorldY)
        }
    }

    Menu {
        id: nodeContextMenu
        
        background: Rectangle {
            implicitWidth: 150
            color: dt ? dt.card : "#1E2128"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1
            radius: 8
        }

        MenuItem {
            id: nodeMenuItem1
            text: qsTr("重命名")
            contentItem: AppText {
                text: nodeMenuItem1.text
                color: nodeMenuItem1.hovered ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.textPrimary : "#E2E4E9")
                font.pixelSize: 13
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItem1.hovered ? Qt.rgba(123, 140, 222, 0.15) : "transparent"
                radius: 6
            }
            onTriggered: {
                if (selectedNodeForMenu) {
                    renameDialog.open("node", selectedNodeForMenu.id, selectedNodeForMenu.title)
                }
            }
        }

        MenuItem {
            id: nodeMenuItem2
            text: qsTr("删除节点")
            contentItem: AppText {
                text: nodeMenuItem2.text
                color: nodeMenuItem2.hovered ? "#FF4D4D" : (dt ? dt.textPrimary : "#E2E4E9")
                font.pixelSize: 13
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: nodeMenuItem2.hovered ? Qt.rgba(255, 77, 77, 0.15) : "transparent"
                radius: 6
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
            color: dt ? dt.card : "#1E2128"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1
            radius: 8
        }

        MenuItem {
            id: edgeMenuItem1
            text: qsTr("重命名连线")
            contentItem: AppText {
                text: edgeMenuItem1.text
                color: edgeMenuItem1.hovered ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.textPrimary : "#E2E4E9")
                font.pixelSize: 13
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem1.hovered ? Qt.rgba(123, 140, 222, 0.15) : "transparent"
                radius: 6
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
                text: edgeMenuItem2.text
                color: edgeMenuItem2.hovered ? "#FF4D4D" : (dt ? dt.textPrimary : "#E2E4E9")
                font.pixelSize: 13
                verticalAlignment: Text.AlignVCenter
                leftPadding: 12
            }
            background: Rectangle {
                color: edgeMenuItem2.hovered ? Qt.rgba(255, 77, 77, 0.15) : "transparent"
                radius: 6
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
        color: Qt.rgba(0,0,0,0.6)
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
            color: dt ? dt.card : "#1E2128"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1.5
            radius: 12
            anchors.centerIn: parent

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 20
                spacing: 16

                AppText {
                    text: renameDialog.targetType === "node" ? qsTr("修改节点标题") : qsTr("修改连线标签")
                    font.pixelSize: 16
                    font.bold: true
                    color: dt ? dt.textPrimary : "#E2E4E9"
                }

                TextField {
                    id: renameInput
                    Layout.fillWidth: true
                    height: 36
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: 14
                    focus: renameDialog.visible
                    text: renameDialog.initialText

                    background: Rectangle {
                        color: dt ? dt.surface : "#1A1D23"
                        border.color: renameInput.activeFocus ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
                        border.width: 1.5
                        radius: 6
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
                            text: cancelBtn.text
                            color: dt ? dt.textSecondary : "#9CA0AB"
                            font.pixelSize: 13
                        }
                        background: Rectangle {
                            color: cancelBtn.hovered ? (dt ? dt.surface : "#1A1D23") : "transparent"
                            border.color: dt ? dt.border : "#2A2E36"
                            radius: 6
                        }
                    }

                    Button {
                        id: confirmBtn
                        text: qsTr("确定")
                        onClicked: renameDialog.confirm()
                        contentItem: AppText {
                            text: confirmBtn.text
                            color: "white"
                            font.bold: true
                            font.pixelSize: 13
                        }
                        background: Rectangle {
                            color: confirmBtn.hovered ? (dt ? dt.accentHover ? dt.accentHover : "#5A6BC8" : "#5A6BC8") : (dt ? dt.accent : "#7B8CDE")
                            radius: 6
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
}
