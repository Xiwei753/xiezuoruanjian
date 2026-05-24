import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: root
    property var dt: null
    property string title: ""
    property string subtitle: ""
    default property alias actions: actionRow.data

    RowLayout {
        anchors.fill: parent
        spacing: dt ? dt.sp16 : 16

        ColumnLayout {
            Layout.fillWidth: true
            spacing: dt ? dt.sp6 : 6

            Text {
                text: root.title
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontTitle : 26
                font.weight: Font.Bold
            }

            Text {
                text: root.subtitle
                color: dt ? dt.textSecondary : "#9CA0AB"
                font.pixelSize: dt ? dt.fontMd : 14
                visible: text.length > 0
            }
        }

        RowLayout {
            id: actionRow
            spacing: dt ? dt.sp10 : 10
            Layout.alignment: Qt.AlignVCenter
        }
    }
}
