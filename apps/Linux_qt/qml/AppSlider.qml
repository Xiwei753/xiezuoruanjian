// =============================================================================
// AppSlider.qml — 通用滑块组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
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
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt

    readonly property color _primary: resolvedDt.primary
    readonly property color _border: resolvedDt.border
    readonly property color _surfaceVariant: resolvedDt.surfaceVariant
    readonly property color _surface: resolvedDt.surface
    readonly property color _accentHover: resolvedDt.accentHover
    readonly property color _textPrimary: resolvedDt.textPrimary
    readonly property color _textSecondary: resolvedDt.textSecondary
    readonly property int _sp6: resolvedDt.sp6
    readonly property int _sp12: resolvedDt.sp12
    readonly property int _body: resolvedDt.body
    readonly property int _caption: resolvedDt.caption
    readonly property string _fontFamily: resolvedDt.fontFamily

    property string label: ""
    property string valueText: ""
    property string description: ""
    property alias from: slider.from
    property alias to: slider.to
    property alias value: slider.value
    property alias stepSize: slider.stepSize
    property alias pressed: slider.pressed
    signal moved()
    signal committed()

    implicitWidth: 240
    implicitHeight: content.implicitHeight

    ColumnLayout {
        id: content
        anchors.fill: parent
        spacing: _sp6

        RowLayout {
            Layout.fillWidth: true
            visible: root.label.length > 0 || root.valueText.length > 0
            spacing: _sp12

            AppText {
                dt: root.resolvedDt
                Layout.fillWidth: true
                text: root.label
                color: _textPrimary
                font.pixelSize: _body
                font.family: _fontFamily
                wrapMode: Text.Wrap
            }

            AppText {
                dt: root.resolvedDt
                text: root.valueText
                color: _textSecondary
                font.pixelSize: _caption
                font.family: _fontFamily
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

            onMoved: { root.moved(); root.committed() }

            onPressedChanged: {
                if (!slider.pressed) {
                    root.committed()
                }
            }

            background: Rectangle {
                x: slider.leftPadding
                y: slider.topPadding + slider.availableHeight / 2 - height / 2
                width: slider.availableWidth
                height: 6
                radius: 3
                color: _surfaceVariant

                Rectangle {
                    width: slider.visualPosition * parent.width
                    height: parent.height
                    color: slider.enabled ? _primary : _border
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
                    ? _accentHover
                    : (slider.enabled ? _primary : _border)
                border.color: _surface
                border.width: 3
            }
        }

        AppText {
            dt: root.resolvedDt
            Layout.fillWidth: true
            text: root.description
            color: _textSecondary
            font.pixelSize: _caption
            font.family: _fontFamily
            wrapMode: Text.Wrap
            visible: root.description.length > 0
        }
    }
}
