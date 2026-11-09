import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    property string label: ""
    property alias from: spin.from
    property alias to: spin.to
    property alias stepSize: spin.stepSize
    property alias value: spin.value
    property var textFromValue: function(value) { return String(value) }
    property var valueFromText: function(text) { return Math.round(Number(text)) }

    signal valueModified(int value)

    function formatValue(value) {
        return root.textFromValue ? root.textFromValue(value) : String(value)
    }

    function parseText(text) {
        return root.valueFromText ? root.valueFromText(text) : Math.round(Number(text))
    }

    spacing: 4
    Layout.fillWidth: true

    Label {
        text: root.label
        visible: root.label.length > 0
        color: Kirigami.Theme.disabledTextColor
    }

    SpinBox {
        id: spin
        Layout.fillWidth: true
        editable: true
        textFromValue: root.formatValue
        valueFromText: root.parseText
        onValueModified: root.valueModified(value)
    }
}
