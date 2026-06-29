// =============================================================================
// AppShadow.qml — 统一阴影 helper
// =============================================================================
//
// 层级：Desktop UI 层
// 职责：提供统一的 elevation 阴影效果，禁止组件自行硬编码阴影
// 约束：所有需要阴影的组件必须使用此 helper 或引用 DesignTokens.shadow*
// =============================================================================

import QtQuick

QtObject {
    id: appShadow

    property DesignTokens dt: null

    // Elevation 0: no shadow
    readonly property var elevation0: null

    // Elevation 1: subtle shadow (cards at rest)
    readonly property var elevation1: dt ? {
        "color": dt.shadowLight,
        "radius": 4,
        "verticalOffset": 1,
        "horizontalOffset": 0,
        "spread": 0
    } : null

    // Elevation 2: medium shadow (FAB, bottom bar)
    readonly property var elevation2: dt ? {
        "color": dt.shadowMedium,
        "radius": 8,
        "verticalOffset": 2,
        "horizontalOffset": 0,
        "spread": 0
    } : null

    // Elevation 3: strong shadow (drawer, modal)
    readonly property var elevation3: dt ? {
        "color": dt.shadowDrawer,
        "radius": 12,
        "verticalOffset": 4,
        "horizontalOffset": 0,
        "spread": 0
    } : null

    // Convenience: get shadow config by elevation level
    function forElevation(level) {
        switch(level) {
            case 0: return elevation0
            case 1: return elevation1
            case 2: return elevation2
            case 3: return elevation3
            default: return elevation0
        }
    }
}
