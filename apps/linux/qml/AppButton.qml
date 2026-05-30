// =============================================================================
// AppButton.qml — 通用按钮组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一风格的按钮组件，支持 primary/secondary/text/danger 变体
// 约束：
//   - 纯 UI 组件，点击事件通过 onClicked 处理
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Button {
    id: control
    property var theme: null
    property string tooltip: ""
    property bool small: false
    property bool primary: false
    property string variant: primary ? "primary" : "secondary"

    readonly property color containerColor: {
        if (!control.enabled) return control.theme ? control.theme.surfaceContainer : "#f1f5f9"
        if (control.variant === "primary") return control.theme ? control.theme.primary : "#006497"
        if (control.variant === "danger") return control.theme ? control.theme.error : "#BA1A1A"
        if (control.variant === "text") return "transparent"
        return control.theme ? control.theme.secondaryContainer : "#D4E4F6"
    }
    readonly property color contentColor: {
        if (!control.enabled) return control.theme ? control.theme.textDisabled : "#94a3b8"
        if (control.variant === "primary") return control.theme ? control.theme.onPrimary : "#ffffff"
        if (control.variant === "danger") return control.theme ? control.theme.onError : "#ffffff"
        if (control.variant === "text") return control.theme ? control.theme.primary : "#006497"
        return control.theme ? control.theme.onSecondaryContainer : "#0E1D2A"
    }

    implicitHeight: small ? 32 : 40
    implicitWidth: Math.max(tm.width + (small ? 20 : 28), small ? 56 : 72)

    TextMetrics { id: tm; text: control.text; font.pixelSize: control.theme ? control.theme.label : 13 }

    contentItem: Text {
        text: control.text
        color: control.contentColor
        font.pixelSize: control.theme ? control.theme.label : 13
        font.weight: Font.Medium
        font.family: control.theme ? control.theme.fontFamily : "sans-serif"
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: {
            if (!control.enabled) return control.containerColor
            if (control.pressed && control.variant !== "text") return control.theme ? (control.theme.isDark ? Qt.lighter(control.containerColor, 1.08) : Qt.darker(control.containerColor, 1.08)) : control.containerColor
            if (control.hovered && control.variant === "text") return control.theme ? control.theme.surfaceContainer : "#f1f5f9"
            if (control.hovered && control.variant !== "text") return control.theme ? (control.theme.isDark ? Qt.lighter(control.containerColor, 1.05) : Qt.darker(control.containerColor, 1.03)) : control.containerColor
            return control.containerColor
        }
        border.color: control.variant === "secondary" ? (control.theme ? control.theme.border : "#e2e8f0") : "transparent"
        border.width: control.variant === "secondary" ? 1 : 0
        radius: control.theme ? control.theme.radiusMd : 12

        Behavior on color { ColorAnimation { duration: control.theme ? control.theme.animFast : 120 } }
    }
}
