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

Text {
    id: control
    property var theme: null
    property string variant: "primary"

    color: !control.theme ? "#E2E2E5" :
           control.variant === "secondary" ? control.theme.textSecondary :
           control.variant === "muted" ? control.theme.textMuted :
           control.variant === "disabled" ? control.theme.textDisabled :
           control.variant === "onPrimary" ? control.theme.onPrimary :
           control.variant === "selected" ? control.theme.selectedText :
           control.theme.textPrimary
    font.pixelSize: control.theme ? control.theme.fontMd : 13
    wrapMode: Text.WordWrap
}
