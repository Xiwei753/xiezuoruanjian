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

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property bool checked: false
    signal toggled(bool checked)

    implicitWidth: 52
    implicitHeight: 32

    Rectangle {
        anchors.centerIn: parent
        width: 50
        height: 28
        radius: 14
        color: !root.enabled ? _dt.surfaceContainerLow : (root.checked ? _dt.switchTrackOn : _dt.switchTrackOff)
        border.width: root.checked ? 0 : 1
        border.color: !root.enabled ? _dt.border : _dt.outline

        Rectangle {
            width: root.checked ? 24 : 18
            height: width
            radius: width / 2
            y: (parent.height - height) / 2
            x: root.checked ? (parent.width - width - 2) : 5
            color: root.checked ? _dt.onPrimary : _dt.outline
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
