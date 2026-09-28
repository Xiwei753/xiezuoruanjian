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

    signal backClicked()
    signal enterStarmapRequested(string starmapId, string title)

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
                        onClicked: root.backClicked()
                    }

                    AppText {
                        dt: root.dt
                        text: root.starmapTitle
                        color: dt.onSurface
                        font.pointSize: dt.fontLgPt
                        font.family: dt.fontFamily
                        font.weight: Font.DemiBold
                        Layout.alignment: Qt.AlignVCenter
                    }

                    Item { Layout.fillWidth: true }

                    AppButton {
                        dt: root.dt
                        variant: "primary"
                        text: qsTr("+ 新建节点")
                        onClicked: canvas.createNodeAtCenter()
                    }

                    AppButton {
                        dt: root.dt
                        variant: "secondary"
                        text: qsTr("自动布局")
                        onClicked: canvas.autoLayout()
                    }

                    AppButton {
                        dt: root.dt
                        variant: "secondary"
                        text: qsTr("放大")
                        onClicked: canvas.zoomLevel = Math.min(2.5, canvas.zoomLevel + 0.2)
                    }

                    AppButton {
                        dt: root.dt
                        variant: "secondary"
                        text: qsTr("缩小")
                        onClicked: canvas.zoomLevel = Math.max(0.35, canvas.zoomLevel - 0.2)
                    }

                    AppButton {
                        dt: root.dt
                        variant: "secondary"
                        text: qsTr("重置视图")
                        onClicked: {
                            canvas.panX = 0
                            canvas.panY = 0
                            canvas.zoomLevel = 1.0
                        }
                    }
                }
            }

            // 主体：左侧画布 + 右侧检查器
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                StarMapCanvas {
                    id: canvas
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    dt: root.dt
                    starmapId: root.starmapId
                    starmapBackendRef: root.starmapBackendRef

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
                    onEnterStarmapRequested: function(smId, smTitle) {
                        root.enterStarmapRequested(smId, smTitle)
                    }
                }

                StarMapInspector {
                    id: inspector
                    Layout.preferredWidth: 300
                    Layout.fillHeight: true
                    dt: root.dt
                    starmapId: root.starmapId

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
        }
    }

    // 星图切换时重新加载
    onStarmapIdChanged: {
        if (starmapId.length > 0) canvas.loadGraph()
    }

    Component.onCompleted: {
        if (starmapId.length > 0) canvas.loadGraph()
    }
}
