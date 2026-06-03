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
    property bool checked: false
    signal toggled(bool checked)

    implicitWidth: 52
    implicitHeight: 32

    Rectangle {
        anchors.centerIn: parent
        width: 50
        height: 28
        radius: 14
        color: !root.enabled ? (dt ? dt.surfaceContainer : "#E2E8F0") : (root.checked ? (dt ? dt.primary : "#006497") : (dt ? dt.surfaceVariant : "#DFE3EB"))
        border.width: root.checked ? 0 : 1
        border.color: !root.enabled ? (dt ? dt.border : "#CBD5E1") : (dt ? dt.outline : "#72787E")

        Rectangle {
            width: root.checked ? 24 : 18
            height: width
            radius: width / 2
            y: (parent.height - height) / 2
            x: root.checked ? (parent.width - width - 2) : 5
            color: root.checked ? (dt ? dt.onPrimary : "#FFFFFF") : (dt ? dt.outline : "#72787E")
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
