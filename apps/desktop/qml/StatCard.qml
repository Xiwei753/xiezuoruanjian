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
    property color tone: dt.primary
    radius: dt.radiusLg
    color: dt.card
    border.color: dt.border
    border.width: 1

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt.sp12
        spacing: dt.sp6

        AppText { dt: root.dt; text: root.title; color: dt.onSurfaceVariant; font.pixelSize: dt.label; font.family: dt.fontFamily }
        AppText { dt: root.dt; text: root.value; color: root.tone; font.pixelSize: dt.title; font.family: dt.fontFamily; font.weight: Font.Bold }
        AppText { dt: root.dt; text: root.caption; color: dt.textMuted; font.pixelSize: dt.caption; font.family: dt.fontFamily }
    }
}
