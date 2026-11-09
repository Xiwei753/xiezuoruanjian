import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    default property alias contentData: internalColumn.data
    property int spacing: Kirigami.Units.smallSpacing

    implicitHeight: internalColumn.implicitHeight + 24
    implicitWidth: internalColumn.implicitWidth + 24
    Layout.fillWidth: true

    Rectangle {
        anchors.fill: parent
        radius: Kirigami.Units.cornerRadius
        color: Kirigami.Theme.alternateBackgroundColor
        border.color: Kirigami.Theme.isDark ? "#3a3a3a" : "#e0e0e0"

        ColumnLayout {
            id: internalColumn
            anchors.fill: parent
            anchors.margins: 12
            spacing: root.spacing
        }
    }
}
