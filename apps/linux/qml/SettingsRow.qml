import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: control
    property var theme: null

    // Switch mode
    property string label: ""
    property string description: ""
    property bool isSwitch: true
    property bool checked: false

    // Slider mode
    property real sliderValue: 50
    property real sliderFrom: 0
    property real sliderTo: 100
    property real sliderStep: 1
    property string valueLabel: ""

    implicitHeight: isSwitch ? 44 : 40
    implicitWidth: 200

    Loader {
        anchors.fill: parent
        sourceComponent: control.isSwitch ? switchComponent : sliderComponent
    }

    Component {
        id: switchComponent
        RowLayout {
            spacing: control.theme ? control.theme.sp12 : 12
            anchors.fill: parent

            Switch {
                id: switchCtrl
                checked: control.checked
                onCheckedChanged: control.checked = checked
                indicator: Rectangle {
                    implicitWidth: 40; implicitHeight: 22
                    x: parent.leftPadding; y: parent.height / 2 - height / 2
                    radius: 11
                    color: parent.checked ? (control.theme ? control.theme.primary : "#0ea5e9") : (control.theme ? control.theme.border : "#334155")
                    Behavior on color { ColorAnimation { duration: 150 } }
                    Rectangle {
                        x: parent.checked ? 20 : 2; y: 2
                        width: 18; height: 18; radius: 9
                        color: "#ffffff"
                        Behavior on x { NumberAnimation { duration: 150 } }
                    }
                }
                background: Item {}
            }

            ColumnLayout {
                spacing: 2
                Label {
                    text: control.label
                    font.pixelSize: control.theme ? control.theme.fontMd : 13
                    color: control.theme ? control.theme.text : "#e2e8f0"
                }
                Label {
                    text: control.description
                    font.pixelSize: control.theme ? control.theme.fontXs : 11
                    color: control.theme ? control.theme.textDim : "#94a3b8"
                    visible: control.description.length > 0
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }
        }
    }

    Component {
        id: sliderComponent
        RowLayout {
            anchors.fill: parent
            spacing: control.theme ? control.theme.sp12 : 12

            Label {
                text: control.label
                font.pixelSize: control.theme ? control.theme.fontMd : 13
                color: control.theme ? control.theme.text : "#e2e8f0"
                Layout.preferredWidth: Math.max(60, control.label.length * 10)
            }

            Slider {
                id: sliderCtrl
                from: control.sliderFrom
                to: control.sliderTo
                value: control.sliderValue
                stepSize: control.sliderStep
                Layout.fillWidth: true
                Layout.preferredHeight: 24
                onValueChanged: control.sliderValue = value
            }

            Label {
                text: control.valueLabel
                font.pixelSize: control.theme ? control.theme.fontSm : 12
                color: control.theme ? control.theme.textDim : "#94a3b8"
                Layout.preferredWidth: 56
                horizontalAlignment: Text.AlignRight
            }
        }
    }
}
