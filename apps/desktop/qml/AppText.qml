// =============================================================================
// AppText.qml — 通用文本组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
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

    color: {
        switch (control.variant) {
            case "secondary": return control.theme.textSecondary;
            case "muted": return control.theme.textMuted;
            case "disabled": return control.theme.textDisabled;
            case "onPrimary": return control.theme.onPrimary;
            case "selected": return control.theme.selectedText;
            case "onSurface": return control.theme.onSurface;
            case "onSurfaceVariant": return control.theme.onSurfaceVariant;
            case "onPrimaryContainer": return control.theme.onPrimaryContainer;
            case "onSecondaryContainer": return control.theme.onSecondaryContainer;
            case "onError": return control.theme.onError;
            case "onDangerContainer": return control.theme.onDangerContainer;
            case "primary":
            default:
                return control.theme.textPrimary;
        }
    }
    font.pixelSize: control.theme.fontMd
    wrapMode: Text.WordWrap
}
