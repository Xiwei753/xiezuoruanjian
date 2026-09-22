import QtQuick

QtObject {
    id: dt

    // Issue #736 评论 5777408243 问题2: themeApplied 信号。
    // applyThemeState() 完成 resolvedTheme 整体替换后发出，携带这份已发布的最终 token snapshot。
    // 外部（main.qml 诊断）监听此信号而非原始 themeStateJson 变化。
    signal themeApplied

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
        // Issue #736 评论 5777408243 问题2: 初始 fallback 也带语义色字段，
        // 与 applyThemeState 构造的 next 结构一致。
        text_primary: "#E2E3E7",
        text_secondary: "#C1C6CF",
        editor_text: "#E2E3E7",
        editor_background: "#1F2225",
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
        // Issue #727 评论 5755858583 问题3: Rust 侧总是生成 colors（即使 scheme
        // 为 None 也根据 is_dark 生成 fallback colors），QML 侧直接读取不再 fallback。
        var colors = (parsed.colors && typeof parsed.colors === "object") ? parsed.colors : null

        // colors 为 null 是防御性情况（不应该发生），保持当前 resolvedTheme 不变
        if (!colors) {
            resolvedTheme = Object.assign({}, resolvedTheme, { is_dark: dark })
            return
        }

        // Issue #727 评论 5760020833 问题3: 去掉 QML 侧 colors.xxx || prev.xxx 静默 fallback。
        // Rust 侧已保证 colors 包含所有字段的最终值，直接读取即可。
        // 字段缺失/非法（undefined 或非 string）由 qcolor() 报成洋红，
        // 不再悄悄继承上一套主题颜色。
        //
        // Issue #736 评论 5777408243 问题2: 一次构造完整的"最终 QML token 快照"再整体替换。
        // 在 next 里直接确定基础色 + 语义色（text_primary/text_secondary/editor_text/editor_background），
        // 公开属性只读这一个 next，不再一级套一级计算（不再 textPrimary: onSurface → onSurface: qcolor(...)）。
        var next = {
            is_dark: dark,
            colors: colors,
            primary: colors.primary,
            on_primary: colors.on_primary,
            primary_container: colors.primary_container,
            on_primary_container: colors.on_primary_container,
            secondary: colors.secondary,
            on_secondary: colors.on_secondary,
            secondary_container: colors.secondary_container,
            on_secondary_container: colors.on_secondary_container,
            tertiary: colors.tertiary,
            on_tertiary: colors.on_tertiary,
            tertiary_container: colors.tertiary_container,
            on_tertiary_container: colors.on_tertiary_container,
            background: colors.background,
            on_background: colors.on_background,
            surface: colors.surface,
            on_surface: colors.on_surface,
            surface_variant: colors.surface_variant,
            on_surface_variant: colors.on_surface_variant,
            surface_tint: colors.surface_tint,
            surface_dim: colors.surface_dim,
            surface_bright: colors.surface_bright,
            surface_container_lowest: colors.surface_container_lowest,
            surface_container_low: colors.surface_container_low,
            surface_container: colors.surface_container,
            surface_container_high: colors.surface_container_high,
            surface_container_highest: colors.surface_container_highest,
            inverse_surface: colors.inverse_surface,
            inverse_on_surface: colors.inverse_on_surface,
            inverse_primary: colors.inverse_primary,
            error: colors.error,
            on_error: colors.on_error,
            error_container: colors.error_container,
            on_error_container: colors.on_error_container,
            outline: colors.outline,
            outline_variant: colors.outline_variant,
            scrim: colors.scrim,
            // Issue #736 评论 5777408243 问题2: 语义色在构造 next 时一次确定，
            // 不再让属性链 textPrimary → onSurface → qcolor(resolvedTheme.on_surface) 多层派生。
            text_primary: colors.on_surface,
            text_secondary: colors.on_surface_variant,
            editor_text: colors.on_surface,
            editor_background: colors.surface_container_low,
        }
        resolvedTheme = next
        // Issue #736 评论 5777408243 问题2: resolvedTheme 替换后发 themeApplied 信号，
        // 让外部（main.qml 诊断）监听这份已发布的最终 token snapshot，不再监听原始 JSON 变化。
        themeApplied()
    }

    // Issue #724 评论 5751573705 问题3: 所有派生 token 只读 resolvedTheme，
    // 不再独立 binding 到 themeControllerRef.*_hex。
    // applyThemeState() 整体替换 resolvedTheme 后 QML 统一更新。
    // Issue #727 评论 5757225958 问题4: 唯一颜色边界 qcolor(value)。
    // 所有最终颜色都经此函数转换，异常值用明显错误色（magenta #FF00FF），
    // 不悄悄回退上一份颜色。Rust 侧 #DFE3E7 到 QML 仍是 #DFE3E7 而非 #000000。
    function qcolor(value) {
        if (typeof value !== "string" || value.length === 0) {
            return Qt.rgba(1.0, 0.0, 1.0, 1.0) // magenta 表示异常
        }
        // Qt.color 非法颜色返回 null（不抛异常），用 null 检查 fallback 到洋红。
        const c = Qt.color(value)
        return c === null ? Qt.rgba(1.0, 0.0, 1.0, 1.0) : c
    }

    property bool isDark: resolvedTheme.is_dark
    property color primary: qcolor(resolvedTheme.primary)
    property color onPrimary: qcolor(resolvedTheme.on_primary)
    property color primaryContainer: qcolor(resolvedTheme.primary_container)
    property color onPrimaryContainer: qcolor(resolvedTheme.on_primary_container)
    property color secondary: qcolor(resolvedTheme.secondary)
    property color onSecondary: qcolor(resolvedTheme.on_secondary)
    property color secondaryContainer: qcolor(resolvedTheme.secondary_container)
    property color onSecondaryContainer: qcolor(resolvedTheme.on_secondary_container)
    property color tertiary: qcolor(resolvedTheme.tertiary)
    property color onTertiary: qcolor(resolvedTheme.on_tertiary)
    property color tertiaryContainer: qcolor(resolvedTheme.tertiary_container)
    property color onTertiaryContainer: qcolor(resolvedTheme.on_tertiary_container)
    property color background: qcolor(resolvedTheme.background)
    property color onBackground: qcolor(resolvedTheme.on_background)
    property color surface: qcolor(resolvedTheme.surface)
    property color onSurface: qcolor(resolvedTheme.on_surface)
    property color surfaceVariant: qcolor(resolvedTheme.surface_variant)
    property color onSurfaceVariant: qcolor(resolvedTheme.on_surface_variant)
    property color surfaceTint: qcolor(resolvedTheme.surface_tint)
    property color surfaceDim: qcolor(resolvedTheme.surface_dim)
    property color surfaceBright: qcolor(resolvedTheme.surface_bright)
    property color surfaceContainerLowest: qcolor(resolvedTheme.surface_container_lowest)
    property color surfaceContainerLow: qcolor(resolvedTheme.surface_container_low)
    property color surfaceContainer: qcolor(resolvedTheme.surface_container)
    property color surfaceContainerHigh: qcolor(resolvedTheme.surface_container_high)
    property color surfaceContainerHighest: qcolor(resolvedTheme.surface_container_highest)
    property color inverseSurface: qcolor(resolvedTheme.inverse_surface)
    property color inverseOnSurface: qcolor(resolvedTheme.inverse_on_surface)
    property color inversePrimary: qcolor(resolvedTheme.inverse_primary)
    property color error: qcolor(resolvedTheme.error)
    property color onError: qcolor(resolvedTheme.on_error)
    property color errorContainer: qcolor(resolvedTheme.error_container)
    property color onErrorContainer: qcolor(resolvedTheme.on_error_container)
    property color outline: qcolor(resolvedTheme.outline)
    property color outlineVariant: qcolor(resolvedTheme.outline_variant)
    property color scrim: qcolor(resolvedTheme.scrim)

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
    // Issue #736 评论 5777408243 问题2: textPrimary/textSecondary/editorText/editorBackground
    // 直接读 resolvedTheme 里的语义色字段，不再 textPrimary: onSurface → onSurface: qcolor(...) 多层派生。
    property color textPrimary: qcolor(resolvedTheme.text_primary)
    property color textSecondary: qcolor(resolvedTheme.text_secondary)
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

    property color editorBackground: qcolor(resolvedTheme.editor_background)
    property color editorText: qcolor(resolvedTheme.editor_text)

    // Issue #736 评论 5777408243 问题2: 原始 hex 字符串属性，给 Rust/Q_PROPERTY 字符串消费者直接用，
    // 避免 QString -> QML color -> toString() -> QString 绕一圈。
    // 值直接从 resolvedTheme（最终 token snapshot）读取，不再经多层 color 属性派生。
    property string onSurfaceHex: resolvedTheme.on_surface || ""
    property string onSurfaceVariantHex: resolvedTheme.on_surface_variant || ""
    property string textPrimaryHex: resolvedTheme.text_primary || resolvedTheme.on_surface || ""
    property string textSecondaryHex: resolvedTheme.text_secondary || resolvedTheme.on_surface_variant || ""
    property string editorTextHex: resolvedTheme.editor_text || resolvedTheme.on_surface || ""
    property string editorBackgroundHex: resolvedTheme.editor_background || resolvedTheme.surface_container_low || ""
    property string primaryHex: resolvedTheme.primary || ""
    property string selectedTextHex: resolvedTheme.on_primary_container || ""

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
