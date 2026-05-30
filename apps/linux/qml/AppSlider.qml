// =============================================================================
// AppSlider.qml — 通用滑块组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一风格的滑块组件，支持主题适配
// 约束：
//   - 纯 UI 组件，值通过 value property 绑定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    property var theme: null
    property string label: ""
    property string valueText: ""
    property string description: ""
    property alias from: slider.from
    property alias to: slider.to
    property alias value: slider.value
    property alias stepSize: slider.stepSize
    property alias pressed: slider.pressed
    signal moved()

    implicitWidth: 240
    implicitHeight: content.implicitHeight

    ColumnLayout {
        id: content
        anchors.fill: parent
        spacing: root.theme ? root.theme.sp6 : 6

        RowLayout {
            Layout.fillWidth: true
            visible: root.label.length > 0 || root.valueText.length > 0
            spacing: root.theme ? root.theme.sp12 : 12

            Text {
                Layout.fillWidth: true
                text: root.label
                color: root.theme ? root.theme.textPrimary : "#E2E4E9"
                font.pixelSize: root.theme ? root.theme.body : 14
                font.family: root.theme ? root.theme.fontFamily : "sans-serif"
                wrapMode: Text.Wrap
            }

            Text {
                text: root.valueText
                color: root.theme ? root.theme.textSecondary : "#9CA0AB"
                font.pixelSize: root.theme ? root.theme.caption : 12
                font.family: root.theme ? root.theme.fontFamily : "sans-serif"
                horizontalAlignment: Text.AlignRight
                visible: root.valueText.length > 0
            }
        }

        Slider {
            id: slider
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            leftPadding: 10
            rightPadding: 10

            onMoved: root.moved()

            background: Rectangle {
                x: slider.leftPadding
                y: slider.topPadding + slider.availableHeight / 2 - height / 2
                width: slider.availableWidth
                height: 6
                radius: 3
                color: root.theme ? root.theme.surfaceVariant : "#DFE3EB"

                Rectangle {
                    width: slider.visualPosition * parent.width
                    height: parent.height
                    color: slider.enabled
                        ? (root.theme ? root.theme.primary : "#006497")
                        : (root.theme ? root.theme.border : "#e2e8f0")
                    radius: 3
                }
            }

            handle: Rectangle {
                x: slider.leftPadding + slider.visualPosition * (slider.availableWidth - width)
                y: slider.topPadding + slider.availableHeight / 2 - height / 2
                width: 20
                height: width
                radius: width / 2
                color: slider.pressed
                    ? (root.theme ? root.theme.primaryHover : "#006497")
                    : (slider.enabled
                        ? (root.theme ? root.theme.primary : "#006497")
                        : (root.theme ? root.theme.border : "#e2e8f0"))
                border.color: root.theme ? root.theme.surface : "#ffffff"
                border.width: 3
            }
        }

        Text {
            Layout.fillWidth: true
            text: root.description
            color: root.theme ? root.theme.textSecondary : "#9CA0AB"
            font.pixelSize: root.theme ? root.theme.caption : 12
            font.family: root.theme ? root.theme.fontFamily : "sans-serif"
            wrapMode: Text.Wrap
            visible: root.description.length > 0
        }
    }
}
