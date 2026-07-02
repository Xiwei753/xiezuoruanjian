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

    readonly property color _primaryContainer: dt.primaryContainer
    readonly property color _onPrimaryContainer: dt.onPrimaryContainer
    readonly property color _successContainer: dt.successContainer
    readonly property color _onSuccessContainer: dt.onSuccessContainer
    readonly property color _warningContainer: dt.warningContainer
    readonly property color _onWarningContainer: dt.onWarningContainer
    readonly property color _dangerContainer: dt.dangerContainer
    readonly property color _onDangerContainer: dt.onDangerContainer
    readonly property int _sp6: dt.sp6
    readonly property int _sp16: dt.sp16
    readonly property int _radiusPill: dt.radiusPill
    readonly property int _caption: dt.caption
    readonly property string _fontFamily: dt.fontFamily

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
        anchors.verticalCenter: parent.verticalCenter
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: _sp6

        Rectangle {
            Layout.alignment: Qt.AlignVCenter
            width: dt.statusDotSize
            height: dt.statusDotSize
            radius: dt.statusDotSize / 2
            color: control.contentColor
            opacity: 0.8
        }

        AppText {
            dt: control.dt
            text: control.text
            visible: control.text.length > 0
            color: control.contentColor
            verticalAlignment: Text.AlignVCenter
            font.pixelSize: _caption
            font.family: _fontFamily
            font.weight: Font.Medium
        }
    }
}
