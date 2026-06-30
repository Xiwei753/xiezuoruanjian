// =============================================================================
// SidebarItem.qml — 侧栏导航项组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：侧栏的单个导航项，支持图标、文字和选中状态
// 约束：
//   - 纯 UI 组件，点击通过 onClicked 处理
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: control
    property var dt: null

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    // Safe access: fallback 根据 SystemPalette 推断深浅色，不再固定走 light
    readonly property color _primaryContainer: dt ? dt.primaryContainer : (_inferDark ? "#004A77" : "#CCE5FF")
    readonly property color _onPrimaryContainer: dt ? dt.onPrimaryContainer : (_inferDark ? "#CCE5FF" : "#001E31")
    readonly property color _onSurfaceVariant: dt ? dt.onSurfaceVariant : (_inferDark ? "#C3C6CF" : "#42474E")
    readonly property color _surfaceVariant: dt ? dt.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")
    readonly property int _sp8: dt ? dt.sp8 : 8
    readonly property int _sp12: dt ? dt.sp12 : 12
    readonly property int _radiusPill: dt ? dt.radiusPill : 999
    readonly property int _fontMd: dt ? dt.fontMd : 14
    readonly property int _label: dt ? dt.label : 13
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"

    property string text: ""
    property string icon: ""
    property bool active: false
    property bool compact: false
    signal clicked()

    height: 44
    implicitWidth: 160

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: _sp8
        anchors.rightMargin: _sp8
        radius: _radiusPill
        color: {
            if (control.active) return _primaryContainer
            return ma.containsMouse ? _surfaceVariant : "transparent"
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: _sp12
        anchors.rightMargin: _sp8
        spacing: _sp8

        AppText {
            text: control.icon
            font.pixelSize: _fontMd
            Layout.preferredWidth: 20
            horizontalAlignment: Text.AlignHCenter
            visible: !control.compact
        }

        AppText {
            text: control.text
            color: {
                if (control.active) return _onPrimaryContainer
                return _onSurfaceVariant
            }
            font.pixelSize: _label
            font.family: _fontFamily
            font.weight: control.active ? Font.Medium : Font.Normal
            Layout.fillWidth: true
            elide: Text.ElideRight
            visible: !control.compact
        }
    }

    MouseArea {
        id: ma
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: control.clicked()
    }
}
