// =============================================================================
// AppButton.qml — 通用按钮组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：统一风格的按钮组件，支持 primary/secondary/text/danger 变体
// 约束：
//   - 纯 UI 组件，点击事件通过 onClicked 处理
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Button {
    id: control
    property var dt: null

    readonly property color _primary: dt ? dt.primary : "#6750A4"
    readonly property color _onPrimary: dt ? dt.onPrimary : "#FFFFFF"
    readonly property color _secondaryContainer: dt ? dt.secondaryContainer : "#E8DEF8"
    readonly property color _onSecondaryContainer: dt ? dt.onSecondaryContainer : "#1D192B"
    readonly property color _surfaceContainerLow: dt ? dt.surfaceContainerLow : "#F7F2FA"
    readonly property color _dangerContainer: dt ? dt.dangerContainer : "#F9DED9"
    readonly property color _onDangerContainer: dt ? dt.onDangerContainer : "#410E0B"
    readonly property color _border: dt ? dt.border : "#CAC4D0"
    readonly property color _textDisabled: dt ? dt.textDisabled : "#1D192B1F"
    readonly property bool _isDark: dt ? dt.isDark : false
    readonly property int _inputFieldRadius: dt ? dt.inputFieldRadius : 8
    readonly property int _label: dt ? dt.label : 14
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"
    readonly property int _animFast: dt ? dt.animFast : 150

    property string tooltip: ""
    property bool small: false
    property bool primary: false
    property string variant: primary ? "primary" : "secondary"

    readonly property bool isPrimary: control.variant === "primary"
    readonly property bool isDanger: control.variant === "danger"
    readonly property bool isText: control.variant === "text"
    readonly property bool isSecondary: control.variant === "secondary"

    readonly property color containerColor: {
        if (!control.enabled) return _surfaceContainerLow;
        if (isPrimary) return _primary;
        if (isDanger) return _dangerContainer;
        if (isText) return "transparent";
        return _secondaryContainer;
    }

    readonly property color contentColor: {
        if (!control.enabled) return _textDisabled;
        if (isPrimary) return _onPrimary;
        if (isDanger) return _onDangerContainer;
        if (isText) return _primary;
        return _onSecondaryContainer;
    }

    implicitHeight: small ? 32 : 40
    implicitWidth: Math.max(tm.width + (small ? 20 : 28), small ? 56 : 72)

    TextMetrics { id: tm; text: control.text; font.pixelSize: _label }

    contentItem: AppText {
        dt: control.dt
        text: control.text
        color: control.contentColor
        font.pixelSize: _label
        font.weight: Font.Medium
        font.family: _fontFamily
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: {
            if (!control.enabled) return control.containerColor;
            if (control.pressed) {
                if (isText) return _surfaceContainerLow;
                return _isDark ? Qt.lighter(control.containerColor, 1.08) : Qt.darker(control.containerColor, 1.08);
            }
            if (control.hovered) {
                if (isText) return _surfaceContainerLow;
                return _isDark ? Qt.lighter(control.containerColor, 1.05) : Qt.darker(control.containerColor, 1.03);
            }
            return control.containerColor;
        }
        border.color: control.variant === "secondary" ? _border : "transparent"
        border.width: control.variant === "secondary" ? 1 : 0
        radius: _inputFieldRadius

        Behavior on color { ColorAnimation { duration: _animFast } }
    }
}
