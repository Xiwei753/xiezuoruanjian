import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: control
    property var theme: null
    property alias spacing: col.spacing

    implicitWidth: 200

    Rectangle {
        anchors.fill: parent
        radius: theme ? theme.radiusMd : 8
        color: theme ? theme.surface : "#ffffff"
        border.color: theme ? theme.border : "#e2e8f0"
        border.width: 1
    }

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: theme ? theme.sp12 : 12
        spacing: theme ? theme.sp8 : 8
    }
}
