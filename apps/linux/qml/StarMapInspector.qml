import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    color: dt ? dt.surface : "#1A1D23"

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
            text: "属性"
            font.pixelSize: 16
            font.bold: true
            color: dt ? dt.textPrimary : "#E2E4E9"
            visible: selectedNode !== null || selectedEdge !== null
        }

        Text {
            text: "请在左侧选择节点或连线"
            color: dt ? dt.textSecondary : "#9CA0AB"
            visible: selectedNode === null && selectedEdge === null
            Layout.alignment: Qt.AlignHCenter
        }

        // --- Node Inspector ---
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 12
            visible: selectedNode !== null

            AppText { text: "标题"; color: dt ? dt.textSecondary : "#9CA0AB" }
            AppTextField {
                id: titleInput
                Layout.fillWidth: true
                text: selectedNode ? selectedNode.title : ""
                onEditingFinished: {
                    if (selectedNode && text !== selectedNode.title) {
                        nodeUpdated(selectedNode.id, { title: text })
                    }
                }
            }

            Item { Layout.fillHeight: true }

            AppButton {
                text: "删除节点"
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

            AppText { text: "标签"; color: dt ? dt.textSecondary : "#9CA0AB" }
            AppTextField {
                id: labelInput
                Layout.fillWidth: true
                text: selectedEdge && selectedEdge.label ? selectedEdge.label : ""
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
                text: "删除连线"
                Layout.fillWidth: true
                onClicked: {
                    if (selectedEdge) edgeDeleted(selectedEdge.id)
                }
            }
        }
    }
}
