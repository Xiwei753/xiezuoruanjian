import QtQuick

QtObject {
    id: dt

    property bool isDark: true

    onIsDarkChanged: {
    }

    property string themePaletteJson: ""
    property string colorSource: "built_in"
    property string selectedBuiltinThemeId: ""
    property string builtinThemesJson: "[]"
    property var _themePalette: {
        if (themePaletteJson.length === 0) return null
        try { return JSON.parse(themePaletteJson) } catch(e) { return null }
    }
    property var _builtinThemes: {
        if (builtinThemesJson.length === 0) return []
        try { return JSON.parse(builtinThemesJson) } catch(e) { return [] }
    }
    property var _selectedBuiltin: {
        if (selectedBuiltinThemeId.length === 0) return _builtinThemes.length > 0 ? _builtinThemes[0] : null
        for (var i = 0; i < _builtinThemes.length; i++) {
            if (_builtinThemes[i].themeId === selectedBuiltinThemeId) return _builtinThemes[i]
        }
        return _builtinThemes.length > 0 ? _builtinThemes[0] : null
    }
    property bool hasThemePalette: {
        if (!_themePalette) return false
        if (_themePalette.lightScheme && _themePalette.darkScheme) return true
        return false
    }
    property bool useThemePalette: colorSource === "saved_palette" && hasThemePalette

    function _schemeColor(key) {
        var schemeKey = isDark ? "darkScheme" : "lightScheme"
        var scheme = null

        if (useThemePalette && _themePalette) {
            scheme = _themePalette[schemeKey]
        } else if (_selectedBuiltin) {
            scheme = _selectedBuiltin[schemeKey]
        }

        if (scheme) {
            var val = scheme[key]
            if (val && val.length > 0 && val.charAt(0) === '#') {
                return Qt.rgba(
                    parseInt(val.substring(1,3), 16) / 255,
                    parseInt(val.substring(3,5), 16) / 255,
                    parseInt(val.substring(5,7), 16) / 255,
                    1
                )
            }
        }
        return undefined
    }

    property color primary: _schemeColor("primary") ?? (isDark ? Qt.rgba(0.573, 0.800, 1.000, 1) : Qt.rgba(0.000, 0.392, 0.592, 1))
    property color onPrimary: _schemeColor("onPrimary") ?? (isDark ? Qt.rgba(0.000, 0.200, 0.318, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color primaryContainer: _schemeColor("primaryContainer") ?? (isDark ? Qt.rgba(0.000, 0.294, 0.451, 1) : Qt.rgba(0.800, 0.898, 1.000, 1))
    property color onPrimaryContainer: _schemeColor("onPrimaryContainer") ?? (isDark ? Qt.rgba(0.800, 0.898, 1.000, 1) : Qt.rgba(0.000, 0.118, 0.192, 1))
    property color secondary: _schemeColor("secondary") ?? (isDark ? Qt.rgba(0.722, 0.784, 0.855, 1) : Qt.rgba(0.318, 0.376, 0.435, 1))
    property color onSecondary: _schemeColor("onSecondary") ?? (isDark ? Qt.rgba(0.137, 0.196, 0.251, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color secondaryContainer: _schemeColor("secondaryContainer") ?? (isDark ? Qt.rgba(0.224, 0.282, 0.341, 1) : Qt.rgba(0.831, 0.894, 0.965, 1))
    property color onSecondaryContainer: _schemeColor("onSecondaryContainer") ?? (isDark ? Qt.rgba(0.831, 0.894, 0.965, 1) : Qt.rgba(0.055, 0.114, 0.165, 1))
    property color tertiary: _schemeColor("tertiary") ?? (isDark ? Qt.rgba(0.843, 0.749, 1.000, 1) : Qt.rgba(0.427, 0.341, 0.549, 1))
    property color onTertiary: _schemeColor("onTertiary") ?? (isDark ? Qt.rgba(0.243, 0.165, 0.361, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color tertiaryContainer: _schemeColor("tertiaryContainer") ?? (isDark ? Qt.rgba(0.333, 0.251, 0.455, 1) : Qt.rgba(0.945, 0.855, 1.000, 1))
    property color onTertiaryContainer: _schemeColor("onTertiaryContainer") ?? (isDark ? Qt.rgba(0.945, 0.855, 1.000, 1) : Qt.rgba(0.149, 0.078, 0.278, 1))
    property color background: _schemeColor("background") ?? (isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color onBackground: _schemeColor("onBackground") ?? (isDark ? Qt.rgba(0.886, 0.890, 0.906, 1) : Qt.rgba(0.094, 0.110, 0.125, 1))
    property color surface: _schemeColor("surface") ?? (isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color onSurface: _schemeColor("onSurface") ?? (isDark ? Qt.rgba(0.886, 0.890, 0.906, 1) : Qt.rgba(0.094, 0.110, 0.125, 1))
    property color surfaceVariant: _schemeColor("surfaceVariant") ?? (isDark ? Qt.rgba(0.259, 0.278, 0.306, 1) : Qt.rgba(0.875, 0.890, 0.922, 1))
    property color onSurfaceVariant: _schemeColor("onSurfaceVariant") ?? (isDark ? Qt.rgba(0.757, 0.776, 0.812, 1) : Qt.rgba(0.259, 0.278, 0.306, 1))
    property color surfaceTint: _schemeColor("surfaceTint") ?? primary
    property color surfaceDim: _schemeColor("surfaceDim") ?? (isDark ? Qt.rgba(0.071, 0.078, 0.094, 1) : Qt.rgba(0.843, 0.851, 0.875, 1))
    property color surfaceBright: _schemeColor("surfaceBright") ?? (isDark ? Qt.rgba(0.220, 0.224, 0.247, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color surfaceContainerLowest: _schemeColor("surfaceContainerLowest") ?? (isDark ? Qt.rgba(0.059, 0.067, 0.075, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color surfaceContainerLow: _schemeColor("surfaceContainerLow") ?? (isDark ? Qt.rgba(0.122, 0.133, 0.145, 1) : Qt.rgba(0.965, 0.973, 0.984, 1))
    property color surfaceContainer: _schemeColor("surfaceContainer") ?? (isDark ? Qt.rgba(0.137, 0.153, 0.165, 1) : Qt.rgba(0.941, 0.953, 0.969, 1))
    property color surfaceContainerHigh: _schemeColor("surfaceContainerHigh") ?? (isDark ? Qt.rgba(0.176, 0.192, 0.208, 1) : Qt.rgba(0.918, 0.937, 0.961, 1))
    property color surfaceContainerHighest: _schemeColor("surfaceContainerHighest") ?? (isDark ? Qt.rgba(0.220, 0.235, 0.251, 1) : Qt.rgba(0.894, 0.914, 0.937, 1))
    property color inverseSurface: _schemeColor("inverseSurface") ?? (isDark ? Qt.rgba(0.886, 0.886, 0.898, 1) : Qt.rgba(0.184, 0.188, 0.200, 1))
    property color inverseOnSurface: _schemeColor("inverseOnSurface") ?? (isDark ? Qt.rgba(0.184, 0.188, 0.200, 1) : Qt.rgba(0.945, 0.941, 0.957, 1))
    property color inversePrimary: _schemeColor("inversePrimary") ?? (isDark ? Qt.rgba(0.000, 0.392, 0.592, 1) : Qt.rgba(0.573, 0.800, 1.000, 1))
    property color error: _schemeColor("error") ?? (isDark ? Qt.rgba(1.000, 0.706, 0.671, 1) : Qt.rgba(0.729, 0.102, 0.102, 1))
    property color onError: _schemeColor("onError") ?? (isDark ? Qt.rgba(0.412, 0.000, 0.020, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color errorContainer: _schemeColor("errorContainer") ?? (isDark ? Qt.rgba(0.576, 0.000, 0.039, 1) : Qt.rgba(1.000, 0.855, 0.839, 1))
    property color onErrorContainer: _schemeColor("onErrorContainer") ?? (isDark ? Qt.rgba(1.000, 0.855, 0.839, 1) : Qt.rgba(0.255, 0.000, 0.008, 1))
    property color outline: _schemeColor("outline") ?? (isDark ? Qt.rgba(0.549, 0.569, 0.596, 1) : Qt.rgba(0.447, 0.471, 0.494, 1))
    property color outlineVariant: _schemeColor("outlineVariant") ?? (isDark ? Qt.rgba(0.259, 0.278, 0.306, 1) : Qt.rgba(0.757, 0.776, 0.812, 1))
    property color scrim: _schemeColor("scrim") ?? Qt.rgba(0.000, 0.000, 0.000, 1)

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
    property color dangerContainer: errorContainer
    property color onDangerContainer: onErrorContainer

    property color editorBackground: isDark ? Qt.rgba(0.125, 0.137, 0.149, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)
    property color editorText: textPrimary

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

    property var projectAccentColors: isDark
        ? ["#7B8CDE", "#DE8C7B", "#7BDE8C", "#DE7BC4", "#7BC4DE", "#C4DE7B"]
        : ["#5B6CAE", "#BE6C5B", "#5BBE6C", "#BE5BA4", "#5BA4BE", "#A4BE5B"]

    property string textPrimaryHex: isDark ? "#E2E2E5" : "#1A1C1E"
    property string textSecondaryHex: isDark ? "#C3C6CF" : "#42474E"

    property int radiusXs: 4
    property int radiusSm: 8
    property int radiusMd: 12
    property int radiusLg: 16
    property int radiusXl: 28
    property int radiusPill: 999
    property int radiusCard: radiusLg
    property int radiusPanel: radiusXl

    property int cardRadius: radiusLg
    property int dialogRadius: radiusXl
    property int fabRadius: radiusLg
    property int bottomBarRadius: 0
    property int inputFieldRadius: radiusMd

    property real elevation0: 0
    property real elevation1: 1
    property real elevation2: 3
    property real elevation3: 6

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

    property color controlBorder: border
    property color borderFocus: primary
    property color inputBg: surfaceContainerLow
    property color switchTrackOn: primary
    property color switchTrackOff: surfaceVariant
    property color switchThumb: isDark ? Qt.rgba(0.847, 0.937, 1.000, 1) : Qt.rgba(1.000, 1.000, 1.000, 1)

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

    property color shadowLight: isDark ? Qt.rgba(0, 0, 0, 0.25) : Qt.rgba(0, 0, 0, 0.06)
    property color shadowMedium: isDark ? Qt.rgba(0, 0, 0, 0.4) : Qt.rgba(0, 0, 0, 0.10)
    property color shadowDrawer: isDark ? Qt.rgba(0, 0, 0, 0.5) : Qt.rgba(0, 0, 0, 0.12)

    property int animFast: 120
    property int animNormal: 200
}
