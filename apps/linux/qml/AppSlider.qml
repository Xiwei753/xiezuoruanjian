import QtQuick 2.15
import QtQuick.Controls 2.15

Slider {
    id: control
    property var theme: null

    Layout.fillWidth: true
    Layout.preferredHeight: 28

    background: Rectangle {
        x: control.leftPadding
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: control.availableWidth
        height: control.theme ? 4 : 4
        radius: 2
        color: control.theme ? control.theme.surfaceAlt : "#f1f5f9"

        Rectangle {
            width: control.visualPosition * parent.width
            height: parent.height
            color: control.enabled
                ? (control.theme ? control.theme.primary : "#3b82f6")
                : (control.theme ? control.theme.border : "#e2e8f0")
            radius: 2
        }
    }

    handle: Rectangle {
        x: control.leftPadding + control.visualPosition * (control.availableWidth - width)
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: control.theme ? 18 : 18
        height: width
        radius: width / 2
        color: control.pressed
            ? (control.theme ? control.theme.primaryHover : "#60a5fa")
            : (control.enabled
                ? (control.theme ? control.theme.primary : "#3b82f6")
                : (control.theme ? control.theme.border : "#e2e8f0"))
        border.color: control.theme ? control.theme.surface : "#ffffff"
        border.width: 2
    }
}
