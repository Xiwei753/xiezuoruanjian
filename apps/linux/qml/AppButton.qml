// =============================================================================
// AppButton.qml — 通用按钮组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一风格的按钮组件，支持 primary/small/tooltip 变体
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

    implicitHeight: small ? 28 : 32
    implicitWidth: Math.max(tm.width + (small ? 16 : 24), small ? 48 : 64)

    TextMetrics { id: tm; text: control.text; font.pixelSize: control.theme ? control.theme.fontSm : 12 }

    contentItem: Text {
        text: control.text
        color: {
            if (!control.enabled) return control.theme ? control.theme.textDisabled : "#94a3b8"
            return control.theme ? control.theme.primaryText : "#ffffff"
        }
        font.pixelSize: control.theme ? control.theme.fontSm : 12
        font.weight: Font.Medium
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: {
            if (!control.enabled) return control.theme ? control.theme.surfaceAlt : "#f1f5f9"
            if (control.primary) return control.theme ? control.theme.primary : "#3b82f6"
            return control.hovered ? (control.theme ? control.theme.primaryHover : "#60a5fa") : (control.theme ? control.theme.surfaceVariant : "#242933")
        }
        border.color: !control.enabled ? (control.theme ? control.theme.border : "#e2e8f0") : "transparent"
        border.width: !control.enabled ? 1 : 0
        radius: control.theme ? control.theme.radiusSm : 6
    }
}
