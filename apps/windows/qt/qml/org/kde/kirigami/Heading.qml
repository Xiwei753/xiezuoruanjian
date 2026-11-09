import QtQuick

Text {
    property int level: 3

    font.family: "Segoe UI"
    font.bold: true
    font.pixelSize: level <= 1 ? 26 : level === 2 ? 21 : level === 3 ? 17 : 15
    color: Theme.textColor
    wrapMode: Text.WordWrap
}
