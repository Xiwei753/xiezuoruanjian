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
    required property var dt

    readonly property color _primary: dt.primary
    readonly property color _onPrimary: dt.onPrimary
    readonly property color _secondaryContainer: dt.secondaryContainer
    readonly property color _onSecondaryContainer: dt.onSecondaryContainer
    readonly property color _surfaceContainerLow: dt.surfaceContainerLow
    readonly property color _dangerContainer: dt.dangerContainer
    readonly property color _onDangerContainer: dt.onDangerContainer
    readonly property color _border: dt.border
    readonly property color _textDisabled: dt.textDisabled
    readonly property bool _isDark: dt.isDark
    readonly property int _inputFieldRadius: dt.inputFieldRadius
    readonly property int _label: dt.label
    readonly property string _fontFamily: dt.fontFamily
    readonly property int _animFast: dt.animFast

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
