// =============================================================================
// ModernComboBox.qml — 现代下拉选择框组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
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
    implicitHeight: Math.max(dt.settingsControlHeight, 40)
    clip: false

    Rectangle {
        anchors.fill: parent
        radius: dt.radiusMd
        color: dt.inputBg
        border.width: 1
        border.color: dt.controlBorder

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt.sp12
            anchors.rightMargin: dt.sp12
            spacing: dt.sp8

            AppText {
                dt: root.dt
                Layout.fillWidth: true
                text: root.currentText
                color: dt.textPrimary
                font.pixelSize: dt.label
                font.family: dt.fontFamily
                elide: Text.ElideRight
                verticalAlignment: Text.AlignVCenter
            }
            AppText {
                dt: root.dt
                text: "v"
                color: dt.textMuted
                font.pixelSize: dt.fontXs
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
        y: root.height + dt.sp6
        width: Math.max(root.width, 180)
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent
        z: 2000

        background: Rectangle {
            radius: dt.radiusLg
            color: dt.surface
            border.width: 1
            border.color: dt.border
        }

        contentItem: ColumnLayout {
            spacing: dt.sp4

            Repeater {
                model: root.model
                delegate: Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: dt.settingsControlHeight
                    radius: dt.radiusMd
                    color: itemHover.containsMouse || index === root.currentIndex
                           ? dt.primaryContainer
                           : "transparent"

                    AppText {
                        dt: root.dt
                        anchors {
                            left: parent.left
                            leftMargin: dt.sp10
                        }
                        anchors.verticalCenter: parent.verticalCenter
                        text: (root.displayModel && root.displayModel.length > index) ? String(root.displayModel[index]) : String(modelData)
                        color: index === root.currentIndex ? dt.onPrimaryContainer : dt.textPrimary
                        font.pixelSize: dt.label
                        font.family: dt.fontFamily
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
