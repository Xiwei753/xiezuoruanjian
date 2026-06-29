// =============================================================================
// FallbackTokens.qml — 设计令牌安全回退
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：当组件的 dt 属性为 null 时提供安全的默认值，防止 TypeError
// 约束：
//   - 默认值与 DesignTokens light 主题一致
//   - 仅在 dt 为 null 时使用，不应替代正常的 dt 传递
//   - 每个组件内部实例化一个 _fallback，通过 _dt: dt ?? _fallback 安全访问
// =============================================================================

import QtQuick

QtObject {
    id: fallback

    // --- Theme ---
    property bool isDark: false

    // --- Semantic color roles (light theme defaults) ---
    property color primary: Qt.rgba(0.000, 0.392, 0.592, 1)
    property color onPrimary: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color primaryContainer: Qt.rgba(0.800, 0.898, 1.000, 1)
    property color onPrimaryContainer: Qt.rgba(0.000, 0.118, 0.192, 1)
    property color secondary: Qt.rgba(0.318, 0.376, 0.435, 1)
    property color onSecondary: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color secondaryContainer: Qt.rgba(0.831, 0.894, 0.965, 1)
    property color onSecondaryContainer: Qt.rgba(0.055, 0.114, 0.165, 1)

    property color tertiary: Qt.rgba(0.427, 0.341, 0.549, 1)
    property color onTertiary: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color tertiaryContainer: Qt.rgba(0.945, 0.855, 1.000, 1)
    property color onTertiaryContainer: Qt.rgba(0.149, 0.078, 0.278, 1)

    property color background: Qt.rgba(0.988, 0.988, 1.000, 1)
    property color onBackground: "#1A1C1E"
    property color surface: Qt.rgba(0.988, 0.988, 1.000, 1)
    property color onSurface: "#1A1C1E"
    property color surfaceVariant: Qt.rgba(0.875, 0.890, 0.922, 1)
    property color onSurfaceVariant: "#42474E"
    property color outline: Qt.rgba(0.447, 0.471, 0.494, 1)
    property color outlineVariant: Qt.rgba(0.757, 0.776, 0.812, 1)
    property color error: Qt.rgba(0.729, 0.102, 0.102, 1)
    property color onError: Qt.rgba(1.000, 1.000, 1.000, 1)

    property color errorContainer: Qt.rgba(1.000, 0.855, 0.839, 1)
    property color onErrorContainer: Qt.rgba(0.255, 0.000, 0.008, 1)

    property color surfaceDim: Qt.rgba(0.843, 0.851, 0.875, 1)
    property color surfaceBright: Qt.rgba(0.988, 0.988, 1.000, 1)

    property color inversePrimary: Qt.rgba(0.573, 0.800, 1.000, 1)

    property color surfaceContainerLowest: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color surfaceContainerLow: Qt.rgba(0.965, 0.973, 0.984, 1)
    property color surfaceContainer: Qt.rgba(0.941, 0.953, 0.969, 1)
    property color surfaceContainerHigh: Qt.rgba(0.918, 0.937, 0.961, 1)
    property color surfaceContainerHighest: Qt.rgba(0.894, 0.914, 0.937, 1)
    property color inverseSurface: Qt.rgba(0.184, 0.188, 0.200, 1)
    property color inverseOnSurface: Qt.rgba(0.945, 0.941, 0.957, 1)
    property color scrim: Qt.rgba(0.000, 0.000, 0.000, 1)

    // --- Semantic colors ---
    property color success: Qt.rgba(0.122, 0.478, 0.271, 1)
    property color onSuccess: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color successContainer: Qt.rgba(0.725, 0.941, 0.784, 1)
    property color onSuccessContainer: Qt.rgba(0.000, 0.129, 0.059, 1)
    property color warning: Qt.rgba(0.478, 0.345, 0.000, 1)
    property color onWarning: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color warningContainer: Qt.rgba(1.000, 0.886, 0.659, 1)
    property color onWarningContainer: Qt.rgba(0.149, 0.102, 0.000, 1)
    property color info: primary
    property color onInfo: onPrimary
    property color infoContainer: primaryContainer
    property color onInfoContainer: onPrimaryContainer

    // --- Derived app roles ---
    property color bg: background
    property color paper: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color border: Qt.rgba(outline.r, outline.g, outline.b, 0.34)
    property color borderStrong: outline
    property color sidebar: Qt.rgba(0.953, 0.969, 0.988, 1)
    property color card: surfaceContainerLow
    property color cardHover: surfaceContainer
    property color selected: primaryContainer
    property color selectedText: onPrimaryContainer
    property color textPrimary: "#1A1C1E"
    property color textSecondary: "#42474E"
    property color textMuted: Qt.rgba(0.455, 0.471, 0.498, 1)
    property color textDisabled: Qt.rgba(onSurface.r, onSurface.g, onSurface.b, 0.38)
    property color defaultAccent: primary
    property color defaultAccentHover: Qt.darker(primary, 1.08)
    property color defaultAccentText: primary
    property color accent: primary
    property color accentSoft: primaryContainer
    property color accentHover: Qt.darker(primary, 1.08)
    property color accentText: onPrimaryContainer
    property color danger: error
    property color dangerContainer: Qt.rgba(1.000, 0.855, 0.839, 1)
    property color onDangerContainer: Qt.rgba(0.255, 0.000, 0.008, 1)

    // --- Editor ---
    property color editorBackground: Qt.rgba(1.000, 1.000, 1.000, 1)
    property color editorText: textPrimary
    property string textPrimaryHex: "#1A1C1E"
    property string textSecondaryHex: "#42474E"

    // --- Radius ---
    property int radiusXs: 4
    property int radiusSm: 8
    property int radiusMd: 12
    property int radiusLg: 16
    property int radiusXl: 24
    property int radiusPill: 999
    property int radiusCard: radiusLg
    property int radiusPanel: radiusXl

    // --- Component shape tokens ---
    property int cardRadius: radiusLg
    property int dialogRadius: radiusXl
    property int fabRadius: radiusLg
    property int bottomBarRadius: 0
    property int inputFieldRadius: radiusMd

    // --- Elevation ---
    property real elevation0: 0
    property real elevation1: 1
    property real elevation2: 3
    property real elevation3: 6

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
    property color switchThumb: Qt.rgba(1.000, 1.000, 1.000, 1)

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
    property color shadowLight: Qt.rgba(0, 0, 0, 0.06)
    property color shadowMedium: Qt.rgba(0, 0, 0, 0.10)
    property color shadowDrawer: Qt.rgba(0, 0, 0, 0.12)

    // --- Transition ---
    property int animFast: 120
    property int animNormal: 200
}
