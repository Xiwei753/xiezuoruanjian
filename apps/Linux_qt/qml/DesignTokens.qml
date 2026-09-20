import QtQuick

QtObject {
    id: dt

    // Issue #702: 主题完整状态曾只通过 themeStateJson 一次性发布。
    // Issue #721: 颜色改为直接读 themeControllerRef.*_hex (QString "#RRGGBB")，
    // 不再从 themeStateJson 解析颜色。themeControllerRef 为 null 时 fallback 到
    // isDark 派生的固定 hex 字符串。isDark 也直接读 themeControllerRef.is_dark。
    // themeStateJson 属性保留做诊断（main.qml 仍会绑定并监听变化打日志），
    // 但不再参与颜色计算。
    // Issue #721 评论 5747140241: 属性名用 themeControllerRef 而非 themeController，
    // 避免 main.qml 里 `DesignTokens { themeController: themeController }` 同名自撞
    // （QML 绑定作用域就是接收对象自身，右侧裸 themeController 会遮蔽 QQmlContext
    // 注入的 themeController，导致自绑定保持 null，颜色链全走 fallback）。
    property string themeStateJson: ""

    property var themeControllerRef: null

    // Issue #721: isDark 直接读 themeControllerRef.is_dark，不再从 themeStateJson 解析。
    property bool isDark: themeControllerRef ? themeControllerRef.is_dark : true

    // Issue #721: 所有颜色直接读 themeControllerRef.*_hex (QString "#RRGGBB")，
    // themeControllerRef 为 null 时 fallback 到 isDark 派生的固定深/浅色 hex。
    // 消除 themeStateJson -> JSON.parse -> _themeState.scheme 的中间解析路径，
    // QString → QML color 单一链。
    property color primary: themeControllerRef ? themeControllerRef.primary_hex : (isDark ? "#92CCFF" : "#006497")
    property color onPrimary: themeControllerRef ? themeControllerRef.on_primary_hex : (isDark ? "#003351" : "#FFFFFF")
    property color primaryContainer: themeControllerRef ? themeControllerRef.primary_container_hex : (isDark ? "#004B73" : "#CCE5FF")
    property color onPrimaryContainer: themeControllerRef ? themeControllerRef.on_primary_container_hex : (isDark ? "#CCE5FF" : "#001E31")
    property color secondary: themeControllerRef ? themeControllerRef.secondary_hex : (isDark ? "#B8C8DA" : "#51606F")
    property color onSecondary: themeControllerRef ? themeControllerRef.on_secondary_hex : (isDark ? "#233240" : "#FFFFFF")
    property color secondaryContainer: themeControllerRef ? themeControllerRef.secondary_container_hex : (isDark ? "#394857" : "#D4E4F6")
    property color onSecondaryContainer: themeControllerRef ? themeControllerRef.on_secondary_container_hex : (isDark ? "#D4E4F6" : "#0E1D2A")
    property color tertiary: themeControllerRef ? themeControllerRef.tertiary_hex : (isDark ? "#D7BFFF" : "#6D578C")
    property color onTertiary: themeControllerRef ? themeControllerRef.on_tertiary_hex : (isDark ? "#3E2A5C" : "#FFFFFF")
    property color tertiaryContainer: themeControllerRef ? themeControllerRef.tertiary_container_hex : (isDark ? "#554074" : "#F1DAFF")
    property color onTertiaryContainer: themeControllerRef ? themeControllerRef.on_tertiary_container_hex : (isDark ? "#F1DAFF" : "#261447")
    property color background: themeControllerRef ? themeControllerRef.background_hex : (isDark ? "#1A1C1E" : "#FCFCFF")
    property color onBackground: themeControllerRef ? themeControllerRef.on_background_hex : (isDark ? "#E2E3E7" : "#181C20")
    property color surface: themeControllerRef ? themeControllerRef.surface_hex : (isDark ? "#1A1C1E" : "#FCFCFF")
    property color onSurface: themeControllerRef ? themeControllerRef.on_surface_hex : (isDark ? "#E2E3E7" : "#181C20")
    property color surfaceVariant: themeControllerRef ? themeControllerRef.surface_variant_hex : (isDark ? "#42474E" : "#DFE3EB")
    property color onSurfaceVariant: themeControllerRef ? themeControllerRef.on_surface_variant_hex : (isDark ? "#C1C6CF" : "#42474E")
    property color surfaceTint: themeControllerRef ? themeControllerRef.surface_tint_hex : (isDark ? "#92CCFF" : "#006497")
    property color surfaceDim: themeControllerRef ? themeControllerRef.surface_dim_hex : (isDark ? "#121418" : "#D7D9DF")
    property color surfaceBright: themeControllerRef ? themeControllerRef.surface_bright_hex : (isDark ? "#38393F" : "#FCFCFF")
    property color surfaceContainerLowest: themeControllerRef ? themeControllerRef.surface_container_lowest_hex : (isDark ? "#0F1113" : "#FFFFFF")
    property color surfaceContainerLow: themeControllerRef ? themeControllerRef.surface_container_low_hex : (isDark ? "#1F2225" : "#F6F8FB")
    property color surfaceContainer: themeControllerRef ? themeControllerRef.surface_container_hex : (isDark ? "#23272A" : "#F0F3F7")
    property color surfaceContainerHigh: themeControllerRef ? themeControllerRef.surface_container_high_hex : (isDark ? "#2D3135" : "#EAEFF5")
    property color surfaceContainerHighest: themeControllerRef ? themeControllerRef.surface_container_highest_hex : (isDark ? "#383C40" : "#E4E9EF")
    property color inverseSurface: themeControllerRef ? themeControllerRef.inverse_surface_hex : (isDark ? "#E2E2E5" : "#2F3033")
    property color inverseOnSurface: themeControllerRef ? themeControllerRef.inverse_on_surface_hex : (isDark ? "#2F3033" : "#F1F0F4")
    property color inversePrimary: themeControllerRef ? themeControllerRef.inverse_primary_hex : (isDark ? "#006497" : "#92CCFF")
    property color error: themeControllerRef ? themeControllerRef.error_hex : (isDark ? "#FFB4AB" : "#BA1A1A")
    property color onError: themeControllerRef ? themeControllerRef.on_error_hex : (isDark ? "#690005" : "#FFFFFF")
    property color errorContainer: themeControllerRef ? themeControllerRef.error_container_hex : (isDark ? "#93000A" : "#FFDAD6")
    property color onErrorContainer: themeControllerRef ? themeControllerRef.on_error_container_hex : (isDark ? "#FFDAD6" : "#410002")
    property color outline: themeControllerRef ? themeControllerRef.outline_hex : (isDark ? "#8C9198" : "#72787E")
    property color outlineVariant: themeControllerRef ? themeControllerRef.outline_variant_hex : (isDark ? "#42474E" : "#C1C6CF")
    property color scrim: themeControllerRef ? themeControllerRef.scrim_hex : (isDark ? "#000000" : "#000000")

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
