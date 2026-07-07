// =============================================================================
// AppSwitch.qml — 通用开关组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
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
    signal clicked()

    implicitWidth: 52
    implicitHeight: 32

    Rectangle {
        id: track
        width: 52
        height: 32
        anchors.centerIn: parent
        radius: height / 2
        color: {
            if (!control.enabled) return control.theme.surfaceContainer
            if (control.checked) return control.theme.primary
            return control.theme.surfaceVariant
        }
        border.color: control.checked ? "transparent" : control.theme.outline
        border.width: control.checked ? 0 : 1

        Behavior on color {
            ColorAnimation { duration: 150; easing.type: Easing.OutCubic }
        }
    }

    Rectangle {
        id: knob
        width: control.checked ? 24 : 18
        height: width
        radius: width / 2
        color: {
            if (!control.enabled) return control.theme.textDisabled
            if (control.checked) return control.theme.onPrimary
            return control.theme.outline
        }
        y: (track.height - height) / 2
        x: control.checked ? track.width - width - 4 : 7

        Behavior on x {
            NumberAnimation { duration: 150; easing.type: Easing.OutCubic }
        }
        Behavior on width { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }

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
