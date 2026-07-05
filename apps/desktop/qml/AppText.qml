import QtQuick
import QtQuick.Controls

Text {
    id: control
    property var dt: null
    property string variant: "primary"

    color: {
        if (!dt) return "#E2E2E5"
        switch (control.variant) {
            case "secondary": return dt.textSecondary;
            case "muted": return dt.textMuted;
            case "disabled": return dt.textDisabled;
            case "onPrimary": return dt.onPrimary;
            case "selected": return dt.selectedText;
            case "onSurface": return dt.onSurface;
            case "onSurfaceVariant": return dt.onSurfaceVariant;
            case "onPrimaryContainer": return dt.onPrimaryContainer;
            case "onSecondaryContainer": return dt.onSecondaryContainer;
            case "onError": return dt.onError;
            case "onDangerContainer": return dt.onDangerContainer;
            case "primary":
            default:
                return dt.textPrimary;
        }
    }
    font.pixelSize: dt ? dt.fontMd : 14
    wrapMode: Text.WordWrap
}
