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

    // Elevation shadow support
    property int elevation: 1
    property var appShadow: null

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    // Safe access: fallback 根据 SystemPalette 推断深浅色，不再固定走 light
    readonly property color _surfaceContainer: dt ? dt.surfaceContainer : (_inferDark ? "#232830" : "#F0F3F8")
    readonly property color _card: dt ? dt.card : (_inferDark ? "#1F2229" : "#F6F8FC")
    readonly property color _border: dt ? dt.border : (_inferDark ? "#8C919842" : "#71788057")
    readonly property int _cardRadius: dt ? dt.cardRadius : 16
    readonly property int _sp12: dt ? dt.sp12 : 12
    readonly property int _sp16: dt ? dt.sp16 : 16

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
