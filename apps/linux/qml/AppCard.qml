import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: control
    property var theme: null
    property alias spacing: col.spacing

    implicitWidth: 200
    implicitHeight: col.implicitHeight + (control.theme ? control.theme.sp24 : 24)
    radius: control.theme ? control.theme.radiusMd : 8
    color: control.theme ? control.theme.surface : "#ffffff"
    border.color: control.theme ? control.theme.border : "#e2e8f0"
    border.width: 1

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.leftMargin: control.theme ? control.theme.sp12 : 12
        anchors.rightMargin: control.theme ? control.theme.sp12 : 12
        anchors.topMargin: control.theme ? control.theme.sp12 : 12
        anchors.bottomMargin: control.theme ? control.theme.sp12 : 12
        spacing: control.theme ? control.theme.sp8 : 8
    }
}
