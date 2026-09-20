import QtQuick

QtObject {
    id: dt

    // Issue #724 评论 5751573705 问题3: DesignTokens 真正原子替换。
    //
    // 旧实现让每个颜色属性独立 binding 到 themeControllerRef.*_hex，
    // applyThemeState() 逐个写 property，导致同一帧内 isDark 已切到 dark 但 scheme
    // 还是上一套浅色值的混合中间态仍可能出现。
    //
    // 新实现：
    // - `resolvedTheme` 是完整的 theme 对象，所有派生 token 只读此对象。
    // - applyThemeState() 一次性构建新 theme 对象并整体替换 resolvedTheme，
    //   QML 引擎在下一帧统一更新所有绑定，不在帧内暴露混合中间态。
    // - 派生色（success/warning/info 等）继续从 resolvedTheme 派生。
    // - 组合色（bg/paper/border/editorText 等）继续从 resolvedTheme 派生。

    /// theme_state_json 字符串（来自 Rust 侧）。
    property string themeStateJson: ""

    property var themeControllerRef: null

    /// 完整的 resolved theme 对象。所有派生 token 只读此属性。
    /// 结构：{ is_dark: bool, scheme: { primary, on_primary, ... } | null }
    /// scheme 为 null 时用 isDark 派生固定 fallback。
    property var resolvedTheme: ({
        is_dark: true,
        scheme: null,
        primary: "#92CCFF",
        on_primary: "#003351",
        primary_container: "#004B73",
        on_primary_container: "#CCE5FF",
        secondary: "#B8C8DA",
        on_secondary: "#233240",
        secondary_container: "#394857",
        on_secondary_container: "#D4E4F6",
        tertiary: "#D7BFFF",
        on_tertiary: "#3E2A5C",
        tertiary_container: "#554074",
        on_tertiary_container: "#F1DAFF",
        background: "#1A1C1E",
        on_background: "#E2E3E7",
        surface: "#1A1C1E",
        on_surface: "#E2E3E7",
        surface_variant: "#42474E",
        on_surface_variant: "#C1C6CF",
        surface_tint: "#92CCFF",
        surface_dim: "#121418",
        surface_bright: "#38393F",
        surface_container_lowest: "#0F1113",
        surface_container_low: "#1F2225",
        surface_container: "#23272A",
        surface_container_high: "#2D3135",
        surface_container_highest: "#383C40",
        inverse_surface: "#E2E2E5",
        inverse_on_surface: "#2F3033",
        inverse_primary: "#006497",
        error: "#FFB4AB",
        on_error: "#690005",
        error_container: "#93000A",
        on_error_container: "#FFDAD6",
        outline: "#8C9198",
        outline_variant: "#42474E",
        scrim: "#000000",
    })

    /// 从 theme_state_json 一次性构建 resolved theme 对象。
    /// 不再逐个写 property——整体替换 resolvedTheme 后 QML 统一更新所有绑定。
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
        var scheme = (parsed.scheme && typeof parsed.scheme === "object") ? parsed.scheme : null

        // 辅助：从 scheme 取 hex 字符串，空则用 isDark fallback
        function hex(field, darkVal, lightVal) {
            if (scheme && scheme[field] && typeof scheme[field] === "string" && scheme[field].length > 0) {
                return scheme[field]
            }
            return dark ? darkVal : lightVal
        }

        // 一次性构建完整 theme 对象，然后整体替换 resolvedTheme。
        // QML 引擎在下一帧统一更新所有绑定，不在帧内暴露混合中间态。
        var next = {
            is_dark: dark,
            scheme: scheme,
            primary: hex("primary", "#92CCFF", "#006497"),
            on_primary: hex("on_primary", "#003351", "#FFFFFF"),
            primary_container: hex("primary_container", "#004B73", "#CCE5FF"),
            on_primary_container: hex("on_primary_container", "#CCE5FF", "#001E31"),
            secondary: hex("secondary", "#B8C8DA", "#51606F"),
            on_secondary: hex("on_secondary", "#233240", "#FFFFFF"),
            secondary_container: hex("secondary_container", "#394857", "#D4E4F6"),
            on_secondary_container: hex("on_secondary_container", "#D4E4F6", "#0E1D2A"),
            tertiary: hex("tertiary", "#D7BFFF", "#6D578C"),
            on_tertiary: hex("on_tertiary", "#3E2A5C", "#FFFFFF"),
            tertiary_container: hex("tertiary_container", "#554074", "#F1DAFF"),
            on_tertiary_container: hex("on_tertiary_container", "#F1DAFF", "#261447"),
            background: hex("background", "#1A1C1E", "#FCFCFF"),
            on_background: hex("on_background", "#E2E3E7", "#181C20"),
            surface: hex("surface", "#1A1C1E", "#FCFCFF"),
            on_surface: hex("on_surface", "#E2E3E7", "#181C20"),
            surface_variant: hex("surface_variant", "#42474E", "#DFE3EB"),
            on_surface_variant: hex("on_surface_variant", "#C1C6CF", "#42474E"),
            surface_tint: hex("surface_tint", "#92CCFF", "#006497"),
            surface_dim: hex("surface_dim", "#121418", "#D7D9DF"),
            surface_bright: hex("surface_bright", "#38393F", "#FCFCFF"),
            surface_container_lowest: hex("surface_container_lowest", "#0F1113", "#FFFFFF"),
            surface_container_low: hex("surface_container_low", "#1F2225", "#F6F8FB"),
            surface_container: hex("surface_container", "#23272A", "#F0F3F7"),
            surface_container_high: hex("surface_container_high", "#2D3135", "#EAEFF5"),
            surface_container_highest: hex("surface_container_highest", "#383C40", "#E4E9EF"),
            inverse_surface: hex("inverse_surface", "#E2E2E5", "#2F3033"),
            inverse_on_surface: hex("inverse_on_surface", "#2F3033", "#F1F0F4"),
            inverse_primary: hex("inverse_primary", "#006497", "#92CCFF"),
            error: hex("error", "#FFB4AB", "#BA1A1A"),
            on_error: hex("on_error", "#690005", "#FFFFFF"),
            error_container: hex("error_container", "#93000A", "#FFDAD6"),
            on_error_container: hex("on_error_container", "#FFDAD6", "#410002"),
            outline: hex("outline", "#8C9198", "#72787E"),
            outline_variant: hex("outline_variant", "#42474E", "#C1C6CF"),
            scrim: hex("scrim", "#000000", "#000000"),
        }
        resolvedTheme = next
    }

    // Issue #724 评论 5751573705 问题3: 所有派生 token 只读 resolvedTheme，
    // 不再独立 binding 到 themeControllerRef.*_hex。
    // applyThemeState() 整体替换 resolvedTheme 后 QML 统一更新。
    property bool isDark: resolvedTheme.is_dark
    property color primary: resolvedTheme.primary
    property color onPrimary: resolvedTheme.on_primary
    property color primaryContainer: resolvedTheme.primary_container
    property color onPrimaryContainer: resolvedTheme.on_primary_container
    property color secondary: resolvedTheme.secondary
    property color onSecondary: resolvedTheme.on_secondary
    property color secondaryContainer: resolvedTheme.secondary_container
    property color onSecondaryContainer: resolvedTheme.on_secondary_container
    property color tertiary: resolvedTheme.tertiary
    property color onTertiary: resolvedTheme.on_tertiary
    property color tertiaryContainer: resolvedTheme.tertiary_container
    property color onTertiaryContainer: resolvedTheme.on_tertiary_container
    property color background: resolvedTheme.background
    property color onBackground: resolvedTheme.on_background
    property color surface: resolvedTheme.surface
    property color onSurface: resolvedTheme.on_surface
    property color surfaceVariant: resolvedTheme.surface_variant
    property color onSurfaceVariant: resolvedTheme.on_surface_variant
    property color surfaceTint: resolvedTheme.surface_tint
    property color surfaceDim: resolvedTheme.surface_dim
    property color surfaceBright: resolvedTheme.surface_bright
    property color surfaceContainerLowest: resolvedTheme.surface_container_lowest
    property color surfaceContainerLow: resolvedTheme.surface_container_low
    property color surfaceContainer: resolvedTheme.surface_container
    property color surfaceContainerHigh: resolvedTheme.surface_container_high
    property color surfaceContainerHighest: resolvedTheme.surface_container_highest
    property color inverseSurface: resolvedTheme.inverse_surface
    property color inverseOnSurface: resolvedTheme.inverse_on_surface
    property color inversePrimary: resolvedTheme.inverse_primary
    property color error: resolvedTheme.error
    property color onError: resolvedTheme.on_error
    property color errorContainer: resolvedTheme.error_container
    property color onErrorContainer: resolvedTheme.on_error_container
    property color outline: resolvedTheme.outline
    property color outlineVariant: resolvedTheme.outline_variant
    property color scrim: resolvedTheme.scrim

    // 派生色：没有直接对应的 scheme 字段，继续用 isDark 派生
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

    // Issue #724 评论 5751573705 问题3: themeStateJson 变化时一次性解析并整体替换 resolvedTheme。
    // 这是唯一的颜色更新入口，不再有独立 binding 到 themeControllerRef.*_hex。
    onThemeStateJsonChanged: applyThemeState(themeStateJson)

    // Issue #724 评论 5751573705 问题3: themeControllerRef 设置时也触发一次解析，
    // 覆盖 main.qml 中 themeControllerRef 先于 themeStateJson 绑定的初始化顺序。
    onThemeControllerRefChanged: applyThemeState(themeStateJson)

    // Issue #724 评论 5751573705 问题3: Component.onCompleted 时应用一次当前状态，
    // 确保初始颜色来自 theme_state_json 而非硬编码 fallback。
    Component.onCompleted: applyThemeState(themeStateJson)
}
