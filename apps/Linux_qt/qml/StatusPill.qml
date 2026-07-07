// =============================================================================
// StatusPill.qml — 状态指示点组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：Material 3 风格状态胶囊，用于展示同步状态等
// 约束：
//   - 纯展示组件，颜色通过 pillColor property 传入
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: control
    property var dt: null
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt

    readonly property color _primaryContainer: resolvedDt.primaryContainer
    readonly property color _onPrimaryContainer: resolvedDt.onPrimaryContainer
    readonly property color _successContainer: resolvedDt.successContainer
    readonly property color _onSuccessContainer: resolvedDt.onSuccessContainer
    readonly property color _warningContainer: resolvedDt.warningContainer
    readonly property color _onWarningContainer: resolvedDt.onWarningContainer
    readonly property color _dangerContainer: resolvedDt.dangerContainer
    readonly property color _onDangerContainer: resolvedDt.onDangerContainer
    readonly property int _sp6: resolvedDt.sp6
    readonly property int _sp16: resolvedDt.sp16
    readonly property int _radiusPill: resolvedDt.radiusPill
    readonly property int _caption: resolvedDt.caption
    readonly property string _fontFamily: resolvedDt.fontFamily

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
            width: resolvedDt.statusDotSize
            height: resolvedDt.statusDotSize
            radius: resolvedDt.statusDotSize / 2
            color: control.contentColor
            opacity: 0.8
        }

        AppText {
            dt: control.resolvedDt
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
