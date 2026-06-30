// =============================================================================
// ModernSwitch.qml — 现代开关组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：自定义样式的开关组件，支持主题适配
// 约束：
//   - 纯 UI 组件，状态通过 checked property 绑定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick

Item {
    id: root
    property var dt: null

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    // Safe access: fallback 根据 SystemPalette 推断深浅色，不再固定走 light
    readonly property color _primary: dt ? dt.primary : (_inferDark ? "#92CCFF" : "#006497")
    readonly property color _onPrimary: dt ? dt.onPrimary : (_inferDark ? "#003351" : "#FFFFFF")
    readonly property color _surfaceContainerLow: dt ? dt.surfaceContainerLow : (_inferDark ? "#1F2229" : "#F6F8FC")
    readonly property color _switchTrackOn: dt ? dt.switchTrackOn : (_inferDark ? "#92CCFF" : "#006497")
    readonly property color _switchTrackOff: dt ? dt.switchTrackOff : (_inferDark ? "#42474E" : "#DFE3EB")
    readonly property color _border: dt ? dt.border : (_inferDark ? "#8C919842" : "#71788057")
    readonly property color _outline: dt ? dt.outline : (_inferDark ? "#8C9198" : "#727880")

    property bool checked: false
    signal toggled(bool checked)

    implicitWidth: 52
    implicitHeight: 32

    Rectangle {
        anchors.centerIn: parent
        width: 50
        height: 28
        radius: 14
        color: !root.enabled ? _surfaceContainerLow : (root.checked ? _switchTrackOn : _switchTrackOff)
        border.width: root.checked ? 0 : 1
        border.color: !root.enabled ? _border : _outline

        Rectangle {
            width: root.checked ? 24 : 18
            height: width
            radius: width / 2
            y: (parent.height - height) / 2
            x: root.checked ? (parent.width - width - 2) : 5
            color: root.checked ? _onPrimary : _outline
            opacity: root.enabled ? 1.0 : 0.45
            Behavior on x { NumberAnimation { duration: 140 } }
            Behavior on width { NumberAnimation { duration: 140 } }
        }
    }

    MouseArea {
        anchors.fill: parent
        enabled: root.enabled
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            root.checked = !root.checked;
            root.toggled(root.checked);
        }
    }
}
