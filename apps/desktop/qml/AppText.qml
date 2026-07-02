import QtQuick
import QtQuick.Controls

Text {
    id: control
    required property var dt
    property string variant: "primary"

    color: {
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
    font.pixelSize: dt.fontMd
    wrapMode: Text.WordWrap
}
