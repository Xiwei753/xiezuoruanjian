// =============================================================================
// StarMapNode.qml — 星图节点组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个星图节点的可视化渲染、拖拽交互、选中状态展示
// 约束：
//   - 纯 UI 组件，数据通过 property 传入
//   - 节点位置变化通过 signal 传递给 StarMapCanvas
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root

    property var dt: null

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _accent: dt.accent
    readonly property color _border: dt.border
    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _shadowLight: dt.shadowLight
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textMuted: dt.textMuted
    readonly property int _radiusXs: dt.radiusXs
    readonly property int _radiusSm: dt.radiusSm

    property string title: "Node"
    property string kind: "Note"
    property bool isSelected: false

    signal positionChanged(real newX, real newY)
    signal positionChangeFinished()
    signal clicked()

    radius: _radiusSm
    color: _surfaceContainer
    border.color: isSelected ? _accent : _border
    border.width: isSelected ? 2 : 1

    // Shadow effect approximation
    Rectangle {
        anchors.fill: parent
        anchors.margins: -1
        z: -1
        color: "transparent"
        border.color: _shadowLight
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
            radius: _radiusXs

            AppText {
                dt: root.dt
                anchors.centerIn: parent
                text: root.kind
                color: _onPrimary
                font.pixelSize: 10
                font.bold: true
            }
        }

        AppText {
            dt: root.dt
            Layout.fillWidth: true
            Layout.fillHeight: true
            text: root.title
            color: _textPrimary
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
            case "Chapter": return dt.starMapNodeChapter
            case "Character": return dt.starMapNodeCharacter
            case "Location": return dt.starMapNodeLocation
            case "Event": return dt.starMapNodeEvent
            case "Concept": return dt.starMapNodeConcept
            default: return _textMuted
        }
    }

    function getKindLabel(k) {
        switch(k) {
            case "Note": return qsTr("笔记")
            case "Chapter": return qsTr("章节")
            case "Character": return qsTr("角色")
            case "Location": return qsTr("地点")
            case "Event": return qsTr("事件")
            case "Concept": return qsTr("概念")
            case "Project": return qsTr("作品")
            case "Volume": return qsTr("卷")
            case "Custom": return qsTr("自定义")
            default: return k
        }
    }
}
