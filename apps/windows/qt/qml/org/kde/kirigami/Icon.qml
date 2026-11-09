import QtQuick

Text {
    id: root

    property string source: ""
    property int sourceSize: 16

    function glyphFor(name) {
        switch (name) {
        case "arrow-right": return "→"
        case "dialog-ok": return "✓"
        case "document-open": return "📂"
        case "edit-find": return "🔍"
        case "folder-open": return "📁"
        case "list-add": return "+"
        case "starred": return "★"
        case "view-refresh": return "↻"
        default: return "•"
        }
    }

    text: glyphFor(source)
    font.pixelSize: sourceSize
    font.family: "Segoe UI Symbol"
    color: Theme.textColor
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    implicitWidth: Math.max(sourceSize, contentWidth)
    implicitHeight: Math.max(sourceSize, contentHeight)
}
