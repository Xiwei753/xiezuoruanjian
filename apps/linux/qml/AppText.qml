// =============================================================================
// AppText.qml — 通用文本组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一风格的文本组件，支持 primary/secondary 等变体
// 约束：
//   - 纯 UI 组件，文本通过 text property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Label {
    id: control
    property var theme: null
    property string variant: "primary"

    color: {
        if (!control.theme) return control.palette.text
        if (control.variant === "secondary") return control.theme.textSecondary
        if (control.variant === "disabled") return control.theme.textDisabled
        return control.theme.textPrimary
    }
    font.pixelSize: control.theme ? control.theme.fontMd : 13
    wrapMode: Text.WordWrap
}
