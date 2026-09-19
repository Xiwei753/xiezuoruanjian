import QtQuick

QtObject {
    id: dt

    // Issue #702: 主题完整状态只通过 themeStateJson 一次性发布。
    // JSON 结构：{"is_dark": bool, "scheme": <ThemeColorScheme object>}。
    // isDark 和 scheme 都从同一份 JSON 解析，彻底消除 isDark 已是 true
    // 但 scheme 还是上一套浅色值的中间状态。不再分开绑定 isDark 和
    // resolvedSchemeJson 两个可能不同步的属性。
    //
    // Issue #715: 颜色绑定改为从 _themeState.scheme (snake_case) 读取，
    // 不再直接读 themeController.xxx_hex。scheme 为 null 时使用 isDark
    // 派生的固定 fallback hex 字符串。所有颜色和 isDark 都从同一个
    // _themeState 对象派生，确保原子快照消费。
    property string themeStateJson: ""

    property var _themeState: {
        if (themeStateJson.length === 0) return null
        try { return JSON.parse(themeStateJson) } catch(e) { return null }
    }

    // Issue #715: isDark 从 _themeState.is_dark 读取，不再直接读 themeController.is_dark。
    property bool isDark: _themeState !== null ? _themeState.is_dark : true

    // Issue #715: scheme 对象从 _themeState.scheme 获取（snake_case 字段名）。
    // scheme 为 null 时所有颜色使用 isDark 派生的固定 fallback。
    readonly property var _scheme: _themeState !== null ? _themeState.scheme : null

    // 辅助函数：从 scheme 读取颜色，scheme 为 null 时返回 isDark fallback hex
    function _color(schemeField, darkFallback, lightFallback) {
        if (_scheme !== null && _scheme[schemeField] !== undefined && _scheme[schemeField] !== "") {
            return _scheme[schemeField]
        }
        return isDark ? darkFallback : lightFallback
    }

    // Issue #715: 所有颜色从 _themeState.scheme (snake_case) 读取，
    // scheme 为 null 时 fallback 到 isDark 派生的固定深/浅色 hex。
    property color primary: _color("primary", "#92CCFF", "#006497")
    property color onPrimary: _color("on_primary", "#003351", "#FFFFFF")
    property color primaryContainer: _color("primary_container", "#004B73", "#CCE5FF")
    property color onPrimaryContainer: _color("on_primary_container", "#CCE5FF", "#001E31")
    property color secondary: _color("secondary", "#B8C8DA", "#51606F")
    property color onSecondary: _color("on_secondary", "#233240", "#FFFFFF")
    property color secondaryContainer: _color("secondary_container", "#394857", "#D4E4F6")
    property color onSecondaryContainer: _color("on_secondary_container", "#D4E4F6", "#0E1D2A")
    property color tertiary: _color("tertiary", "#D7BFFF", "#6D578C")
    property color onTertiary: _color("on_tertiary", "#3E2A5C", "#FFFFFF")
    property color tertiaryContainer: _color("tertiary_container", "#554074", "#F1DAFF")
    property color onTertiaryContainer: _color("on_tertiary_container", "#F1DAFF", "#261447")
    property color background: _color("background", "#1A1C1E", "#FCFCFF")
    property color onBackground: _color("on_background", "#E2E3E7", "#181C20")
    property color surface: _color("surface", "#1A1C1E", "#FCFCFF")
    property color onSurface: _color("on_surface", "#E2E3E7", "#181C20")
    property color surfaceVariant: _color("surface_variant", "#42474E", "#DFE3EB")
    property color onSurfaceVariant: _color("on_surface_variant", "#C1C6CF", "#42474E")
    property color surfaceTint: _color("surface_tint", "#92CCFF", "#006497")
    property color surfaceDim: _color("surface_dim", "#121418", "#D7D9DF")
    property color surfaceBright: _color("surface_bright", "#38393F", "#FCFCFF")
    property color surfaceContainerLowest: _color("surface_container_lowest", "#0F1113", "#FFFFFF")
    property color surfaceContainerLow: _color("surface_container_low", "#1F2225", "#F6F8FB")
    property color surfaceContainer: _color("surface_container", "#23272A", "#F0F3F7")
    property color surfaceContainerHigh: _color("surface_container_high", "#2D3135", "#EAEFF5")
    property color surfaceContainerHighest: _color("surface_container_highest", "#383C40", "#E4E9EF")
    property color inverseSurface: _color("inverse_surface", "#E2E2E5", "#2F3033")
    property color inverseOnSurface: _color("inverse_on_surface", "#2F3033", "#F1F0F4")
    property color inversePrimary: _color("inverse_primary", "#006497", "#92CCFF")
    property color error: _color("error", "#FFB4AB", "#BA1A1A")
    property color onError: _color("on_error", "#690005", "#FFFFFF")
    property color errorContainer: _color("error_container", "#93000A", "#FFDAD6")
    property color onErrorContainer: _color("on_error_container", "#FFDAD6", "#410002")
    property color outline: _color("outline", "#8C9198", "#72787E")
    property color outlineVariant: _color("outline_variant", "#42474E", "#C1C6CF")
    property color scrim: _color("scrim", "#000000", "#000000")

    // 派生色：没有直接对应的 scheme 字段，继续用 isDark 派生
    // Issue #715: isDark 本身从 _themeState.is_dark 读取
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

    // 组合色：基于绑定后的基础色计算
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
