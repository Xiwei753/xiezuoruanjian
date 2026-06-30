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

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    color: {
        if (!control.theme) {
            // 从 SystemPalette 推断深浅色，不再固定走 light fallback
            switch (control.variant) {
                case "secondary": return _inferDark ? "#C3C6CF" : "#42474E";
                case "muted": return _inferDark ? "#8C9198" : "#74777F";
                case "disabled": return _inferDark ? "#5A5E66" : "#1A1C1E61";
                case "onPrimary": return _inferDark ? "#003351" : "#FFFFFF";
                case "selected": return _inferDark ? "#CCE5FF" : "#001E31";
                case "onSurface": return _inferDark ? "#E2E2E5" : "#1A1C1E";
                case "onSurfaceVariant": return _inferDark ? "#C3C6CF" : "#42474E";
                case "onPrimaryContainer": return _inferDark ? "#CCE5FF" : "#001E31";
                case "onSecondaryContainer": return _inferDark ? "#D4E3F7" : "#0E1D2A";
                case "onError": return _inferDark ? "#690005" : "#FFFFFF";
                case "onDangerContainer": return _inferDark ? "#FFDAD6" : "#410002";
                case "primary":
                default:
                    return _inferDark ? "#E2E2E5" : "#1A1C1E";
            }
        }
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
    font.pixelSize: control.theme ? control.theme.fontMd : 13
    wrapMode: Text.WordWrap
}
