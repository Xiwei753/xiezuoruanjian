// =============================================================================
// StatCard.qml — 统计卡片组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：展示单个统计数据的卡片（标题、数值、说明）
// 约束：
//   - 纯展示组件，数据通过 property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property string title: ""
    property string value: "0"
    property string caption: ""
    property color tone: dt ? dt.primary : "#006497"
    radius: dt ? dt.radiusLg : 16
    color: dt ? dt.card : "#1E2128"
    border.color: dt ? dt.border : "#2A2E36"
    border.width: 1

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp12 : 12
        spacing: dt ? dt.sp6 : 6

        AppText { text: root.title; color: dt ? dt.onSurfaceVariant : "#42474E"; font.pixelSize: dt ? dt.label : 13; font.family: dt ? dt.fontFamily : "sans-serif" }
        AppText { text: root.value; color: root.tone; font.pixelSize: dt ? dt.title : 24; font.family: dt ? dt.fontFamily : "sans-serif"; font.weight: Font.Bold }
        AppText { text: root.caption; color: dt ? dt.textMuted : "#74787F"; font.pixelSize: dt ? dt.caption : 12; font.family: dt ? dt.fontFamily : "sans-serif" }
    }
}
