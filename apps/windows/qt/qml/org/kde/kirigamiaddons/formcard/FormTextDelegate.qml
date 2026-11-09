import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: root
    property string text: ""
    property string description: ""
    spacing: 2
    Layout.fillWidth: true

    Label {
        Layout.fillWidth: true
        text: root.text
        color: Kirigami.Theme.textColor
        wrapMode: Text.WordWrap
    }
    Label {
        Layout.fillWidth: true
        visible: root.description.length > 0
        text: root.description
        color: Kirigami.Theme.disabledTextColor
        font: Kirigami.Theme.smallFont
        wrapMode: Text.WordWrap
    }
}
