// =============================================================================
// ModernSwitch.qml — 现代开关组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：自定义样式的开关组件，支持主题适配
// 约束：
//   - 纯 UI 组件，状态通过 checked property 绑定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick

Item {
    id: root
    property var dt: null
    property bool checked: false
    property bool enabled: true
    signal toggled(bool checked)

    width: 52
    height: 32

    Rectangle {
        anchors.centerIn: parent
        width: 50
        height: 28
        radius: 14
        color: !root.enabled ? (dt ? dt.surfaceVariant : "#2A2E36") : (root.checked ? (dt ? dt.switchTrackOn : "#6679D8") : (dt ? dt.switchTrackOff : "#303543"))
        border.width: 1
        border.color: !root.enabled ? (dt ? dt.border : "#2A2E36") : (root.checked ? Qt.lighter(dt ? dt.switchTrackOn : "#6679D8", 1.1) : (dt ? dt.controlBorder : "#3A3F49"))

        Rectangle {
            width: 24
            height: 24
            radius: 12
            y: 2
            x: root.checked ? (parent.width - width - 2) : 2
            color: dt ? dt.switchThumb : "#EEF1FB"
            opacity: root.enabled ? 1.0 : 0.45
            Behavior on x { NumberAnimation { duration: 140 } }
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
