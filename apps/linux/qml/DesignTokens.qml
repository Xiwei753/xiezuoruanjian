// =============================================================================
// DesignTokens.qml — 设计令牌
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
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

    // --- Material 3 color roles (aligned with Android colors.xml) ---
    property bool hasMonetColor: monetColor.length === 7 && monetColor.startsWith("#")
    property color primary: hasMonetColor ? monetColor : (isDark ? "#92CCFF" : "#006497")
    property color onPrimary: isDark ? "#003351" : "#FFFFFF"
    property color primaryContainer: isDark ? "#004B73" : "#CCE5FF"
    property color onPrimaryContainer: isDark ? "#CCE5FF" : "#001E31"
    property color secondary: isDark ? "#B8C8DA" : "#51606F"
    property color onSecondary: isDark ? "#233240" : "#FFFFFF"
    property color secondaryContainer: isDark ? "#394857" : "#D4E4F6"
    property color onSecondaryContainer: isDark ? "#D4E4F6" : "#0E1D2A"
    property color background: isDark ? "#1A1C1E" : "#FCFCFF"
    property color onBackground: isDark ? "#E2E2E5" : "#1A1C1E"
    property color surface: isDark ? "#1A1C1E" : "#FCFCFF"
    property color onSurface: isDark ? "#E2E2E5" : "#1A1C1E"
    property color surfaceVariant: isDark ? "#42474E" : "#DFE3EB"
    property color onSurfaceVariant: isDark ? "#C3C6CF" : "#42474E"
    property color outline: isDark ? "#8C9198" : "#72787E"
    property color error: isDark ? "#FFB4AB" : "#BA1A1A"
    property color onError: isDark ? "#690005" : "#FFFFFF"

    // --- Desktop surfaces ---
    property color surfaceContainerLowest: isDark ? "#0F1113" : "#FFFFFF"
    property color surfaceContainerLow: isDark ? "#1F2225" : "#F6F8FB"
    property color surfaceContainer: isDark ? "#23272A" : "#F0F3F7"
    property color surfaceContainerHigh: isDark ? "#2D3135" : "#EAEFF5"
    property color surfaceContainerHighest: isDark ? "#383C40" : "#E4E9EF"
    property color inverseSurface: isDark ? "#E2E2E5" : "#2F3033"
    property color inverseOnSurface: isDark ? "#2F3033" : "#F1F0F4"
    property color scrim: "#000000"

    // --- Semantic colors ---
    property color success: isDark ? "#8FD6A3" : "#1F7A45"
    property color onSuccess: isDark ? "#00391D" : "#FFFFFF"
    property color successContainer: isDark ? "#0F5A30" : "#B9F0C8"
    property color onSuccessContainer: isDark ? "#B9F0C8" : "#00210F"
    property color warning: isDark ? "#F4C56A" : "#7A5800"
    property color onWarning: isDark ? "#402D00" : "#FFFFFF"
    property color warningContainer: isDark ? "#5D4200" : "#FFE2A8"
    property color onWarningContainer: isDark ? "#FFE2A8" : "#261A00"
    property color info: primary
    property color onInfo: onPrimary
    property color infoContainer: primaryContainer
    property color onInfoContainer: onPrimaryContainer

    // --- Derived app roles ---
    property color bg: background
    property color paper: isDark ? "#202326" : "#FFFFFF"
    property color border: isDark ? Qt.rgba(outline.r, outline.g, outline.b, 0.42) : Qt.rgba(outline.r, outline.g, outline.b, 0.34)
    property color borderStrong: outline
    property color sidebar: isDark ? "#15181B" : "#F3F7FC"
    property color card: surfaceContainerLow
    property color cardHover: surfaceContainer
    property color selected: primaryContainer
    property color selectedText: onPrimaryContainer
    property color textPrimary: onSurface
    property color textSecondary: onSurfaceVariant
    property color textMuted: isDark ? "#8C9198" : "#74787F"
    property color textDisabled: isDark ? Qt.rgba(onSurface.r, onSurface.g, onSurface.b, 0.38) : Qt.rgba(onSurface.r, onSurface.g, onSurface.b, 0.38)
    property color defaultAccent: primary
    property color defaultAccentHover: isDark ? Qt.lighter(primary, 1.08) : Qt.darker(primary, 1.08)
    property color defaultAccentText: primary
    property color accent: primary
    property color accentSoft: primaryContainer
    property color accentHover: defaultAccentHover
    property color accentText: onPrimaryContainer
    property color danger: error
    property color dangerContainer: isDark ? "#93000A" : "#FFDAD6"
    property color onDangerContainer: isDark ? "#FFDAD6" : "#410002"

    // --- Editor ---
    property color editorBackground: isDark ? "#202326" : "#FFFFFF"
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
    property color switchThumb: isDark ? "#D8EFFF" : "#FFFFFF"

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

    // --- Legacy aliases (kept so existing pages do not duplicate color logic) ---
    property color bgDark: bg
    property color bgDarker: surfaceContainerLowest
    property color surfaceAlt: surfaceContainer
    property color divider: border
    property color primaryHover: accentHover
    property color primaryText: accentText
    property color textMain: textPrimary
    property color textDim: textMuted
    property color sidebarBg: sidebar
    property color sidebarHover: surfaceContainer
    property color buttonBg: surface
    property color buttonHover: cardHover
    property color editorBg: editorBackground
    property color hover: cardHover
    property color text: textPrimary
}
