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

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property alias spacing: col.spacing
    property string variant: "surface"
    property int padding: _dt.sp16
    property bool outlined: true

    implicitWidth: 200
    implicitHeight: col.implicitHeight + control.padding * 2

    Rectangle {
        anchors.fill: parent
        radius: _dt.cardRadius
        color: control.variant === "surfaceVariant" ? _dt.surfaceContainer : _dt.card
        border.color: _dt.border
        border.width: control.outlined ? 1 : 0
    }

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: control.padding
        spacing: _dt.sp12
    }
}
