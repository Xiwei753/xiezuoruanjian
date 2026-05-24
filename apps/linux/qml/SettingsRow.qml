import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property string title: ""
    property string description: ""
    property bool clickable: false
    signal clicked()
    default property alias controlData: controlHost.data

    color: "transparent"
    implicitHeight: dt ? dt.settingsRowHeight : 64

    RowLayout {
        anchors.fill: parent
        spacing: dt ? dt.sp12 : 12

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Text {
                text: root.title
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontMd : 14
            }
            Text {
                text: root.description
                color: dt ? dt.textSecondary : "#9CA0AB"
                font.pixelSize: dt ? dt.fontSm : 12
                visible: text.length > 0
            }
        }

        Item {
            id: controlHost
            Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
            Layout.preferredHeight: dt ? dt.settingsControlHeight : 36
        }
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: dt ? dt.border : "#2A2E36"
        opacity: 0.65
    }

    MouseArea {
        anchors.fill: parent
        enabled: root.clickable
        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: root.clicked()
    }
}
