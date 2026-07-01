// =============================================================================
// AppText.qml — 通用文本组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：统一风格的文本组件，支持 primary/secondary 等变体
// 约束：
//   - 纯 UI 组件，文本通过 text property 传入
//   - 使用 DesignTokens 统一样式
//   - 支持 dt 和 theme 双入口，优先 dt，其次 theme
// =============================================================================

import QtQuick
import QtQuick.Controls

Text {
    id: control
    property var dt: null
    property var theme: null
    property string variant: "primary"

    // Unified token entry: prefer dt, then theme
    readonly property var tokens: dt || theme

    onTokensChanged: {
        if (!tokens) console.warn("AppText: dt and theme are both null — must pass dt or theme to AppText. variant=" + control.variant)
    }

    color: {
        if (tokens) {
            switch (control.variant) {
                case "secondary": return tokens.textSecondary;
                case "muted": return tokens.textMuted;
                case "disabled": return tokens.textDisabled;
                case "onPrimary": return tokens.onPrimary;
                case "selected": return tokens.selectedText;
                case "onSurface": return tokens.onSurface;
                case "onSurfaceVariant": return tokens.onSurfaceVariant;
                case "onPrimaryContainer": return tokens.onPrimaryContainer;
                case "onSecondaryContainer": return tokens.onSecondaryContainer;
                case "onError": return tokens.onError;
                case "onDangerContainer": return tokens.onDangerContainer;
                case "primary":
                default:
                    return tokens.textPrimary;
            }
        }
        // No fallback: dt/theme must be provided.
        // If you see this color (magenta), pass dt or theme to AppText.
        return "magenta"
    }
    font.pixelSize: tokens ? tokens.fontMd : 14
    wrapMode: Text.WordWrap
}
