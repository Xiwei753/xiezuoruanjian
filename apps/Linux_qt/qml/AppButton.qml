// =============================================================================
// AppButton.qml — 通用按钮组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
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
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt

    readonly property color _primary: resolvedDt.primary
    readonly property color _onPrimary: resolvedDt.onPrimary
    readonly property color _secondaryContainer: resolvedDt.secondaryContainer
    readonly property color _onSecondaryContainer: resolvedDt.onSecondaryContainer
    readonly property color _surfaceContainerLow: resolvedDt.surfaceContainerLow
    readonly property color _dangerContainer: resolvedDt.dangerContainer
    readonly property color _onDangerContainer: resolvedDt.onDangerContainer
    readonly property color _border: resolvedDt.border
    readonly property color _textDisabled: resolvedDt.textDisabled
    readonly property bool _isDark: resolvedDt.isDark
    readonly property int _inputFieldRadius: resolvedDt.inputFieldRadius
    readonly property int _label: resolvedDt.label
    readonly property string _fontFamily: resolvedDt.fontFamily
    readonly property int _animFast: resolvedDt.animFast

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
        dt: control.resolvedDt
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
