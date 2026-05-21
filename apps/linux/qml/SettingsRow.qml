import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: control
    property var theme: null

    property string label: ""
    property string description: ""
    property bool isSwitch: true
    property bool checked: false
    property bool enabled: true

    property real sliderValue: 50
    property real sliderFrom: 0
    property real sliderTo: 100
    property real sliderStep: 1
    property string valueLabel: ""

    implicitHeight: isSwitch ? (description.length > 0 ? 56 : 44) : 48
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
            anchors.leftMargin: control.theme ? control.theme.sp4 : 4
            anchors.rightMargin: control.theme ? control.theme.sp4 : 4

            AppSwitch {
                id: switchCtrl
                checked: control.checked
                enabled: control.enabled
                theme: control.theme
                Layout.alignment: Qt.AlignTop
                Layout.topMargin: 4
                onCheckedChanged: control.checked = checked
            }

            ColumnLayout {
                spacing: 2
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignLeft
                Label {
                    text: control.label
                    font.pixelSize: control.theme ? control.theme.fontMd : 13
                    color: control.enabled ? (control.theme ? control.theme.text : "#e2e8f0") : (control.theme ? control.theme.textDim : "#94a3b8")
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                }
                Label {
                    text: control.description
                    font.pixelSize: control.theme ? control.theme.fontXs : 11
                    color: control.theme ? control.theme.textDim : "#94a3b8"
                    visible: control.description.length > 0
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }
            }
        }
    }

    Component {
        id: sliderComponent
        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: control.theme ? control.theme.sp4 : 4
            anchors.rightMargin: control.theme ? control.theme.sp4 : 4
            spacing: control.theme ? control.theme.sp4 : 4

            RowLayout {
                Layout.fillWidth: true
                spacing: control.theme ? control.theme.sp12 : 12

                Label {
                    text: control.label
                    font.pixelSize: control.theme ? control.theme.fontMd : 13
                    color: control.theme ? control.theme.text : "#e2e8f0"
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                }

                Label {
                    text: control.valueLabel
                    font.pixelSize: control.theme ? control.theme.fontSm : 12
                    color: control.theme ? control.theme.textDim : "#94a3b8"
                    Layout.preferredWidth: 60
                    horizontalAlignment: Text.AlignRight
                }
            }

            Slider {
                id: sliderCtrl
                from: control.sliderFrom
                to: control.sliderTo
                value: control.sliderValue
                stepSize: control.sliderStep
                Layout.fillWidth: true
                Layout.preferredHeight: 24
                Layout.bottomMargin: 4
                onValueChanged: control.sliderValue = value
            }
        }
    }
}
