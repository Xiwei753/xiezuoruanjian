import QtQuick 2.15

Rectangle {
    id: control
    property var theme: null
    property string pillColor: control.theme ? control.theme.success : "#22c55e"

    width: 8
    height: 8
    radius: 4
    color: pillColor
}
