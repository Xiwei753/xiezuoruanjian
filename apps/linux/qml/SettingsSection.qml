import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property string title: ""
    default property alias contentData: rows.data

    radius: dt ? dt.radiusMd : 12
    color: dt ? dt.card : "#1E2128"
    border.color: dt ? dt.border : "#2A2E36"
    border.width: 1
    implicitHeight: contentCol.implicitHeight

    ColumnLayout {
        id: contentCol
        anchors.fill: parent
        anchors.margins: dt ? dt.sp16 : 16
        spacing: dt ? dt.sp10 : 10

        Text {
            text: root.title
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: dt ? dt.fontLg : 16
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }

        ColumnLayout {
            id: rows
            Layout.fillWidth: true
            spacing: 0
        }
    }
}
