import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: control
    property var theme: null
    property string label: ""
    property string placeholder: ""
    property alias text: inputField.text
    property alias echoMode: inputField.echoMode
    property alias validator: inputField.validator
    property bool fieldTabFocus: true

    implicitHeight: inputField.height + 22
    implicitWidth: 200

    ColumnLayout {
        anchors.fill: parent
        spacing: control.theme ? control.theme.sp4 : 4

        Label {
            text: control.label
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            color: control.theme ? control.theme.text : "#e2e8f0"
            font.weight: Font.Medium
            visible: control.label.length > 0
        }

        TextField {
            id: inputField
            Layout.fillWidth: true
            implicitHeight: 32
            placeholderText: control.placeholder
            color: control.theme ? control.theme.text : "#e2e8f0"
            echoMode: control.echoMode
            validator: control.validator
            activeFocusOnTab: control.fieldTabFocus
            background: Rectangle {
                color: control.theme ? control.theme.surface : "#1a1a2e"
                border.color: inputField.activeFocus ? (control.theme ? control.theme.borderFocus : "#0ea5e9") : (control.theme ? control.theme.border : "#334155")
                border.width: 1
                radius: control.theme ? control.theme.radiusSm : 4
            }
            font.pixelSize: control.theme ? control.theme.fontMd : 13
            leftPadding: control.theme ? control.theme.sp8 : 8
            topPadding: control.theme ? control.theme.sp6 : 6
            bottomPadding: control.theme ? control.theme.sp6 : 6
        }
    }
}
