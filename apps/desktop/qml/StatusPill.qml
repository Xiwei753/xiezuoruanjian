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
    property var dt: null
    property string status: "info"
    property string text: ""
    property color pillColor: {
        if (control.status === "success") return dt.successContainer
        if (control.status === "warning") return dt.warningContainer
        if (control.status === "error") return dt.dangerContainer
        return dt.primaryContainer
    }
    property color contentColor: {
        if (control.status === "success") return dt.onSuccessContainer
        if (control.status === "warning") return dt.onWarningContainer
        if (control.status === "error") return dt.onDangerContainer
        return dt.onPrimaryContainer
    }

    implicitWidth: pillRow.implicitWidth + (dt ? dt.sp16 : 16)
    implicitHeight: 28
    radius: dt ? dt.radiusPill : 999
    color: pillColor

    RowLayout {
        id: pillRow
        anchors.centerIn: parent
        spacing: dt ? dt.sp6 : 6

        Rectangle {
            width: 7
            height: 7
            radius: dt.radiusXs
            color: control.contentColor
            opacity: 0.8
        }

        AppText {
            text: control.text
            visible: control.text.length > 0
            color: control.contentColor
            font.pixelSize: dt ? dt.caption : 12
            font.family: dt ? dt.fontFamily : "sans-serif"
            font.weight: Font.Medium
        }
    }
}
