import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    property string label: ""
    property alias text: field.text
    property alias placeholderText: field.placeholderText
    property alias echoMode: field.echoMode
    property alias readOnly: field.readOnly
    property alias inputMethodHints: field.inputMethodHints

    spacing: 4
    Layout.fillWidth: true

    Label {
        text: root.label
        visible: root.label.length > 0
        color: Kirigami.Theme.disabledTextColor
    }

    TextField {
        id: field
        Layout.fillWidth: true
    }
}
