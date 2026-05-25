import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root
    width: parent ? parent.width : 800
    height: parent ? parent.height : 600

    property string starmapId: ""
    property string starmapTitle: "星图编辑器"
    property var dt: null
    property var backendRef: null

    signal backClicked()

    Rectangle {
        anchors.fill: parent
        color: dt ? dt.surface : "#1A1D23"
    }

    // 顶部工具栏
    Rectangle {
        id: toolbar
        width: parent.width
        height: 56
        color: dt ? dt.card : "#1E2128"
        border.color: dt ? dt.border : "#2A2E36"
        border.width: 1
        anchors.top: parent.top
        z: 10

        RowLayout {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 16

            AppButton {
                text: "返回"
                onClicked: root.backClicked()
            }

            AppText {
                text: root.starmapTitle
                font.pixelSize: 18
                font.bold: true
                color: dt ? dt.textPrimary : "#E2E4E9"
                Layout.fillWidth: true
            }

            AppButton {
                text: "新增节点"
                onClicked: canvas.createNodeAtCenter()
            }

            AppButton {
                text: canvas.isConnectingMode ? "取消连线" : "连线"
                onClicked: canvas.isConnectingMode = !canvas.isConnectingMode
                primary: canvas.isConnectingMode
            }
        }
    }

    // 画布与检查器布局
    RowLayout {
        anchors.top: toolbar.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 0

        StarMapCanvas {
            id: canvas
            Layout.fillWidth: true
            Layout.fillHeight: true
            starmapId: root.starmapId
            dt: root.dt
            backendRef: root.backendRef

            onNodeSelected: function(node) {
                inspector.selectedNode = node
                inspector.selectedEdge = null
            }
            onEdgeSelected: function(edge) {
                inspector.selectedEdge = edge
                inspector.selectedNode = null
            }
            onSelectionCleared: {
                inspector.selectedNode = null
                inspector.selectedEdge = null
            }
        }

        Rectangle {
            width: 1
            Layout.fillHeight: true
            color: dt ? dt.border : "#2A2E36"
        }

        StarMapInspector {
            id: inspector
            width: 300
            Layout.fillHeight: true
            starmapId: root.starmapId
            dt: root.dt

            onNodeUpdated: function(nodeId, patch) {
                canvas.updateNodeFromInspector(nodeId, patch)
            }
            onNodeDeleted: function(nodeId) {
                canvas.deleteNodeFromInspector(nodeId)
            }
            onEdgeUpdated: function(edgeId, patch) {
                canvas.updateEdgeFromInspector(edgeId, patch)
            }
            onEdgeDeleted: function(edgeId) {
                canvas.deleteEdgeFromInspector(edgeId)
            }
        }
    }

    Component.onCompleted: {
        if (starmapId !== "") {
            canvas.loadGraph()
        }
    }
}
