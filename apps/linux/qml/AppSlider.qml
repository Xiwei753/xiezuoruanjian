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

Slider {
    id: control
    property var theme: null

    implicitWidth: 200
    implicitHeight: 32

    background: Rectangle {
        x: control.leftPadding
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: control.availableWidth
        height: 6
        radius: 3
        color: control.theme ? control.theme.surfaceVariant : "#DFE3EB"

        Rectangle {
            width: control.visualPosition * parent.width
            height: parent.height
            color: control.enabled
                ? (control.theme ? control.theme.primary : "#006497")
                : (control.theme ? control.theme.border : "#e2e8f0")
            radius: 3
        }
    }

    handle: Rectangle {
        x: control.leftPadding + control.visualPosition * (control.availableWidth - width)
        y: control.topPadding + control.availableHeight / 2 - height / 2
        width: control.theme ? 20 : 20
        height: width
        radius: width / 2
        color: control.pressed
            ? (control.theme ? control.theme.primaryHover : "#006497")
            : (control.enabled
                ? (control.theme ? control.theme.primary : "#006497")
                : (control.theme ? control.theme.border : "#e2e8f0"))
        border.color: control.theme ? control.theme.surface : "#ffffff"
        border.width: 3
    }
}
