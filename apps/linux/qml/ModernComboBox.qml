// =============================================================================
// ModernComboBox.qml — 现代下拉选择框组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：自定义样式的下拉选择框，支持主题适配
// 约束：
//   - 纯 UI 组件，数据通过 model property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    property var dt: null
    property var model: []
    property var displayModel: null
    property int currentIndex: 0
    readonly property string currentText: {
        if (!model || model.length === 0 || currentIndex < 0 || currentIndex >= model.length) return ""
        if (displayModel && displayModel.length > currentIndex) return String(displayModel[currentIndex])
        return String(model[currentIndex])
    }
    signal activated(int index)

    implicitWidth: 180
    implicitHeight: Math.max(dt ? dt.settingsControlHeight : 36, 40)
    clip: false

    Rectangle {
        anchors.fill: parent
        radius: dt ? dt.radiusMd : 12
        color: dt ? dt.inputBg : "#F1F5F9"
        border.width: 1
        border.color: dt ? dt.controlBorder : "#3A3F49"

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt ? dt.sp12 : 12
            anchors.rightMargin: dt ? dt.sp12 : 12
            spacing: dt ? dt.sp8 : 8

            AppText {
                Layout.fillWidth: true
                text: root.currentText
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.label : 13
                font.family: dt ? dt.fontFamily : "sans-serif"
                elide: Text.ElideRight
                verticalAlignment: Text.AlignVCenter
            }
            AppText {
                text: "v"
                color: dt ? dt.textMuted : "#606470"
                font.pixelSize: dt ? dt.fontXs : 11
                Layout.preferredWidth: 16
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
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
            radius: dt ? dt.radiusLg : 16
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
                    radius: dt ? dt.radiusMd : 12
                    color: itemHover.containsMouse || index === root.currentIndex
                           ? (dt ? dt.primaryContainer : "#CCE5FF")
                           : "transparent"

                    AppText {
                        Layout.alignment: Qt.AlignVCenter
                        anchors.left: parent.left
                        anchors.leftMargin: dt ? dt.sp10 : 10
                        text: (root.displayModel && root.displayModel.length > index) ? String(root.displayModel[index]) : String(modelData)
                        color: index === root.currentIndex ? (dt ? dt.onPrimaryContainer : "#001E31") : (dt ? dt.textPrimary : "#E2E4E9")
                        font.pixelSize: dt ? dt.label : 13
                        font.family: dt ? dt.fontFamily : "sans-serif"
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
