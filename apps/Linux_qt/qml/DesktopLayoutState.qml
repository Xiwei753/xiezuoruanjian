// =============================================================================
// DesktopLayoutState.qml — Linux 桌面统一页面骨架尺寸与折叠状态
// =============================================================================
//
// Issue #1014：所有主页面共享同一套顶栏 / 左栏 / 右栏默认尺寸；用户手动拖动
// 后只保存当前页面的覆盖宽度。页面切换、窗口缩放等普通布局重算不写持久化。
//
// 页面 key：
//   hub.projects / hub.starmaps / hub.stats / writing / starmap.workspace
//
// 宽度语义：
//   - left：完整左栏宽度；
//   - right：完整右栏宽度，包含 56vp 工具轨与工具内容；
//   - 默认右栏 296 = 240 内容 + 56 工具轨。
//
// 折叠状态与宽度独立：折叠不会清空用户拖动宽度，展开时恢复该页宽度。
// =============================================================================

import QtQuick

QtObject {
    id: root

    required property var settingsBackendRef

    readonly property int topBarHeight: 64
    readonly property int defaultLeftWidth: 240
    readonly property int defaultRightWidth: 296
    readonly property int toolRailWidth: 56
    readonly property int splitHandleWidth: 6
    readonly property int minLeftWidth: 200
    readonly property int minRightContentWidth: 200
    // 右栏总宽度 = 内容最小宽度 + 56vp 工具轨；SplitView 的最小值必须用总宽度，
    // 否则 200 会被工具轨挤占成 144 内容。
    readonly property int minRightWidth: minRightContentWidth + toolRailWidth

    // { pageKey: { left: number, right: number } }
    property var paneOverrides: ({})
    // Issue #1016：本次窗口布局真实渲染宽度（不持久化）。0 表示该侧当前不可见。
    // 与用户拖动偏好分开：窗口缩窄只改这里，不覆盖 paneOverrides。
    property var renderedPaneWidths: ({})
    // { pageKey: { left: bool, right: bool, topBar: bool } }
    property var collapsedState: ({})

    Component.onCompleted: reload()

    onSettingsBackendRefChanged: reload()

    function reload() {
        paneOverrides = ({})
        collapsedState = ({})
        renderedPaneWidths = ({})
        if (!settingsBackendRef)
            return
        var raw = settingsBackendRef.desktop_layout_json()
        if (!raw)
            return
        try {
            var parsed = JSON.parse(raw)
            paneOverrides = parsed.pagePaneOverrides || ({})
            collapsedState = parsed.pageCollapsed || ({})
        } catch (e) {
            paneOverrides = ({})
            collapsedState = ({})
        }
    }

    function hubPageKey(tab) {
        if (tab === 1)
            return "hub.starmaps"
        if (tab === 2)
            return "hub.stats"
        return "hub.projects"
    }

    function paneWidth(pageKey, side) {
        var fallback = side === "left" ? defaultLeftWidth : defaultRightWidth
        var page = paneOverrides[pageKey]
        if (!page)
            return fallback
        var value = page[side]
        return typeof value === "number" && value > 0 ? value : fallback
    }

    // 右栏内容宽度（不含工具轨）。
    function rightContentWidth(pageKey) {
        return Math.max(minRightContentWidth, paneWidth(pageKey, "right") - toolRailWidth)
    }

    /// 记录某页某侧本次布局的真实宽度；0 表示不可见。不写盘。
    function setRenderedPaneWidth(pageKey, side, visibleWidth) {
        if (!pageKey)
            return
        var width = Math.max(0, Math.round(visibleWidth))
        var page = renderedPaneWidths[pageKey]
        if (page && page[side] === width)
            return
        var next = Object.assign({}, renderedPaneWidths)
        var nextPage = Object.assign({}, next[pageKey] || ({}))
        nextPage[side] = width
        next[pageKey] = nextPage
        renderedPaneWidths = next
    }

    /// 顶栏读取本次真实渲染宽度；尚未上报时回退到期望宽度/折叠状态。
    function renderedPaneWidth(pageKey, side) {
        var page = renderedPaneWidths[pageKey]
        if (page && typeof page[side] === "number")
            return page[side]
        if (isCollapsed(pageKey, side))
            return 0
        return paneWidth(pageKey, side)
    }

    function setDraggedWidth(pageKey, side, value) {
        if (!pageKey || !settingsBackendRef)
            return
        var width = Math.round(value)
        if (width <= 0)
            return
        var next = Object.assign({}, paneOverrides)
        var page = Object.assign({}, next[pageKey] || ({}))
        page[side] = width
        next[pageKey] = page
        paneOverrides = next
        // 只在这里写盘；调用方只在用户拖动结束 / 点击折叠时调用。
        settingsBackendRef.set_desktop_pane_width(pageKey, side, width)
    }

    function isCollapsed(pageKey, part) {
        var page = collapsedState[pageKey]
        if (page && typeof page[part] === "boolean")
            return page[part]
        // 写作页右栏默认收起（与既有工作台初始状态一致）。
        return pageKey === "writing" && part === "right"
    }

    function setCollapsed(pageKey, part, collapsed) {
        if (!pageKey || !settingsBackendRef)
            return
        var next = Object.assign({}, collapsedState)
        var page = Object.assign({}, next[pageKey] || ({}))
        page[part] = collapsed
        next[pageKey] = page
        collapsedState = next
        settingsBackendRef.set_desktop_pane_collapsed(pageKey, part, collapsed)
    }

    function toggleCollapsed(pageKey, part) {
        setCollapsed(pageKey, part, !isCollapsed(pageKey, part))
    }
}
