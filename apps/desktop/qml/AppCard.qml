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
    property string variant: "surface"
    property int padding: theme ? theme.sp16 : 16
    property bool outlined: true

    implicitWidth: 200
    implicitHeight: col.implicitHeight + control.padding * 2

    Rectangle {
        anchors.fill: parent
        radius: theme ? theme.radiusLg : 16
        color: {
            if (!theme) return "#ffffff"
            return control.variant === "surfaceVariant" ? theme.surfaceContainer : theme.card
        }
        border.color: theme ? theme.border : "#e2e8f0"
        border.width: control.outlined ? 1 : 0
    }

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: control.padding
        spacing: theme ? theme.sp12 : 12
    }
}
