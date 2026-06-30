// =============================================================================
// AppSwitch.qml — 通用开关组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
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

    // ── SystemPalette 推断：theme 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    implicitWidth: 52
    implicitHeight: 32

    Rectangle {
        id: track
        width: 52
        height: 32
        anchors.centerIn: parent
        radius: height / 2
        color: {
            if (!control.enabled) return control.theme ? control.theme.surfaceContainer : (_inferDark ? "#232830" : "#e2e8f0")
            if (control.checked) return control.theme ? control.theme.primary : (_inferDark ? "#92CCFF" : "#006497")
            return control.theme ? control.theme.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")
        }
        border.color: control.checked ? "transparent" : (control.theme ? control.theme.outline : (_inferDark ? "#8C9198" : "#72787E"))
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
            if (!control.enabled) return control.theme ? control.theme.textDisabled : (_inferDark ? "#5A5E66" : "#94a3b8")
            if (control.checked) return control.theme ? control.theme.onPrimary : (_inferDark ? "#003351" : "#ffffff")
            return control.theme ? control.theme.outline : (_inferDark ? "#8C9198" : "#72787E")
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
