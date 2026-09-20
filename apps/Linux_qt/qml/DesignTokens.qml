import QtQuick

QtObject {
    id: dt

    // Issue #724 评论 5751268664 缺口3: DesignTokens 真正只消费 theme_state_json。
    //
    // 旧实现（Issue #721）让每个颜色属性独立 binding 到 themeControllerRef.*_hex，
    // themeStateJson 只做诊断。这导致 themeStateJsonChanged 与独立颜色 binding 可能不同步，
    // 同一帧内 isDark 已切到 dark 但 scheme 还是上一套浅色值的中间状态仍可能出现。
    //
    // 新实现：所有颜色属性初始值为 fallback，onThemeStateJsonChanged 一次性解析
    // theme_state_json JSON，从同一份不可分割快照更新 isDark 和全部颜色属性。
    // 不再让每个颜色属性独立 binding 到 themeControllerRef.*_hex。
    // 条件 reload（reload_from_backend_if_changed）保留在 Rust 侧，不影响此处原子发布。
    property string themeStateJson: ""

    property var themeControllerRef: null

    // Issue #724 评论 5751268664 缺口3: isDark 初始值为 true（fallback），
    // 由 applyThemeState() 一次性更新，不再独立 binding 到 themeControllerRef.is_dark。
    property bool isDark: true

    // Issue #724 评论 5751268664 缺口3: 所有颜色属性初始值为深色 fallback，
    // 由 applyThemeState() 一次性更新，不再独立 binding 到 themeControllerRef.*_hex。
    property color primary: "#92CCFF"
    property color onPrimary: "#003351"
    property color primaryContainer: "#004B73"
    property color onPrimaryContainer: "#CCE5FF"
    property color secondary: "#B8C8DA"
    property color onSecondary: "#233240"
    property color secondaryContainer: "#394857"
    property color onSecondaryContainer: "#D4E4F6"
    property color tertiary: "#D7BFFF"
    property color onTertiary: "#3E2A5C"
    property color tertiaryContainer: "#554074"
    property color onTertiaryContainer: "#F1DAFF"
    property color background: "#1A1C1E"
    property color onBackground: "#E2E3E7"
    property color surface: "#1A1C1E"
    property color onSurface: "#E2E3E7"
    property color surfaceVariant: "#42474E"
    property color onSurfaceVariant: "#C1C6CF"
    property color surfaceTint: "#92CCFF"
    property color surfaceDim: "#121418"
    property color surfaceBright: "#38393F"
    property color surfaceContainerLowest: "#0F1113"
    property color surfaceContainerLow: "#1F2225"
    property color surfaceContainer: "#23272A"
    property color surfaceContainerHigh: "#2D3135"
    property color surfaceContainerHighest: "#383C40"
    property color inverseSurface: "#E2E2E5"
    property color inverseOnSurface: "#2F3033"
    property color inversePrimary: "#006497"
    property color error: "#FFB4AB"
    property color onError: "#690005"
    property color errorContainer: "#93000A"
    property color onErrorContainer: "#FFDAD6"
    property color outline: "#8C9198"
    property color outlineVariant: "#42474E"
    property color scrim: "#000000"

    // Issue #724 评论 5751268664 缺口3: 从 theme_state_json 一次性解析并更新所有颜色。
    // theme_state_json 结构：{"is_dark": bool, "scheme": {primary, on_primary, ...} | null}
    // scheme 为 null 时用 isDark 派生的固定 fallback。
    // 所有颜色属性在此函数内一次性赋值，QML 引擎在下一帧统一更新所有绑定，
    // 不在帧内暴露 light/dark 混合中间状态。
    function applyThemeState(jsonStr) {
        if (!jsonStr || jsonStr.length === 0) {
            // 空状态：保持 fallback（初始深色值）
            return
        }
        var parsed
        try {
            parsed = JSON.parse(jsonStr)
        } catch (e) {
            // 解析失败：保持上一份状态，不暴露部分更新中间态
            return
        }
        if (!parsed || typeof parsed !== "object") {
            return
        }
        var dark = !!parsed.is_dark
        isDark = dark
        var scheme = (parsed.scheme && typeof parsed.scheme === "object") ? parsed.scheme : null
        // 辅助：从 scheme 取 hex 字符串，空则用 isDark fallback
        function hex(field, darkVal, lightVal) {
            if (scheme && scheme[field] && typeof scheme[field] === "string" && scheme[field].length > 0) {
                return scheme[field]
            }
            return dark ? darkVal : lightVal
        }
        primary = hex("primary", "#92CCFF", "#006497")
        onPrimary = hex("on_primary", "#003351", "#FFFFFF")
        primaryContainer = hex("primary_container", "#004B73", "#CCE5FF")
        onPrimaryContainer = hex("on_primary_container", "#CCE5FF", "#001E31")
        secondary = hex("secondary", "#B8C8DA", "#51606F")
        onSecondary = hex("on_secondary", "#233240", "#FFFFFF")
        secondaryContainer = hex("secondary_container", "#394857", "#D4E4F6")
        onSecondaryContainer = hex("on_secondary_container", "#D4E4F6", "#0E1D2A")
        tertiary = hex("tertiary", "#D7BFFF", "#6D578C")
        onTertiary = hex("on_tertiary", "#3E2A5C", "#FFFFFF")
        tertiaryContainer = hex("tertiary_container", "#554074", "#F1DAFF")
        onTertiaryContainer = hex("on_tertiary_container", "#F1DAFF", "#261447")
        background = hex("background", "#1A1C1E", "#FCFCFF")
        onBackground = hex("on_background", "#E2E3E7", "#181C20")
        surface = hex("surface", "#1A1C1E", "#FCFCFF")
        onSurface = hex("on_surface", "#E2E3E7", "#181C20")
        surfaceVariant = hex("surface_variant", "#42474E", "#DFE3EB")
        onSurfaceVariant = hex("on_surface_variant", "#C1C6CF", "#42474E")
        surfaceTint = hex("surface_tint", "#92CCFF", "#006497")
        surfaceDim = hex("surface_dim", "#121418", "#D7D9DF")
        surfaceBright = hex("surface_bright", "#38393F", "#FCFCFF")
        surfaceContainerLowest = hex("surface_container_lowest", "#0F1113", "#FFFFFF")
        surfaceContainerLow = hex("surface_container_low", "#1F2225", "#F6F8FB")
        surfaceContainer = hex("surface_container", "#23272A", "#F0F3F7")
        surfaceContainerHigh = hex("surface_container_high", "#2D3135", "#EAEFF5")
        surfaceContainerHighest = hex("surface_container_highest", "#383C40", "#E4E9EF")
        inverseSurface = hex("inverse_surface", "#E2E2E5", "#2F3033")
        inverseOnSurface = hex("inverse_on_surface", "#2F3033", "#F1F0F4")
        inversePrimary = hex("inverse_primary", "#006497", "#92CCFF")
        error = hex("error", "#FFB4AB", "#BA1A1A")
        onError = hex("on_error", "#690005", "#FFFFFF")
        errorContainer = hex("error_container", "#93000A", "#FFDAD6")
        onErrorContainer = hex("on_error_container", "#FFDAD6", "#410002")
        outline = hex("outline", "#8C9198", "#72787E")
        outlineVariant = hex("outline_variant", "#42474E", "#C1C6CF")
        scrim = hex("scrim", "#000000", "#000000")
    }

    // Issue #724 评论 5751268664 缺口3: themeStateJson 变化时一次性解析并更新所有颜色。
    // 这是唯一的颜色更新入口，不再有独立 binding 到 themeControllerRef.*_hex。
    onThemeStateJsonChanged: applyThemeState(themeStateJson)

    // Issue #724 评论 5751268664 缺口3: themeControllerRef 设置时也触发一次解析，
    // 覆盖 main.qml 中 themeControllerRef 先于 themeStateJson 绑定的初始化顺序。
    onThemeControllerRefChanged: applyThemeState(themeStateJson)

    // Issue #724 评论 5751268664 缺口3: Component.onCompleted 时应用一次当前状态，
    // 确保初始颜色来自 theme_state_json 而非硬编码 fallback。
    Component.onCompleted: applyThemeState(themeStateJson)

    // 派生色：没有直接对应的 scheme 字段，继续用 isDark 派生
    // Issue #721: isDark 直接读 themeController.is_dark
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
