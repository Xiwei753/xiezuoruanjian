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

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property string status: "info"
    property string text: ""
    property color pillColor: {
        if (control.status === "success") return _dt.successContainer
        if (control.status === "warning") return _dt.warningContainer
        if (control.status === "error") return _dt.dangerContainer
        return _dt.primaryContainer
    }
    property color contentColor: {
        if (control.status === "success") return _dt.onSuccessContainer
        if (control.status === "warning") return _dt.onWarningContainer
        if (control.status === "error") return _dt.onDangerContainer
        return _dt.onPrimaryContainer
    }

    implicitWidth: pillRow.implicitWidth + _dt.sp16
    implicitHeight: 28
    radius: _dt.radiusPill
    color: pillColor

    RowLayout {
        id: pillRow
        anchors.centerIn: parent
        spacing: _dt.sp6

        Rectangle {
            width: 7
            height: 7
            radius: _dt.radiusXs
            color: control.contentColor
            opacity: 0.8
        }

        AppText {
            text: control.text
            visible: control.text.length > 0
            color: control.contentColor
            font.pixelSize: _dt.caption
            font.family: _dt.fontFamily
            font.weight: Font.Medium
        }
    }
}
