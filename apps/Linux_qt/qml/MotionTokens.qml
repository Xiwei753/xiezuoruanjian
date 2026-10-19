import QtQuick
import org.kde.kirigami as Kirigami

QtObject {
    readonly property int pressMs: Math.min(110, Kirigami.Units.shortDuration)
    readonly property int hoverMs: Kirigami.Units.shortDuration
    readonly property int menuMs: Math.min(160, Kirigami.Units.shortDuration)
    readonly property int dialogMs: Math.min(240, Kirigami.Units.longDuration)
}
