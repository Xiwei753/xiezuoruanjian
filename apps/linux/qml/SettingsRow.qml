import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: control
    property var theme: null

    property string mode: "switch"
    property string label: ""
    property string description: ""
    property bool isSwitch: mode === "switch"
    property bool checked: false
    property bool enabled: true

    property real sliderValue: 50
    property real sliderFrom: 0
    property real sliderTo: 100
    property real sliderStep: 1
    property string valueLabel: ""

    implicitWidth: 200
    implicitHeight: {
        if (mode === "switch") return 56
        if (mode === "slider") return 72
        return 40
    }

    RowLayout {
        id: switchLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: control.theme ? control.theme.sp4 : 4
        anchors.rightMargin: control.theme ? control.theme.sp4 : 4
        spacing: control.theme ? control.theme.sp12 : 12
        visible: control.mode === "switch"

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
                color: control.enabled
                    ? (control.theme ? control.theme.textPrimary : "#0f172a")
                    : (control.theme ? control.theme.textDisabled : "#94a3b8")
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }
            Label {
                text: control.description
                font.pixelSize: control.theme ? control.theme.fontSm : 12
                color: control.enabled
                    ? (control.theme ? control.theme.textSecondary : "#475569")
                    : (control.theme ? control.theme.textDisabled : "#94a3b8")
                visible: control.description.length > 0
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }
        }
    }

    ColumnLayout {
        id: sliderLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: control.theme ? control.theme.sp4 : 4
        anchors.rightMargin: control.theme ? control.theme.sp4 : 4
        spacing: control.theme ? control.theme.sp4 : 4
        visible: control.mode === "slider"

        RowLayout {
            Layout.fillWidth: true
            spacing: control.theme ? control.theme.sp12 : 12

            Label {
                text: control.label
                font.pixelSize: control.theme ? control.theme.fontMd : 13
                color: control.theme ? control.theme.textPrimary : "#0f172a"
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }

            Label {
                text: control.valueLabel
                font.pixelSize: control.theme ? control.theme.fontSm : 12
                color: control.theme ? control.theme.textSecondary : "#475569"
                Layout.preferredWidth: 60
                horizontalAlignment: Text.AlignRight
            }
        }

        AppSlider {
            id: sliderCtrl
            from: control.sliderFrom
            to: control.sliderTo
            value: control.sliderValue
            stepSize: control.sliderStep
            theme: control.theme
            Layout.fillWidth: true
            onValueChanged: control.sliderValue = value
        }
    }
}
