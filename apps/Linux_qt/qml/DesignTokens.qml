import QtQuick

QtObject {
    id: dt

    // Issue #702: 主题完整状态只通过 themeStateJson 一次性发布。
    // JSON 结构：{"is_dark": bool, "scheme": <ThemeColorScheme object>}。
    // isDark 和 scheme 都从同一份 JSON 解析，彻底消除 isDark 已是 true
    // 但 scheme 还是上一套浅色值的中间状态。不再分开绑定 isDark 和
    // resolvedSchemeJson 两个可能不同步的属性。
    property string themeStateJson: ""

    property var _themeState: {
        if (themeStateJson.length === 0) return null
        try { return JSON.parse(themeStateJson) } catch(e) { return null }
    }

    // isDark 从同一份 themeStateJson 解析，保证与 scheme 同步。
    property bool isDark: _themeState !== null && _themeState.is_dark !== undefined ? _themeState.is_dark : true

    onIsDarkChanged: {
    }

    // Issue #702: scheme 直接从 themeStateJson 的 scheme 字段读取，
    // 与 isDark 来自同一份 JSON，不再有独立 resolvedSchemeJson 属性。
    // Issue #709 评论 issue-body-709: theme_state_json 现在输出完整状态，
    // 顶层包含 appearance_mode/is_dark/color_source/selected_builtin_theme_id/
    // selected_palette_id/scheme。scheme 为 null（无可用 scheme）时
    // _hasResolvedScheme 为 false，fallback 到 isDark 派生的固定深/浅色。
    // 之前 scheme 为空时是 {}（空对象），现在是 null，_hasResolvedScheme
    // 逻辑（_resolvedScheme !== null && ...）两种情况都正确 fallback。
    property var _resolvedScheme: _themeState !== null ? _themeState.scheme : null
    property bool _hasResolvedScheme: _resolvedScheme !== null && _resolvedScheme.primary !== undefined

    function _schemeColor(key) {
        if (_hasResolvedScheme) {
            var val = _resolvedScheme[key]
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

    // Issue #677 评论 5653315696: _schemeColor() 的 key 参数是 Core DTO 的 JSON 字段名，
    // 统一使用 snake_case（与 Core 的 ThemeColorScheme serde 序列化一致）。
    // QML 属性名（onSurface、surfaceContainerLow 等）保持 camelCase，符合 QML 惯例。
    property color primary: _schemeColor("primary") ?? (isDark ? Qt.rgba(0.573, 0.800, 1.000, 1) : Qt.rgba(0.000, 0.392, 0.592, 1))
    property color onPrimary: _schemeColor("on_primary") ?? (isDark ? Qt.rgba(0.000, 0.200, 0.318, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color primaryContainer: _schemeColor("primary_container") ?? (isDark ? Qt.rgba(0.000, 0.294, 0.451, 1) : Qt.rgba(0.800, 0.898, 1.000, 1))
    property color onPrimaryContainer: _schemeColor("on_primary_container") ?? (isDark ? Qt.rgba(0.800, 0.898, 1.000, 1) : Qt.rgba(0.000, 0.118, 0.192, 1))
    property color secondary: _schemeColor("secondary") ?? (isDark ? Qt.rgba(0.722, 0.784, 0.855, 1) : Qt.rgba(0.318, 0.376, 0.435, 1))
    property color onSecondary: _schemeColor("on_secondary") ?? (isDark ? Qt.rgba(0.137, 0.196, 0.251, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color secondaryContainer: _schemeColor("secondary_container") ?? (isDark ? Qt.rgba(0.224, 0.282, 0.341, 1) : Qt.rgba(0.831, 0.894, 0.965, 1))
    property color onSecondaryContainer: _schemeColor("on_secondary_container") ?? (isDark ? Qt.rgba(0.831, 0.894, 0.965, 1) : Qt.rgba(0.055, 0.114, 0.165, 1))
    property color tertiary: _schemeColor("tertiary") ?? (isDark ? Qt.rgba(0.843, 0.749, 1.000, 1) : Qt.rgba(0.427, 0.341, 0.549, 1))
    property color onTertiary: _schemeColor("on_tertiary") ?? (isDark ? Qt.rgba(0.243, 0.165, 0.361, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color tertiaryContainer: _schemeColor("tertiary_container") ?? (isDark ? Qt.rgba(0.333, 0.251, 0.455, 1) : Qt.rgba(0.945, 0.855, 1.000, 1))
    property color onTertiaryContainer: _schemeColor("on_tertiary_container") ?? (isDark ? Qt.rgba(0.945, 0.855, 1.000, 1) : Qt.rgba(0.149, 0.078, 0.278, 1))
    property color background: _schemeColor("background") ?? (isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color onBackground: _schemeColor("on_background") ?? (isDark ? Qt.rgba(0.886, 0.890, 0.906, 1) : Qt.rgba(0.094, 0.110, 0.125, 1))
    property color surface: _schemeColor("surface") ?? (isDark ? Qt.rgba(0.102, 0.110, 0.118, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color onSurface: _schemeColor("on_surface") ?? (isDark ? Qt.rgba(0.886, 0.890, 0.906, 1) : Qt.rgba(0.094, 0.110, 0.125, 1))
    property color surfaceVariant: _schemeColor("surface_variant") ?? (isDark ? Qt.rgba(0.259, 0.278, 0.306, 1) : Qt.rgba(0.875, 0.890, 0.922, 1))
    property color onSurfaceVariant: _schemeColor("on_surface_variant") ?? (isDark ? Qt.rgba(0.757, 0.776, 0.812, 1) : Qt.rgba(0.259, 0.278, 0.306, 1))
    property color surfaceTint: _schemeColor("surface_tint") ?? primary
    property color surfaceDim: _schemeColor("surface_dim") ?? (isDark ? Qt.rgba(0.071, 0.078, 0.094, 1) : Qt.rgba(0.843, 0.851, 0.875, 1))
    property color surfaceBright: _schemeColor("surface_bright") ?? (isDark ? Qt.rgba(0.220, 0.224, 0.247, 1) : Qt.rgba(0.988, 0.988, 1.000, 1))
    property color surfaceContainerLowest: _schemeColor("surface_container_lowest") ?? (isDark ? Qt.rgba(0.059, 0.067, 0.075, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color surfaceContainerLow: _schemeColor("surface_container_low") ?? (isDark ? Qt.rgba(0.122, 0.133, 0.145, 1) : Qt.rgba(0.965, 0.973, 0.984, 1))
    property color surfaceContainer: _schemeColor("surface_container") ?? (isDark ? Qt.rgba(0.137, 0.153, 0.165, 1) : Qt.rgba(0.941, 0.953, 0.969, 1))
    property color surfaceContainerHigh: _schemeColor("surface_container_high") ?? (isDark ? Qt.rgba(0.176, 0.192, 0.208, 1) : Qt.rgba(0.918, 0.937, 0.961, 1))
    property color surfaceContainerHighest: _schemeColor("surface_container_highest") ?? (isDark ? Qt.rgba(0.220, 0.235, 0.251, 1) : Qt.rgba(0.894, 0.914, 0.937, 1))
    property color inverseSurface: _schemeColor("inverse_surface") ?? (isDark ? Qt.rgba(0.886, 0.886, 0.898, 1) : Qt.rgba(0.184, 0.188, 0.200, 1))
    property color inverseOnSurface: _schemeColor("inverse_on_surface") ?? (isDark ? Qt.rgba(0.184, 0.188, 0.200, 1) : Qt.rgba(0.945, 0.941, 0.957, 1))
    property color inversePrimary: _schemeColor("inverse_primary") ?? (isDark ? Qt.rgba(0.000, 0.392, 0.592, 1) : Qt.rgba(0.573, 0.800, 1.000, 1))
    property color error: _schemeColor("error") ?? (isDark ? Qt.rgba(1.000, 0.706, 0.671, 1) : Qt.rgba(0.729, 0.102, 0.102, 1))
    property color onError: _schemeColor("on_error") ?? (isDark ? Qt.rgba(0.412, 0.000, 0.020, 1) : Qt.rgba(1.000, 1.000, 1.000, 1))
    property color errorContainer: _schemeColor("error_container") ?? (isDark ? Qt.rgba(0.576, 0.000, 0.039, 1) : Qt.rgba(1.000, 0.855, 0.839, 1))
    property color onErrorContainer: _schemeColor("on_error_container") ?? (isDark ? Qt.rgba(1.000, 0.855, 0.839, 1) : Qt.rgba(0.255, 0.000, 0.008, 1))
    property color outline: _schemeColor("outline") ?? (isDark ? Qt.rgba(0.549, 0.569, 0.596, 1) : Qt.rgba(0.447, 0.471, 0.494, 1))
    property color outlineVariant: _schemeColor("outline_variant") ?? (isDark ? Qt.rgba(0.259, 0.278, 0.306, 1) : Qt.rgba(0.757, 0.776, 0.812, 1))
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
    property color paper: surfaceContainerLow
    property color border: isDark ? Qt.rgba(outline.r, outline.g, outline.b, 0.42) : Qt.rgba(outline.r, outline.g, outline.b, 0.34)
    property color borderStrong: outline
    property color sidebar: surfaceContainer
    property color card: surfaceContainerLow
    property color cardHover: surfaceContainer
    property color selected: primaryContainer
    property color selectedText: onPrimaryContainer
    property color textPrimary: onSurface
    property color textSecondary: onSurfaceVariant
    property color textMuted: outline
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

    property color editorBackground: surfaceContainerLow
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
    // ── 逻辑像素字号（pixelSize，设备相关）──
    // 保留供非字体逻辑像素用途；字体应优先使用下面的 *Pt pointSize token。
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

    // ── High-DPI：字体 pointSize token（设备无关，跟随桌面 logical DPI / 字体缩放）──
    // Issue #692: Qt 6 High-DPI 模型下 font.pointSize 才是设备无关字号。
    // 按旧 pixelSize × 0.75 (72/96) 换算并微调，不简单把旧数字原样当 pt。
    // 旧 px → 新 pt:  28→21  24→18  18→13.5  14→10.5  13→10  12→9
    //                 11→8.5  12→9  14→10.5  16→12  18→13.5  22→16  26→19
    property real displayPt: 21       // 28px → 21pt
    property real titlePt: 18         // 24px → 18pt
    property real subtitlePt: 13.5    // 18px → 13.5pt
    property real bodyPt: 10.5        // 14px → 10.5pt
    property real labelPt: 10         // 13px → 9.75pt ≈ 10pt
    property real captionPt: 9        // 12px → 9pt
    property real fontXsPt: 8.5       // 11px → 8.25pt ≈ 8.5pt
    property real fontSmPt: 9         // 12px → 9pt
    property real fontMdPt: 10.5      // 14px → 10.5pt
    property real fontLgPt: 12        // 16px → 12pt
    property real fontXlPt: 13.5      // 18px → 13.5pt
    property real fontXxlPt: 16       // 22px → 16.5pt ≈ 16pt
    property real fontTitlePt: 19     // 26px → 19.5pt ≈ 19pt
    // 装饰性大字号（emoji / 大图标），旧 32/36/48 px
    property real fontEmojiSmPt: 24   // 32px → 24pt
    property real fontEmojiMdPt: 27   // 36px → 27pt
    property real fontEmojiLgPt: 36   // 48px → 36pt

    property color shadowLight: isDark ? Qt.rgba(0, 0, 0, 0.25) : Qt.rgba(0, 0, 0, 0.06)
    property color shadowMedium: isDark ? Qt.rgba(0, 0, 0, 0.4) : Qt.rgba(0, 0, 0, 0.10)
    property color shadowDrawer: isDark ? Qt.rgba(0, 0, 0, 0.5) : Qt.rgba(0, 0, 0, 0.12)

    property int animFast: 120
    property int animNormal: 200
}
