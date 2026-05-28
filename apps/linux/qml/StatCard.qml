// =============================================================================
// StatCard.qml — 统计卡片组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：展示单个统计数据的卡片（标题、数值、说明）
// 约束：
//   - 纯展示组件，数据通过 property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property string title: ""
    property string value: "0"
    property string caption: ""
    property color tone: dt ? dt.accent : "#7B8CDE"
    radius: dt ? dt.radiusMd : 12
    color: dt ? dt.card : "#1E2128"
    border.color: dt ? dt.border : "#2A2E36"
    border.width: 1

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp12 : 12
        spacing: dt ? dt.sp6 : 6

        Text { text: root.title; color: dt ? dt.textSecondary : "#9CA0AB"; font.pixelSize: dt ? dt.fontSm : 12 }
        Text { text: root.value; color: root.tone; font.pixelSize: dt ? dt.fontXxl : 22; font.weight: Font.Bold }
        Text { text: root.caption; color: dt ? dt.textMuted : "#606470"; font.pixelSize: dt ? dt.fontXs : 11 }
    }
}
