// =============================================================================
// DesignTokens.qml — 设计令牌
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：全局颜色、间距、字号、圆角、动画时长定义
// 约束：
//   - 所有 UI 组件必须引用此文件的令牌，禁止硬编码颜色/间距
//   - 支持深色/浅色主题切换（isDark 属性）
//   - 是 UI 一致性的唯一事实来源
// =============================================================================

import QtQuick

QtObject {
    id: dt

    property bool isDark: true
    property string monetColor: ""
    property color _monetColorObj: monetColor

    // --- Material 3 color roles (aligned with Android colors.xml) ---
    property bool hasMonetColor: monetColor.length === 7 && monetColor.startsWith("#")
    property color primary: hasMonetColor ? _monetColorObj : (isDark ? Qt.rgba(0.573, 0.800, 1.000, 1) : Qt.rgba(0.000, 0.392, 0.592, 1))
    property color onPrimary: isDark ? Qt.rgba(0.000, 0.200, 0.318, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color primaryContainer: isDark ? Qt.rgba(0.000, 0.294, 0.451, 1) : Qt.rgba(0.800, 0.898, 1.000, 1)
    property color onPrimaryContainer: isDark ? Qt.rgba(0.800, 0.898, 1.000, 1) : Qt.rgba(0.000, 0.118, 0.192, 1)
    property color secondary: isDark ? Qt.rgba(0.722, 0.784, 0.855, 1) : Qt.rgba(0.318, 0.376, 0.435, 1)
    property color onSecondary: isDark ? Qt.rgba(0.137, 0.196, 0.251, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color secondaryContainer: isDark ? Qt.rgba(0.224, 0.282, 0.341, 1) : Qt.rgba(0.831, 0.894, 0.965, 1)
    property color onSecondaryContainer: isDark ? Qt.rgba(0.831, 0.894, 0.965, 1) : Qt.rgba(0.055, 0.114, 0.165, 1)
    property color background: isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1)
    property color onBackground: isDark ? Qt.rgba(0.886, 0.886, 0.898, 1) : Qt.rgba(0.102, 0.110, 0.118, 1)
    property color surface: isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1)
    property color onSurface: isDark ? Qt.rgba(0.886, 0.886, 0.898, 1) : Qt.rgba(0.102, 0.110, 0.118, 1)
    property color surfaceVariant: isDark ? Qt.rgba(0.259, 0.278, 0.306, 1) : Qt.rgba(0.875, 0.890, 0.922, 1)
    property color onSurfaceVariant: isDark ? Qt.rgba(0.765, 0.776, 0.812, 1) : Qt.rgba(0.259, 0.278, 0.306, 1)
    property color outline: isDark ? Qt.rgba(0.549, 0.569, 0.596, 1) : Qt.rgba(0.447, 0.471, 0.494, 1)
    property color error: isDark ? Qt.rgba(1.000, 0.706, 0.671, 1) : Qt.rgba(0.729, 0.102, 0.102, 1)
    property color onError: isDark ? Qt.rgba(0.412, 0.000, 0.020, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)

    // --- Desktop surfaces ---
    property color surfaceContainerLowest: isDark ? Qt.rgba(0.059, 0.067, 0.075, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color surfaceContainerLow: isDark ? Qt.rgba(0.122, 0.133, 0.145, 1) : Qt.rgba(0.965, 0.973, 0.984, 1)
    property color surfaceContainer: isDark ? Qt.rgba(0.137, 0.153, 0.165, 1) : Qt.rgba(0.941, 0.953, 0.969, 1)
    property color surfaceContainerHigh: isDark ? Qt.rgba(0.176, 0.192, 0.208, 1) : Qt.rgba(0.918, 0.937, 0.961, 1)
    property color surfaceContainerHighest: isDark ? Qt.rgba(0.220, 0.235, 0.251, 1) : Qt.rgba(0.894, 0.914, 0.937, 1)
    property color inverseSurface: isDark ? Qt.rgba(0.886, 0.886, 0.898, 1) : Qt.rgba(0.184, 0.188, 0.200, 1)
    property color inverseOnSurface: isDark ? Qt.rgba(0.184, 0.188, 0.200, 1) : Qt.rgba(0.945, 0.941, 0.957, 1)
    property color scrim: Qt.rgba(0.000, 0.000, 0.000, 1)

    // --- Semantic colors ---
    property color success: isDark ? Qt.rgba(0.561, 0.839, 0.639, 1) : Qt.rgba(0.122, 0.478, 0.271, 1)
    property color onSuccess: isDark ? Qt.rgba(0.000, 0.224, 0.114, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color successContainer: isDark ? Qt.rgba(0.059, 0.353, 0.188, 1) : Qt.rgba(0.725, 0.941, 0.784, 1)
    property color onSuccessContainer: isDark ? Qt.rgba(0.725, 0.941, 0.784, 1) : Qt.rgba(0.000, 0.129, 0.059, 1)
    property color warning: isDark ? Qt.rgba(0.957, 0.773, 0.416, 1) : Qt.rgba(0.478, 0.345, 0.000, 1)
    property color onWarning: isDark ? Qt.rgba(0.251, 0.176, 0.000, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color warningContainer: isDark ? Qt.rgba(0.365, 0.259, 0.000, 1) : Qt.rgba(1.000, 0.886, 0.659, 1)
    property color onWarningContainer: isDark ? Qt.rgba(1.000, 0.886, 0.659, 1) : Qt.rgba(0.149, 0.102, 0.000, 1)
    property color info: primary
    property color onInfo: onPrimary
    property color infoContainer: primaryContainer
    property color onInfoContainer: onPrimaryContainer

    // --- Derived app roles ---
    property color bg: background
    property color paper: isDark ? Qt.rgba(0.125, 0.137, 0.149, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color border: isDark ? Qt.rgba(outline.r, outline.g, outline.b, 0.42) : Qt.rgba(outline.r, outline.g, outline.b, 0.34)
    property color borderStrong: outline
    property color sidebar: isDark ? Qt.rgba(0.082, 0.094, 0.106, 1) : Qt.rgba(0.953, 0.969, 0.988, 1)
    property color card: surfaceContainerLow
    property color cardHover: surfaceContainer
    property color selected: primaryContainer
    property color selectedText: onPrimaryContainer
    property color textPrimary: onSurface
    property color textSecondary: onSurfaceVariant
    property color textMuted: isDark ? Qt.rgba(0.549, 0.569, 0.596, 1) : Qt.rgba(0.455, 0.471, 0.498, 1)
    property color textDisabled: isDark ? Qt.rgba(onSurface.r, onSurface.g, onSurface.b, 0.38) : Qt.rgba(onSurface.r, onSurface.g, onSurface.b, 0.38)
    property color defaultAccent: primary
    property color defaultAccentHover: isDark ? Qt.lighter(primary, 1.08) : Qt.darker(primary, 1.08)
    property color defaultAccentText: primary
    property color accent: primary
    property color accentSoft: primaryContainer
    property color accentHover: defaultAccentHover
    property color accentText: onPrimaryContainer
    property color danger: error
    property color dangerContainer: isDark ? Qt.rgba(0.576, 0.000, 0.039, 1) : Qt.rgba(1.000, 0.855, 0.839, 1)
    property color onDangerContainer: isDark ? Qt.rgba(1.000, 0.855, 0.839, 1) : Qt.rgba(0.255, 0.000, 0.008, 1)

    // --- Editor ---
    property color editorBackground: isDark ? Qt.rgba(0.125, 0.137, 0.149, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color editorText: onSurface
    property string editorTextHex: isDark ? "#E2E2E5" : "#1A1C1E"
    property string textPrimaryHex: isDark ? "#E2E2E5" : "#1A1C1E"
    property string textSecondaryHex: isDark ? "#C3C6CF" : "#42474E"

    // --- Radius ---
    property int radiusXs: 4
    property int radiusSm: 8
    property int radiusMd: 12
    property int radiusLg: 16
    property int radiusXl: 24
    property int radiusPill: 999
    property int radiusCard: radiusLg
    property int radiusPanel: radiusXl

    // --- Spacing ---
    property int sp4: 4
    property int sp6: 6
    property int sp8: 8
    property int sp10: 10
    property int sp12: 12
    property int sp16: 16
    property int sp20: 20
    property int sp24: 24
    property int sp32: 32
    property int sp40: 40
    property int sp48: 48
    property int sp64: 64

    // --- Hub Layout ---
    property int pageMarginWide: 48
    property int pageMarginNarrow: 24
    property int maxContentWidth: 1240
    property int pageHeaderHeight: 76
    property int cardGap: 16
    property int gridGap: 16
    property int actionButtonHeight: 40
    property int actionButtonRadius: 12
    property int settingsRowHeight: 68
    property int settingsControlHeight: 40

    // --- Controls ---
    property color controlBorder: border
    property color borderFocus: primary
    property color inputBg: surfaceContainerLow
    property color switchTrackOn: primary
    property color switchTrackOff: surfaceVariant
    property color switchThumb: isDark ? Qt.rgba(0.847, 0.937, 1.000, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)

    // --- Typography ---
    property string fontFamily: "sans-serif"
    property int display: 28
    property int title: 24
    property int subtitle: 18
    property int body: 14
    property int label: 13
    property int caption: 12
    property int fontXs: 11
    property int fontSm: 12
    property int fontMd: 14
    property int fontLg: 16
    property int fontXl: 18
    property int fontXxl: 22
    property int fontTitle: 26

    // --- Shadow ---
    property color shadowLight: isDark ? Qt.rgba(0, 0, 0, 0.25) : Qt.rgba(0, 0, 0, 0.06)
    property color shadowMedium: isDark ? Qt.rgba(0, 0, 0, 0.4) : Qt.rgba(0, 0, 0, 0.10)
    property color shadowDrawer: isDark ? Qt.rgba(0, 0, 0, 0.5) : Qt.rgba(0, 0, 0, 0.12)

    // --- Transition ---
    property int animFast: 120
    property int animNormal: 200
}
