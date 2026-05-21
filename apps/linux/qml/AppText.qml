import QtQuick 2.15
import QtQuick.Controls 2.15

Label {
    id: control
    property var theme: null
    property string variant: "primary"

    color: {
        if (!control.theme) return "#0f172a"
        if (control.variant === "secondary") return control.theme.textSecondary
        if (control.variant === "disabled") return control.theme.textDisabled
        return control.theme.textPrimary
    }
    font.pixelSize: control.theme ? control.theme.fontMd : 13
    wrapMode: Text.WordWrap
}
