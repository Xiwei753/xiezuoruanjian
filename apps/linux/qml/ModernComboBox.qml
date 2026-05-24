import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root
    property var dt: null
    property var model: []
    property int currentIndex: 0
    readonly property string currentText: {
        if (!model || model.length === 0 || currentIndex < 0 || currentIndex >= model.length) return ""
        return String(model[currentIndex])
    }
    signal activated(int index)

    implicitWidth: 160
    implicitHeight: dt ? dt.settingsControlHeight : 36

    Rectangle {
        anchors.fill: parent
        radius: dt ? dt.radiusSm : 8
        color: dt ? dt.surfaceVariant : "#242933"
        border.width: 1
        border.color: dt ? dt.controlBorder : "#3A3F49"

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt ? dt.sp10 : 10
            anchors.rightMargin: dt ? dt.sp10 : 10
            spacing: dt ? dt.sp8 : 8

            Text {
                Layout.fillWidth: true
                text: root.currentText
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontSm : 12
                elide: Text.ElideRight
            }
            Text {
                text: "v"
                color: dt ? dt.textMuted : "#606470"
                font.pixelSize: dt ? dt.fontXs : 11
            }
        }

        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: popup.open()
        }
    }

    Popup {
        id: popup
        y: root.height + (dt ? dt.sp6 : 6)
        width: Math.max(root.width, 180)
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent
        z: 2000

        background: Rectangle {
            radius: dt ? dt.radiusMd : 12
            color: dt ? dt.surface : "#1A1D23"
            border.width: 1
            border.color: dt ? dt.border : "#2A2E36"
        }

        contentItem: ColumnLayout {
            spacing: dt ? dt.sp4 : 4

            Repeater {
                model: root.model
                delegate: Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: dt ? dt.settingsControlHeight : 36
                    radius: dt ? dt.radiusSm : 8
                    color: itemHover.containsMouse || index === root.currentIndex
                           ? (dt ? dt.accentSoft : "rgba(123,140,222,0.12)")
                           : "transparent"

                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: dt ? dt.sp10 : 10
                        text: String(modelData)
                        color: index === root.currentIndex ? (dt ? dt.accentText : "#3D4D9E") : (dt ? dt.textPrimary : "#E2E4E9")
                        font.pixelSize: dt ? dt.fontSm : 12
                    }

                    MouseArea {
                        id: itemHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.currentIndex = index
                            popup.close()
                            root.activated(index)
                        }
                    }
                }
            }
        }
    }
}
