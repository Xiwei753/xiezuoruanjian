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
    readonly property int minRightWidth: 200

    // { pageKey: { left: number, right: number } }
    property var paneOverrides: ({})
    // { pageKey: { left: bool, right: bool, topBar: bool } }
    property var collapsedState: ({})

    Component.onCompleted: reload()

    onSettingsBackendRefChanged: reload()

    function reload() {
        paneOverrides = ({})
        collapsedState = ({})
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
        return Math.max(minRightWidth, paneWidth(pageKey, "right") - toolRailWidth)
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
