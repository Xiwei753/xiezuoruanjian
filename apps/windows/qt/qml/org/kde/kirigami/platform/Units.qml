pragma Singleton
import QtQuick
import org.kde.kirigami as Kirigami

QtObject {
    readonly property int gridUnit: Kirigami.Units.gridUnit
    readonly property int smallSpacing: Kirigami.Units.smallSpacing
    readonly property int largeSpacing: Kirigami.Units.largeSpacing
    readonly property int cornerRadius: Kirigami.Units.cornerRadius
    readonly property int shortDuration: Kirigami.Units.shortDuration
    readonly property int longDuration: Kirigami.Units.longDuration
    readonly property int veryLongDuration: Kirigami.Units.veryLongDuration
    readonly property QtObject iconSizes: Kirigami.Units.iconSizes
}
