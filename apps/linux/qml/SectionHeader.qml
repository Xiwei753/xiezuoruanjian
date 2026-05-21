import QtQuick 2.15
import QtQuick.Controls 2.15

Label {
    id: control
    property var theme: null

    font.pixelSize: control.theme ? control.theme.fontXl : 18
    font.weight: Font.Bold
    color: control.theme ? control.theme.text : "#e2e8f0"
}
