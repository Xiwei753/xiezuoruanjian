// =============================================================================
// StarMapNode.qml — 星图节点组件
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
// 职责：单个星图节点的可视化渲染、拖拽交互、选中状态展示
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 节点位置变化通过 signal 传递给 StarMapCanvas
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property var dt: null
    property string title: "Node"
    property string kind: "Note"
    property bool isSelected: false

    signal positionChanged(real newX, real newY)
    signal positionChangeFinished()
    signal clicked()

    radius: 8
    color: dt ? dt.surface : "#1A1D23"
    border.color: isSelected ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
    border.width: isSelected ? 2 : 1

    // Shadow effect approximation
    Rectangle {
        anchors.fill: parent
        anchors.margins: -1
        z: -1
        color: "transparent"
        border.color: Qt.rgba(0,0,0,0.1)
        radius: root.radius + 1
        visible: !isSelected
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 4

        Rectangle {
            Layout.fillWidth: true
            height: 16
            color: getKindColor(root.kind)
            radius: 4

            Text {
                anchors.centerIn: parent
                text: root.kind
                color: "white"
                font.pixelSize: 10
                font.bold: true
            }
        }

        Text {
            Layout.fillWidth: true
            Layout.fillHeight: true
            text: root.title
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: 12
            wrapMode: Text.Wrap
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }

    signal rightPressed(real mouseX, real mouseY)
    signal rightDragged(real worldX, real worldY)
    signal rightReleased(real worldX, real worldY)

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        
        property point clickPos: "0,0"

        onPressed: function(mouse) {
            if (mouse.button === Qt.RightButton) {
                root.rightPressed(mouse.x, mouse.y)
            } else if (mouse.button === Qt.LeftButton) {
                clickPos = Qt.point(mouse.x, mouse.y)
                root.clicked()
            }
        }

        onPositionChanged: function(mouse) {
            if (pressedButtons & Qt.RightButton) {
                var mapped = mapToItem(root.parent, mouse.x, mouse.y)
                root.rightDragged(mapped.x, mapped.y)
            } else if (pressedButtons & Qt.LeftButton) {
                var dx = mouse.x - clickPos.x
                var dy = mouse.y - clickPos.y
                var zoom = (root.parent && root.parent.scale) ? root.parent.scale : 1.0
                root.x += dx / zoom
                root.y += dy / zoom
                root.positionChanged(root.x, root.y)
            }
        }

        onReleased: function(mouse) {
            if (mouse.button === Qt.RightButton) {
                var mapped = mapToItem(root.parent, mouse.x, mouse.y)
                root.rightReleased(mapped.x, mapped.y)
            } else if (mouse.button === Qt.LeftButton) {
                root.positionChangeFinished()
            }
        }
    }

    function getKindColor(k) {
        switch(k) {
            case "Chapter": return "#4CAF50"
            case "Character": return "#2196F3"
            case "Location": return "#FF9800"
            case "Event": return "#F44336"
            case "Concept": return "#9C27B0"
            default: return dt ? dt.textMuted : "#606470"
        }
    }

    function getKindLabel(k) {
        switch(k) {
            case "Note": return "笔记"
            case "Chapter": return "章节"
            case "Character": return "角色"
            case "Location": return "地点"
            case "Event": return "事件"
            case "Concept": return "概念"
            case "Project": return "作品"
            case "Volume": return "卷"
            case "Custom": return "自定义"
            default: return k
        }
    }
}
