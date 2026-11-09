pragma Singleton
import QtQuick
import org.kde.kirigami as Kirigami

QtObject {
    readonly property bool isDark: Kirigami.Theme.isDark
    readonly property color backgroundColor: Kirigami.Theme.backgroundColor
    readonly property color activeBackgroundColor: Kirigami.Theme.activeBackgroundColor
    readonly property color alternateBackgroundColor: Kirigami.Theme.alternateBackgroundColor
    readonly property color textColor: Kirigami.Theme.textColor
    readonly property color disabledTextColor: Kirigami.Theme.disabledTextColor
    readonly property color highlightColor: Kirigami.Theme.highlightColor
    readonly property color highlightedTextColor: Kirigami.Theme.highlightedTextColor
    readonly property font defaultFont: Kirigami.Theme.defaultFont
    readonly property font smallFont: Kirigami.Theme.smallFont
}
