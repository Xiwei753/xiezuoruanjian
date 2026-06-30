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

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    // Safe access: fallback 根据 SystemPalette 推断深浅色，不再固定走 light
    readonly property color _primary: dt ? dt.primary : (_inferDark ? "#92CCFF" : "#006497")
    readonly property color _border: dt ? dt.border : (_inferDark ? "#8C919842" : "#71788057")
    readonly property color _surfaceVariant: dt ? dt.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")
    readonly property color _surface: dt ? dt.surface : (_inferDark ? "#1A1D23" : "#FCFCFF")
    readonly property color _accentHover: dt ? dt.accentHover : (_inferDark ? "#BFE0FF" : "#005079")
    readonly property color _textPrimary: dt ? dt.textPrimary : (_inferDark ? "#E2E2E5" : "#1A1C1E")
    readonly property color _textSecondary: dt ? dt.textSecondary : (_inferDark ? "#C3C6CF" : "#42474E")
    readonly property int _sp6: dt ? dt.sp6 : 6
    readonly property int _sp12: dt ? dt.sp12 : 12
    readonly property int _body: dt ? dt.body : 14
    readonly property int _caption: dt ? dt.caption : 12
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"

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
        spacing: _sp6

        RowLayout {
            Layout.fillWidth: true
            visible: root.label.length > 0 || root.valueText.length > 0
            spacing: _sp12

            AppText {
                Layout.fillWidth: true
                text: root.label
                color: _textPrimary
                font.pixelSize: _body
                font.family: _fontFamily
                wrapMode: Text.Wrap
            }

            AppText {
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

            onMoved: root.moved()

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
