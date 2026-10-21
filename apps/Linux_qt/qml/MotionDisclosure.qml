import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    default property alias contentData: contentLayout.data
    property bool expanded: true
    property real revealProgress: expanded ? 1 : 0

    implicitWidth: contentLayout.implicitWidth
    implicitHeight: contentLayout.implicitHeight * revealProgress
    clip: true
    enabled: expanded

    Behavior on revealProgress {
        NumberAnimation {
            duration: 200
            easing.type: Easing.OutCubic
        }
    }

    ColumnLayout {
        id: contentLayout
        x: 0
        y: 0
        width: root.width
        height: implicitHeight
        spacing: Kirigami.Units.smallSpacing
    }
}
