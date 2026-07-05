import QtQuick
import QtQuick.Controls

Text {
    id: control
    property var dt: null
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt
    property string variant: "primary"

    color: {
        switch (control.variant) {
            case "secondary": return resolvedDt.textSecondary;
            case "muted": return resolvedDt.textMuted;
            case "disabled": return resolvedDt.textDisabled;
            case "onPrimary": return resolvedDt.onPrimary;
            case "selected": return resolvedDt.selectedText;
            case "onSurface": return resolvedDt.onSurface;
            case "onSurfaceVariant": return resolvedDt.onSurfaceVariant;
            case "onPrimaryContainer": return resolvedDt.onPrimaryContainer;
            case "onSecondaryContainer": return resolvedDt.onSecondaryContainer;
            case "onError": return resolvedDt.onError;
            case "onDangerContainer": return resolvedDt.onDangerContainer;
            case "primary":
            default:
                return resolvedDt.textPrimary;
        }
    }
    font.pixelSize: resolvedDt.fontMd
    wrapMode: Text.WordWrap
}
