import QtQuick

Text {
    id: root

    property string source: ""
    property int sourceSize: 16

    // Keep the same KDE icon names used by the shared Linux QML pages.
    // Windows doesn't ship Breeze icons, so map them to available system glyphs.
    function glyphFor(name) {
        switch (name || "") {
        case "arrow-left": return "\u2190"
        case "arrow-right": return "\u2192"
        case "arrow-up": return "\u2191"
        case "arrow-down": return "\u2193"
        case "dialog-ok": return "\u2713"
        case "document-open": return "\uD83D\uDCC2"
        case "document-multiple": return "\u25A4"
        case "text-x-generic": return "\u2261"
        case "folder": return "\uD83D\uDCC1"
        case "folder-open": return "\uD83D\uDCC2"
        case "edit-find": return "\u2315"
        case "list-add": return "+"
        case "starred": return "\u2605"
        case "view-refresh": return "\u21BB"
        case "view-grid": return "\u25A6"
        case "view-list": return "\u2637"
        default: return name ? "\u25A1" : ""
        }
    }

    text: glyphFor(source)
    // The shared QML controls specify icon dimensions through width or
    // implicitWidth, not through a Kirigami-specific sourceSize property.
    // The implicit geometry must not depend on contentWidth/Height here,
    // otherwise font-size bindings would loop as the icon grows.
    implicitWidth: sourceSize
    implicitHeight: sourceSize
    font.pixelSize: Math.max(1, Math.min(width, height))
    font.family: "Segoe UI Symbol"
    color: Theme.textColor
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
}