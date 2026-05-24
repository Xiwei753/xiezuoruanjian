import QtQuick 2.15

QtObject {
    id: dt

    property bool isDark: true

    // --- Background ---
    property color darkBg: "#111318"
    property color darkSurface: "#1A1D23"
    property color darkPaper: "#191C21"
    property color darkBorder: "#2A2E36"
    property color darkSidebar: "#14161B"
    property color darkCard: "#1E2128"
    property color darkCardHover: "#22262E"

    property color lightBg: "#F3F1EC"
    property color lightSurface: "#FAF8F3"
    property color lightPaper: "#FFFDF8"
    property color lightBorder: "#E2DED6"
    property color lightSidebar: "#EDE9E1"
    property color lightCard: "#FFFFFF"
    property color lightCardHover: "#FAFAF7"

    // --- Derived ---
    property color bg: isDark ? darkBg : lightBg
    property color surface: isDark ? darkSurface : lightSurface
    property color paper: isDark ? darkPaper : lightPaper
    property color border: isDark ? darkBorder : lightBorder
    property color sidebar: isDark ? darkSidebar : lightSidebar
    property color card: isDark ? darkCard : lightCard
    property color cardHover: isDark ? darkCardHover : lightCardHover

    // --- Text ---
    property color textPrimary: isDark ? "#E2E4E9" : "#1A1C23"
    property color textSecondary: isDark ? "#9CA0AB" : "#5C6070"
    property color textMuted: isDark ? "#606470" : "#8E9099"

    // --- Accent ---
    property color accent: isDark ? "#7B8CDE" : "#5B6BC0"
    property color accentSoft: isDark ? Qt.rgba(0.48, 0.55, 0.87, 0.12) : Qt.rgba(0.36, 0.42, 0.75, 0.08)
    property color accentHover: isDark ? "#8E9EE8" : "#4A5AB0"
    property color accentText: isDark ? "#C5CCEE" : "#3D4D9E"

    // --- Semantic ---
    property color danger: isDark ? "#E06060" : "#D33030"
    property color warning: isDark ? "#E0A840" : "#C88820"
    property color success: isDark ? "#5CB880" : "#309060"

    // --- Editor ---
    property color editorBackground: isDark ? darkPaper : lightPaper
    property color editorText: isDark ? "#D8DAE0" : "#2C2E36"

    // --- Radius ---
    property int radiusSm: 8
    property int radiusMd: 12
    property int radiusCard: 18
    property int radiusPanel: 22

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
    property int pageMarginWide: 40
    property int pageMarginNarrow: 24
    property int pageHeaderHeight: 72
    property int cardGap: 16
    property int gridGap: 16
    property int actionButtonHeight: 40
    property int actionButtonRadius: 12

    // --- Controls ---
    property color controlBorder: isDark ? "#3A3F49" : "#D2CDC3"
    property color surfaceVariant: isDark ? "#242933" : "#F1EEE7"
    property color switchTrackOn: isDark ? "#6679D8" : "#5B6BC0"
    property color switchTrackOff: isDark ? "#303543" : "#E1DBD0"
    property color switchThumb: isDark ? "#EEF1FB" : "#FFFFFF"

    // --- Font ---
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

    // --- Legacy aliases (for SettingsDialog, SyncPage, etc.) ---
    property color bgDark: bg
    property color bgDarker: surface
    property color surfaceAlt: surface
    property color divider: border
    property color primaryHover: accentHover
    property color primaryText: accentText
    property color textMain: textPrimary
    property color textDim: textMuted
    property color sidebarBg: sidebar
    property color sidebarHover: cardHover
    property color inputBg: paper
    property color buttonBg: surface
    property color buttonHover: cardHover
    property color editorBg: editorBackground
    property color hover: cardHover
    property color primary: accent
    property color textDisabled: isDark ? "#4A4E58" : "#B0B3BA"
}
