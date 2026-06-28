// =============================================================================
// StatusPill.qml — 状态指示点组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：Material 3 风格状态胶囊，用于展示同步状态等
// 约束：
//   - 纯展示组件，颜色通过 pillColor property 传入
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: control
    property var theme: null
    property string status: "info"
    property string text: ""
    property color pillColor: {
        if (!control.theme) return "#CCE5FF"
        if (control.status === "success") return control.theme.successContainer
        if (control.status === "warning") return control.theme.warningContainer
        if (control.status === "error") return control.theme.dangerContainer
        return control.theme.primaryContainer
    }
    property color contentColor: {
        if (!control.theme) return "#001E31"
        if (control.status === "success") return control.theme.onSuccessContainer
        if (control.status === "warning") return control.theme.onWarningContainer
        if (control.status === "error") return control.theme.onDangerContainer
        return control.theme.onPrimaryContainer
    }

    implicitWidth: pillRow.implicitWidth + (theme ? theme.sp16 : 16)
    implicitHeight: 28
    radius: theme ? theme.radiusPill : 999
    color: pillColor

    RowLayout {
        id: pillRow
        anchors.centerIn: parent
        spacing: control.theme ? control.theme.sp6 : 6

        Rectangle {
            width: 7
            height: 7
            radius: 4
            color: control.contentColor
            opacity: 0.8
        }

        AppText {
            text: control.text
            visible: control.text.length > 0
            color: control.contentColor
            font.pixelSize: control.theme ? control.theme.caption : 12
            font.family: control.theme ? control.theme.fontFamily : "sans-serif"
            font.weight: Font.Medium
        }
    }
}
