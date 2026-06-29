// =============================================================================
// AppCard.qml — 通用卡片组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：统一风格的卡片容器，支持主题适配
// 约束：
//   - 纯布局组件，内容通过 children 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Item {
    id: control
    property var dt: null
    property alias spacing: col.spacing
    property string variant: "surface"
    property int padding: dt ? dt.sp16 : 16
    property bool outlined: true

    implicitWidth: 200
    implicitHeight: col.implicitHeight + control.padding * 2

    Rectangle {
        anchors.fill: parent
        radius: dt ? dt.cardRadius : dt.radiusLg
        color: control.variant === "surfaceVariant" ? dt.surfaceContainer : dt.card
        border.color: dt.border
        border.width: control.outlined ? 1 : 0
    }

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: control.padding
        spacing: dt ? dt.sp12 : 12
    }
}
