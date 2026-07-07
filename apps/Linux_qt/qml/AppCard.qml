// =============================================================================
// AppCard.qml — 通用卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
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

    // Elevation shadow support
    property int elevation: 1
    property var appShadow: null

    readonly property color _surfaceContainer: dt.surfaceContainer
    readonly property color _card: dt.card
    readonly property color _border: dt.border
    readonly property int _cardRadius: dt.cardRadius
    readonly property int _sp12: dt.sp12
    readonly property int _sp16: dt.sp16

    property alias spacing: col.spacing
    property string variant: "surface"
    property int padding: _sp16
    property bool outlined: true

    implicitWidth: 200
    implicitHeight: col.implicitHeight + control.padding * 2

    // Shadow layer (behind the card background)
    Rectangle {
        anchors.fill: parent
        anchors.topMargin: control.elevation > 0 && control.appShadow ? control.appShadow.forElevation(control.elevation).verticalOffset : 0
        radius: _cardRadius
        color: control.elevation > 0 && control.appShadow ? control.appShadow.forElevation(control.elevation).color : "transparent"
        opacity: 0.2
        visible: control.elevation > 0 && control.appShadow !== null
        z: -1
    }

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
