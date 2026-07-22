// =============================================================================
// StarMapInspector.qml — 星图检查器面板
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：展示和编辑选中节点/边的属性（标题、类型、描述等）
// 约束：
//   - 纯 UI 组件，属性变更通过 signal 传递给 StarMapGraphController
//   - 不直接操作文件系统或 Core 层
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var textCoordinator: null
    color: dt.surface

    property string starmapId: ""
    property var selectedNode: null
    property var selectedEdge: null

    signal nodeUpdated(string nodeId, var patch)
    signal nodeDeleted(string nodeId)
    signal edgeUpdated(string edgeId, var patch)
    signal edgeDeleted(string edgeId)

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 16

        AppText {
            dt: root.dt
            text: qsTr("属性")
            font.pixelSize: 16
            font.bold: true
            color: dt.textPrimary
            visible: selectedNode !== null || selectedEdge !== null
        }

        AppText {
            dt: root.dt
            text: qsTr("请在左侧选择节点或连线")
            color: dt.textSecondary
            visible: selectedNode === null && selectedEdge === null
            Layout.alignment: Qt.AlignHCenter
        }

        // --- Node Inspector ---
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 12
            visible: selectedNode !== null

            AppText { dt: root.dt; text: qsTr("标题"); color: dt.textSecondary }
            CoordinatorTextField {
                id: titleInput
                Layout.fillWidth: true
                text: selectedNode ? selectedNode.title : ""
                coordinator: root.textCoordinator
                targetId: "starmap-node-title"
                onEditingFinished: {
                    if (selectedNode && text !== selectedNode.title) {
                        nodeUpdated(selectedNode.id, { title: text })
                    }
                }
            }

            Item { Layout.fillHeight: true }

            AppButton {
                dt: root.dt
                text: qsTr("删除节点")
                Layout.fillWidth: true
                onClicked: {
                    if (selectedNode) nodeDeleted(selectedNode.id)
                }
            }
        }

        // --- Edge Inspector ---
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 12
            visible: selectedEdge !== null

            AppText { dt: root.dt; text: qsTr("标签"); color: dt.textSecondary }
            CoordinatorTextField {
                id: labelInput
                Layout.fillWidth: true
                text: selectedEdge && selectedEdge.label ? selectedEdge.label : ""
                coordinator: root.textCoordinator
                targetId: "starmap-edge-label"
                onEditingFinished: {
                    if (selectedEdge) {
                        var l = text
                        if (selectedEdge.label !== l) {
                            edgeUpdated(selectedEdge.id, { label: l })
                        }
                    }
                }
            }

            Item { Layout.fillHeight: true }

            AppButton {
                dt: root.dt
                text: qsTr("删除连线")
                Layout.fillWidth: true
                onClicked: {
                    if (selectedEdge) edgeDeleted(selectedEdge.id)
                }
            }
        }
    }
}
