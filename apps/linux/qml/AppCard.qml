// =============================================================================
// AppCard.qml — 通用卡片组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一风格的卡片容器，支持主题适配
// 约束：
//   - 纯布局组件，内容通过 children 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Item {
    id: control
    property var theme: null
    property alias spacing: col.spacing

    implicitWidth: 200

    Rectangle {
        anchors.fill: parent
        radius: theme ? theme.radiusMd : 8
        color: theme ? theme.surface : "#ffffff"
        border.color: theme ? theme.border : "#e2e8f0"
        border.width: 1
    }

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: theme ? theme.sp12 : 12
        spacing: theme ? theme.sp8 : 8
    }
}
