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

    // Deprecated: use themePaletteJson instead. Retained for backward-compatible reading.
    property string monetColor: ""
    property color _monetColorObj: monetColor

    // --- Theme palette (synced from Android Dynamic Color) ---
    // Non-Android clients only consume this; they never produce it.
    property string themePaletteJson: ""
    property var _themePalette: {
        if (themePaletteJson.length === 0) return null
        try { return JSON.parse(themePaletteJson) } catch(e) { return null }
    }
    property bool hasThemePalette: _themePalette !== null && _themePalette.source === "android_dynamic_color"

    function _paletteColor(lightKey, darkKey) {
        if (!hasThemePalette) return undefined
        var key = isDark ? darkKey : lightKey
        var val = _themePalette[key]
        if (val && val.length > 0 && val.charAt(0) === '#') {
            return Qt.rgba(
                parseInt(val.substring(1,3), 16) / 255,
                parseInt(val.substring(3,5), 16) / 255,
                parseInt(val.substring(5,7), 16) / 255,
                1
            )
        }
        return undefined
    }

    // --- Semantic color roles (素笺品牌语义色) ---
    property bool hasMonetColor: monetColor.length === 7 && monetColor.startsWith("#")
    property color primary: _paletteColor("light_primary", "dark_primary") ?? (hasMonetColor ? _monetColorObj : (isDark ? Qt.rgba(0.573, 0.800, 1.000, 1) : Qt.rgba(0.000, 0.392, 0.592, 1)))
    property color onPrimary: _paletteColor("light_on_primary", "dark_on_primary") ?? (isDark ? Qt.rgba(0.000, 0.200, 0.318, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color primaryContainer: _paletteColor("light_primary_container", "dark_primary_container") ?? (isDark ? Qt.rgba(0.000, 0.294, 0.451, 1) : Qt.rgba(0.800, 0.898, 1.000, 1))
    property color onPrimaryContainer: _paletteColor("light_on_primary_container", "dark_on_primary_container") ?? (isDark ? Qt.rgba(0.800, 0.898, 1.000, 1) : Qt.rgba(0.000, 0.118, 0.192, 1))
    property color secondary: _paletteColor("light_secondary", "dark_secondary") ?? (isDark ? Qt.rgba(0.722, 0.784, 0.855, 1) : Qt.rgba(0.318, 0.376, 0.435, 1))
    property color onSecondary: _paletteColor("light_on_secondary", "dark_on_secondary") ?? (isDark ? Qt.rgba(0.137, 0.196, 0.251, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color secondaryContainer: _paletteColor("light_secondary_container", "dark_secondary_container") ?? (isDark ? Qt.rgba(0.224, 0.282, 0.341, 1) : Qt.rgba(0.831, 0.894, 0.965, 1))
    property color onSecondaryContainer: _paletteColor("light_on_secondary_container", "dark_on_secondary_container") ?? (isDark ? Qt.rgba(0.831, 0.894, 0.965, 1) : Qt.rgba(0.055, 0.114, 0.165, 1))

    // --- Tertiary ---
    property color tertiary: _paletteColor("light_tertiary", "dark_tertiary") ?? (isDark ? Qt.rgba(0.843, 0.749, 1.000, 1) : Qt.rgba(0.427, 0.341, 0.549, 1))
    property color onTertiary: _paletteColor("light_on_tertiary", "dark_on_tertiary") ?? (isDark ? Qt.rgba(0.243, 0.165, 0.361, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color tertiaryContainer: _paletteColor("light_tertiary_container", "dark_tertiary_container") ?? (isDark ? Qt.rgba(0.333, 0.251, 0.455, 1) : Qt.rgba(0.945, 0.855, 1.000, 1))
    property color onTertiaryContainer: _paletteColor("light_on_tertiary_container", "dark_on_tertiary_container") ?? (isDark ? Qt.rgba(0.945, 0.855, 1.000, 1) : Qt.rgba(0.149, 0.078, 0.278, 1))

    property color background: _paletteColor("light_background", "dark_background") ?? (isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color onBackground: _paletteColor("light_on_background", "dark_on_background") ?? (isDark ? "#E2E2E5" : "#1A1C1E")
    property color surface: _paletteColor("light_surface", "dark_surface") ?? (isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color onSurface: isDark ? "#E2E2E5" : "#1A1C1E"
    property color surfaceVariant: _paletteColor("light_surface_variant", "dark_surface_variant") ?? (isDark ? Qt.rgba(0.259, 0.278, 0.306, 1) : Qt.rgba(0.875, 0.890, 0.922, 1))
    property color onSurfaceVariant: _paletteColor("light_on_surface_variant", "dark_on_surface_variant") ?? (isDark ? "#C3C6CF" : "#42474E")
    property color outline: _paletteColor("light_outline", "dark_outline") ?? (isDark ? Qt.rgba(0.549, 0.569, 0.596, 1) : Qt.rgba(0.447, 0.471, 0.494, 1))
    property color outlineVariant: _paletteColor("light_outline_variant", "dark_outline_variant") ?? (isDark ? Qt.rgba(0.259, 0.278, 0.306, 1) : Qt.rgba(0.757, 0.776, 0.812, 1))
    property color error: isDark ? Qt.rgba(1.000, 0.706, 0.671, 1) : Qt.rgba(0.729, 0.102, 0.102, 1)
    property color onError: isDark ? Qt.rgba(0.412, 0.000, 0.020, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)

    // --- Error container ---
    property color errorContainer: isDark ? Qt.rgba(0.576, 0.000, 0.039, 1) : Qt.rgba(1.000, 0.855, 0.839, 1)
    property color onErrorContainer: isDark ? Qt.rgba(1.000, 0.855, 0.839, 1) : Qt.rgba(0.255, 0.000, 0.008, 1)

    // --- Surface dim/bright ---
    property color surfaceDim: isDark ? Qt.rgba(0.071, 0.078, 0.094, 1) : Qt.rgba(0.843, 0.851, 0.875, 1)
    property color surfaceBright: isDark ? Qt.rgba(0.220, 0.224, 0.247, 1) : Qt.rgba(0.988, 0.988, 1.000, 1)

    // --- Inverse primary ---
    property color inversePrimary: isDark ? Qt.rgba(0.000, 0.392, 0.592, 1) : Qt.rgba(0.573, 0.800, 1.000, 1)

    // --- Desktop surfaces ---
    property color surfaceContainerLowest: _paletteColor("light_surface_container_lowest", "dark_surface_container_lowest") ?? (isDark ? Qt.rgba(0.059, 0.067, 0.075, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color surfaceContainerLow: _paletteColor("light_surface_container_low", "dark_surface_container_low") ?? (isDark ? Qt.rgba(0.122, 0.133, 0.145, 1) : Qt.rgba(0.965, 0.973, 0.984, 1))
    property color surfaceContainer: _paletteColor("light_surface_container", "dark_surface_container") ?? (isDark ? Qt.rgba(0.137, 0.153, 0.165, 1) : Qt.rgba(0.941, 0.953, 0.969, 1))
    property color surfaceContainerHigh: _paletteColor("light_surface_container_high", "dark_surface_container_high") ?? (isDark ? Qt.rgba(0.176, 0.192, 0.208, 1) : Qt.rgba(0.918, 0.937, 0.961, 1))
    property color surfaceContainerHighest: _paletteColor("light_surface_container_highest", "dark_surface_container_highest") ?? (isDark ? Qt.rgba(0.220, 0.235, 0.251, 1) : Qt.rgba(0.894, 0.914, 0.937, 1))
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
    property color textPrimary: isDark ? "#E2E2E5" : "#1A1C1E"
    property color textSecondary: isDark ? "#C3C6CF" : "#42474E"
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
    property color editorText: textPrimary

    // --- Component fallback semantic tokens ---
    // 统一 fallback 语义色，组件不再自行推断深浅色
    property color surfaceFallback: isDark ? "#1A1D23" : "#FCFCFF"
    property color surfaceContainerLowFallback: isDark ? "#1F2229" : "#F6F8FC"
    property color borderFallback: isDark ? "#2A2E36" : "#CBD5E1"
    property color borderWithAlpha: isDark ? Qt.rgba(0.165, 0.173, 0.192, 0.26) : Qt.rgba(0.443, 0.471, 0.502, 0.34)
    property color primaryFallback: isDark ? "#92CCFF" : "#006497"
    property color primaryContainerFallback: isDark ? "#004A77" : "#CCE5FF"
    property color hoverOverlay: isDark ? Qt.rgba(1, 1, 1, 0.08) : Qt.rgba(0, 0, 0, 0.04)
    property color pressOverlay: isDark ? Qt.rgba(1, 1, 1, 0.12) : Qt.rgba(0, 0, 0, 0.08)
    property color disabledOverlay: isDark ? Qt.rgba(1, 1, 1, 0.04) : Qt.rgba(0, 0, 0, 0.02)
    property color divider: isDark ? Qt.rgba(0.165, 0.173, 0.192, 0.26) : Qt.rgba(0.839, 0.859, 0.886, 0.52)
    property color starMapNodeChapter: isDark ? "#4CAF50" : "#2E7D32"
    property color starMapNodeCharacter: isDark ? "#2196F3" : "#1565C0"
    property color starMapNodeLocation: isDark ? "#FF9800" : "#E65100"
    property color starMapNodeEvent: isDark ? "#F44336" : "#C62828"
    property color starMapNodeConcept: isDark ? "#9C27B0" : "#6A1B9A"

    // --- Project accent colors (decorative, used in project cards) ---
    property var projectAccentColors: isDark
        ? ["#7B8CDE", "#DE8C7B", "#7BDE8C", "#DE7BC4", "#7BC4DE", "#C4DE7B"]
        : ["#5B6CAE", "#BE6C5B", "#5BBE6C", "#BE5BA4", "#5BA4BE", "#A4BE5B"]

    property string textPrimaryHex: isDark ? "#E2E2E5" : "#1A1C1E"
    property string textSecondaryHex: isDark ? "#C3C6CF" : "#42474E"

    // --- Radius ---
    property int radiusXs: 4
    property int radiusSm: 8
    property int radiusMd: 12
    property int radiusLg: 16
    property int radiusXl: 28
    property int radiusPill: 999
    property int radiusCard: radiusLg
    property int radiusPanel: radiusXl

    // --- Component shape tokens (cross-platform UI token contract) ---
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
    property int statusDotSize: 7

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
