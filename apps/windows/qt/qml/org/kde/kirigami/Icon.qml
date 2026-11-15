import QtQuick

Item {
    id: root

    property string source: ""
    property int sourceSize: 16
    property color color: Theme.textColor

    // The shared QML uses the Kirigami Icon's Item geometry and color.
    // Text.implicitWidth/implicitHeight are read-only in Qt 6, so draw the
    // fallback glyph inside an Item instead of assigning them on Text.
    implicitWidth: sourceSize
    implicitHeight: sourceSize

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

    Text {
        anchors.fill: parent
        text: root.glyphFor(root.source)
        font.pixelSize: Math.max(1, Math.min(root.width, root.height))
        font.family: "Segoe UI Symbol"
        color: root.color
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }
}
