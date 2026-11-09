import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root

    property string text: ""
    property string description: ""
    property var model
    property alias currentIndex: combo.currentIndex
    property alias currentText: combo.currentText

    signal activated(int index)

    spacing: 4
    Layout.fillWidth: true

    Label {
        text: root.text
        visible: root.text.length > 0
        color: Kirigami.Theme.disabledTextColor
    }
    Label {
        Layout.fillWidth: true
        visible: root.description.length > 0
        text: root.description
        color: Kirigami.Theme.disabledTextColor
        font: Kirigami.Theme.smallFont
        wrapMode: Text.WordWrap
    }

    ComboBox {
        id: combo
        Layout.fillWidth: true
        model: root.model
        onActivated: root.activated(index)
    }
}
