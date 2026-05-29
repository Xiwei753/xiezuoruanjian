// =============================================================================
// AppSwitch.qml — 通用开关组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：统一风格的开关组件，支持主题适配
// 约束：
//   - 纯 UI 组件，状态通过 checked property 绑定
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Item {
    id: control
    property var theme: null
    property bool checked: false
    property bool enabled: true
    signal clicked()

    implicitWidth: 40
    implicitHeight: 22

    Rectangle {
        id: track
        anchors.fill: parent
        radius: height / 2
        color: {
            if (!control.enabled) return control.theme ? control.theme.border : "#555555"
            if (control.checked) return control.theme ? control.theme.primary : "#0ea5e9"
            return control.theme ? control.theme.border : "#334155"
        }
        border.color: {
            if (!control.checked && !control.enabled) return "transparent"
            if (!control.checked) return control.theme ? control.theme.border : "#334155"
            return "transparent"
        }
        border.width: control.checked ? 0 : 1

        Behavior on color {
            ColorAnimation { duration: 150; easing.type: Easing.OutCubic }
        }
    }

    Rectangle {
        id: knob
        width: 16
        height: 16
        radius: 8
        color: control.enabled ? "#ffffff" : (control.theme ? control.theme.textDisabled : "#94a3b8")
        y: 3
        x: control.checked ? parent.width - width - 3 : 3

        Behavior on x {
            NumberAnimation { duration: 150; easing.type: Easing.OutCubic }
        }

        Behavior on color {
            ColorAnimation { duration: 150 }
        }
    }

    MouseArea {
        anchors.fill: parent
        enabled: control.enabled
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            control.checked = !control.checked
            control.clicked()
        }
    }
}
