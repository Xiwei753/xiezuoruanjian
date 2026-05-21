import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: control
    property var theme: null
    property alias spacing: col.spacing

    implicitWidth: 200
    implicitHeight: 60
    radius: control.theme ? control.theme.radiusMd : 8
    color: control.theme ? control.theme.surface : "#ffffff"
    border.color: control.theme ? control.theme.border : "#e2e8f0"
    border.width: 1

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: control.theme ? control.theme.sp12 : 12
        spacing: control.theme ? control.theme.sp8 : 8
    }
}
