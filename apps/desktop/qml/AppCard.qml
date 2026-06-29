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
    readonly property color _surfaceContainer: dt ? dt.surfaceContainer : "#F0F3F8"
    readonly property color _card: dt ? dt.card : "#F6F8FC"
    readonly property color _border: dt ? dt.border : "#71788057"
    readonly property int _cardRadius: dt ? dt.cardRadius : 16
    readonly property int _sp12: dt ? dt.sp12 : 12
    readonly property int _sp16: dt ? dt.sp16 : 16

    property alias spacing: col.spacing
    property string variant: "surface"
    property int padding: _sp16
    property bool outlined: true

    implicitWidth: 200
    implicitHeight: col.implicitHeight + control.padding * 2

    Rectangle {
        anchors.fill: parent
        radius: _cardRadius
        color: control.variant === "surfaceVariant" ? _surfaceContainer : _card
        border.color: _border
        border.width: control.outlined ? 1 : 0
    }

    default property alias content: col.children

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: control.padding
        spacing: _sp12
    }
}
