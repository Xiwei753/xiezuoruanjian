import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property string title: ""
    default property alias contentData: body.data
    implicitHeight: 220
    radius: dt ? dt.radiusMd : 12
    color: dt ? dt.card : "#1E2128"
    border.color: dt ? dt.border : "#2A2E36"
    border.width: 1

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp16 : 16
        spacing: dt ? dt.sp12 : 12

        Text {
            text: root.title
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontSm : 12
            font.weight: Font.DemiBold
        }

        ColumnLayout {
            id: body
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: dt ? dt.sp8 : 8
        }
    }
}
