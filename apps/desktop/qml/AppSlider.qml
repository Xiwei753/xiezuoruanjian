// =============================================================================
// AppSlider.qml — 通用滑块组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
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
    property var dt: null
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
        spacing: dt ? dt.sp6 : 6

        RowLayout {
            Layout.fillWidth: true
            visible: root.label.length > 0 || root.valueText.length > 0
            spacing: dt ? dt.sp12 : 12

            AppText {
                Layout.fillWidth: true
                text: root.label
                color: dt.textPrimary
                font.pixelSize: dt ? dt.body : 14
                font.family: dt ? dt.fontFamily : "sans-serif"
                wrapMode: Text.Wrap
            }

            AppText {
                text: root.valueText
                color: dt.textSecondary
                font.pixelSize: dt ? dt.caption : 12
                font.family: dt ? dt.fontFamily : "sans-serif"
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
                color: dt.surfaceVariant

                Rectangle {
                    width: slider.visualPosition * parent.width
                    height: parent.height
                    color: slider.enabled ? dt.primary : dt.border
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
                    ? dt.accentHover
                    : (slider.enabled ? dt.primary : dt.border)
                border.color: dt.surface
                border.width: 3
            }
        }

        AppText {
            Layout.fillWidth: true
            text: root.description
            color: dt.textSecondary
            font.pixelSize: dt ? dt.caption : 12
            font.family: dt ? dt.fontFamily : "sans-serif"
            wrapMode: Text.Wrap
            visible: root.description.length > 0
        }
    }
}
