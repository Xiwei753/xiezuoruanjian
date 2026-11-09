pragma Singleton
import QtQuick
import QtQuick.Controls

QtObject {
    // 颜色全部来自当前 QApplication 调色板。显式浅色/深色由 Windows 主题桥写入
    // 调色板；system 模式由 Qt/Windows 自己维护，避免两套状态互相打脸。
    readonly property SystemPalette palette: SystemPalette { colorGroup: SystemPalette.Active }
    readonly property color backgroundColor: palette.window
    readonly property color activeBackgroundColor: palette.base
    readonly property color alternateBackgroundColor: palette.alternateBase
    readonly property color textColor: palette.windowText
    readonly property color disabledTextColor: Qt.rgba(textColor.r, textColor.g, textColor.b, 0.45)
    readonly property color focusColor: palette.highlight
    readonly property color highlightColor: palette.highlight
    readonly property color highlightedTextColor: palette.highlightedText
    readonly property color hoverColor: Qt.lighter(activeBackgroundColor, 1.12)
    readonly property color negativeTextColor: isDark ? "#ff99a4" : "#c42b1c"
    readonly property color neutralTextColor: isDark ? "#f0c674" : "#9d5d00"
    readonly property color positiveTextColor: isDark ? "#7bd88f" : "#107c10"

    readonly property real _luminance: 0.2126 * backgroundColor.r
        + 0.7152 * backgroundColor.g
        + 0.0722 * backgroundColor.b
    readonly property bool isDark: _luminance < 0.5

    readonly property font defaultFont: Qt.font({ family: "Segoe UI", pointSize: 10 })
    readonly property font smallFont: Qt.font({ family: "Segoe UI", pointSize: 9 })
}
