import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    property string title: "Node"
    property string kind: "Note"
    property bool isSelected: false

    signal positionChanged(real newX, real newY)
    signal positionChangeFinished()
    signal clicked()

    radius: 8
    color: DesignTokens.colorSurface
    border.color: isSelected ? DesignTokens.colorPrimary : DesignTokens.colorBorder
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
            color: DesignTokens.colorText
            font.pixelSize: 12
            wrapMode: Text.Wrap
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }

    MouseArea {
        anchors.fill: parent
        drag.target: root
        drag.axis: Drag.XAndYAxis

        onClicked: root.clicked()

        onPositionChanged: function(mouse) {
            if (drag.active) {
                root.positionChanged(root.x, root.y)
            }
        }

        onReleased: function(mouse) {
            if (drag.active) {
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
            default: return DesignTokens.colorTextLight
        }
    }
}
