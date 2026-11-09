pragma Singleton
import QtQuick
import QtQuick.Controls

QtObject {
    readonly property bool isDark: Application.styleHints.colorScheme === Qt.ColorScheme.Dark

    readonly property color backgroundColor: isDark ? "#202020" : "#f3f3f3"
    readonly property color activeBackgroundColor: isDark ? "#2d2d2d" : "#ffffff"
    readonly property color alternateBackgroundColor: isDark ? "#292929" : "#f7f7f7"
    readonly property color hoverColor: isDark ? "#383838" : "#e8e8e8"
    readonly property color focusColor: isDark ? "#4cc2ff" : "#0078d4"
    readonly property color highlightColor: isDark ? "#0078d4" : "#0078d4"
    readonly property color highlightedTextColor: "#ffffff"
    readonly property color textColor: isDark ? "#eeeeee" : "#202020"
    readonly property color disabledTextColor: isDark ? "#8a8a8a" : "#9a9a9a"
    readonly property color negativeTextColor: isDark ? "#ff99a4" : "#c42b1c"
    readonly property color neutralTextColor: isDark ? "#f0c674" : "#9d5d00"
    readonly property color positiveTextColor: isDark ? "#7bd88f" : "#107c10"

    readonly property font defaultFont: Qt.font({ family: "Segoe UI", pointSize: 10 })
    readonly property font smallFont: Qt.font({ family: "Segoe UI", pointSize: 9 })
}
