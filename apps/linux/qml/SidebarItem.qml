import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: control
    property var theme: null
    property string text: ""
    property string icon: ""
    property bool active: false
    property bool compact: false
    signal clicked()

    height: 36
    implicitWidth: 160

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: control.theme ? control.theme.sp8 : 8
        anchors.rightMargin: control.theme ? control.theme.sp8 : 8
        radius: control.theme ? control.theme.radiusSm : 4
        color: {
            if (control.active) return control.theme ? control.theme.selected : "#0c4a6e"
            return ma.containsMouse ? (control.theme ? control.theme.hover : "#1e293b") : "transparent"
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: control.theme ? control.theme.sp12 : 12
        anchors.rightMargin: control.theme ? control.theme.sp8 : 8
        spacing: control.theme ? control.theme.sp8 : 8

        Text {
            text: control.icon
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            Layout.preferredWidth: 20
            horizontalAlignment: Text.AlignHCenter
            visible: !control.compact
        }

        Text {
            text: control.text
            color: {
                if (control.active) return control.theme ? control.theme.selectedText : "#e2e8f0"
                return control.theme ? control.theme.textPrimary : "#0f172a"
            }
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            font.weight: control.active ? Font.Medium : Font.Normal
            Layout.fillWidth: true
            elide: Text.ElideRight
            visible: !control.compact
        }
    }

    MouseArea {
        id: ma
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: control.clicked()
    }
}
