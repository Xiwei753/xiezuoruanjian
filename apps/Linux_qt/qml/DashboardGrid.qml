// =============================================================================
// DashboardGrid.qml — 仪表盘网格布局
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：响应式网格布局，自动适配宽屏/中屏/窄屏
// 约束：
//   - 纯布局组件，内容通过 children 传入
//   - 使用 DesignTokens 统一间距
// =============================================================================

import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root
    property var dt: null
    readonly property bool wide: width >= 1120
    readonly property bool medium: width >= 760 && width < 1120
    property int gap: dt.gridGap
    Layout.fillWidth: true
    spacing: gap
}
