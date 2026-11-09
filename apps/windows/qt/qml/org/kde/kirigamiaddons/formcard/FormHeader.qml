import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

Label {
    property string title: ""
    text: title
    font.bold: true
    font.pixelSize: 15
    color: Kirigami.Theme.textColor
    topPadding: 8
    bottomPadding: 2
}
