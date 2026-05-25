import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    color: DesignTokens.colorSurface

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
            color: DesignTokens.colorText
            visible: selectedNode !== null || selectedEdge !== null
        }

        Text {
            text: "请在左侧选择节点或连线"
            color: DesignTokens.colorTextLight
            visible: selectedNode === null && selectedEdge === null
            Layout.alignment: Qt.AlignHCenter
        }

        // --- Node Inspector ---
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 12
            visible: selectedNode !== null

            AppText { text: "标题"; color: DesignTokens.colorTextLight }
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

            AppText { text: "类型"; color: DesignTokens.colorTextLight }
            ModernComboBox {
                id: kindInput
                Layout.fillWidth: true
                model: ["Project", "Volume", "Chapter", "TextAnchor", "Character", "Event", "Location", "Item", "Concept", "Theme", "Note", "Organization", "Timeline", "Plot", "Foreshadowing", "Custom"]
                currentIndex: selectedNode ? model.indexOf(selectedNode.kind) : -1
                onActivated: {
                    if (selectedNode) {
                        var k = model[index]
                        if (k !== selectedNode.kind) {
                            nodeUpdated(selectedNode.id, { kind: k })
                        }
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

            AppText { text: "类型"; color: DesignTokens.colorTextLight }
            ModernComboBox {
                id: edgeKindInput
                Layout.fillWidth: true
                model: ["Contains", "References", "AppearsIn", "Causes", "RelatedTo", "LocatedAt", "CharacterRelation", "Timeline", "Foreshadows", "Resolves", "DependsOn", "ConflictsWith", "Custom"]
                currentIndex: selectedEdge ? model.indexOf(selectedEdge.kind) : -1
                onActivated: {
                    if (selectedEdge) {
                        var k = model[index]
                        if (k !== selectedEdge.kind) {
                            edgeUpdated(selectedEdge.id, { kind: k })
                        }
                    }
                }
            }

            AppText { text: "标签"; color: DesignTokens.colorTextLight }
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
