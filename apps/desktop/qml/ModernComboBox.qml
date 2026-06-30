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

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    implicitWidth: 180
    implicitHeight: Math.max(dt ? dt.settingsControlHeight : 36, 40)
    clip: false

    Rectangle {
        anchors.fill: parent
        radius: dt ? dt.radiusMd : 12
        color: dt ? dt.inputBg : (_inferDark ? "#1F2229" : "#F1F5F9")
        border.width: 1
        border.color: dt ? dt.controlBorder : (_inferDark ? "#8C9198" : "#3A3F49")

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: dt ? dt.sp12 : 12
            anchors.rightMargin: dt ? dt.sp12 : 12
            spacing: dt ? dt.sp8 : 8

            AppText {
                Layout.fillWidth: true
                text: root.currentText
                color: dt ? dt.textPrimary : (_inferDark ? "#E2E2E5" : "#1A1C1E")
                font.pixelSize: dt ? dt.label : 13
                font.family: dt ? dt.fontFamily : "sans-serif"
                elide: Text.ElideRight
                verticalAlignment: Text.AlignVCenter
            }
            AppText {
                text: "v"
                color: dt ? dt.textMuted : (_inferDark ? "#8C9198" : "#606470")
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
            color: dt ? dt.surface : (_inferDark ? "#1A1D23" : "#FCFCFF")
            border.width: 1
            border.color: dt ? dt.border : (_inferDark ? "#2A2E36" : "#CBD5E1")
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
                           ? (dt ? dt.primaryContainer : (_inferDark ? "#004A77" : "#CCE5FF"))
                           : "transparent"

                    AppText {
                        anchors {
                            left: parent.left
                            leftMargin: dt ? dt.sp10 : 10
                        }
                        anchors.verticalCenter: parent.verticalCenter
                        text: (root.displayModel && root.displayModel.length > index) ? String(root.displayModel[index]) : String(modelData)
                        color: index === root.currentIndex ? (dt ? dt.onPrimaryContainer : (_inferDark ? "#CCE5FF" : "#001E31")) : (dt ? dt.textPrimary : (_inferDark ? "#E2E2E5" : "#1A1C1E"))
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
