import QtQuick 2.15
import QtQuick.Layouts 1.15

ColumnLayout {
    id: root
    property var dt: null
    readonly property bool wide: width >= 1120
    readonly property bool medium: width >= 760 && width < 1120
    property int gap: dt ? dt.gridGap : 16
    spacing: gap
}
