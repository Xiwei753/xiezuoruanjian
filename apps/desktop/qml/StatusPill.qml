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

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    // Safe access: fallback 根据 SystemPalette 推断深浅色，不再固定走 light
    readonly property color _primaryContainer: dt ? dt.primaryContainer : (_inferDark ? "#004A77" : "#CCE5FF")
    readonly property color _onPrimaryContainer: dt ? dt.onPrimaryContainer : (_inferDark ? "#CCE5FF" : "#001E31")
    readonly property color _successContainer: dt ? dt.successContainer : (_inferDark ? "#005A30" : "#B9F0C8")
    readonly property color _onSuccessContainer: dt ? dt.onSuccessContainer : (_inferDark ? "#B9F0C8" : "#00210F")
    readonly property color _warningContainer: dt ? dt.warningContainer : (_inferDark ? "#5D4200" : "#FFE2A8")
    readonly property color _onWarningContainer: dt ? dt.onWarningContainer : (_inferDark ? "#FFE2A8" : "#261A00")
    readonly property color _dangerContainer: dt ? dt.dangerContainer : (_inferDark ? "#93000A" : "#FFDAD6")
    readonly property color _onDangerContainer: dt ? dt.onDangerContainer : (_inferDark ? "#FFDAD6" : "#410002")
    readonly property int _sp6: dt ? dt.sp6 : 6
    readonly property int _sp16: dt ? dt.sp16 : 16
    readonly property int _radiusPill: dt ? dt.radiusPill : 999
    readonly property int _radiusXs: dt ? dt.radiusXs : 4
    readonly property int _caption: dt ? dt.caption : 12
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"

    property string status: "info"
    property string text: ""
    property color pillColor: {
        if (control.status === "success") return _successContainer
        if (control.status === "warning") return _warningContainer
        if (control.status === "error") return _dangerContainer
        return _primaryContainer
    }
    property color contentColor: {
        if (control.status === "success") return _onSuccessContainer
        if (control.status === "warning") return _onWarningContainer
        if (control.status === "error") return _onDangerContainer
        return _onPrimaryContainer
    }

    implicitWidth: pillRow.implicitWidth + _sp16
    implicitHeight: 28
    radius: _radiusPill
    color: pillColor

    RowLayout {
        id: pillRow
        anchors.centerIn: parent
        spacing: _sp6

        Rectangle {
            width: 7
            height: 7
            radius: _radiusXs
            color: control.contentColor
            opacity: 0.8
        }

        AppText {
            text: control.text
            visible: control.text.length > 0
            color: control.contentColor
            font.pixelSize: _caption
            font.family: _fontFamily
            font.weight: Font.Medium
        }
    }
}
