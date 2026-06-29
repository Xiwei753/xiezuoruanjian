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
    readonly property color _primaryContainer: dt ? dt.primaryContainer : "#CCE5FF"
    readonly property color _onPrimaryContainer: dt ? dt.onPrimaryContainer : "#001E31"
    readonly property color _successContainer: dt ? dt.successContainer : "#B9F0C8"
    readonly property color _onSuccessContainer: dt ? dt.onSuccessContainer : "#00210F"
    readonly property color _warningContainer: dt ? dt.warningContainer : "#FFE2A8"
    readonly property color _onWarningContainer: dt ? dt.onWarningContainer : "#261A00"
    readonly property color _dangerContainer: dt ? dt.dangerContainer : "#FFDAD6"
    readonly property color _onDangerContainer: dt ? dt.onDangerContainer : "#410002"
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
