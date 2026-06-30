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

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    // Safe access: fallback 根据 SystemPalette 推断深浅色，不再固定走 light
    readonly property color _primary: dt ? dt.primary : (_inferDark ? "#92CCFF" : "#006497")
    readonly property color _onPrimary: dt ? dt.onPrimary : (_inferDark ? "#003351" : "#FFFFFF")
    readonly property color _secondaryContainer: dt ? dt.secondaryContainer : (_inferDark ? "#394858" : "#D4E3F7")
    readonly property color _onSecondaryContainer: dt ? dt.onSecondaryContainer : (_inferDark ? "#D4E3F7" : "#0E1D2A")
    readonly property color _surfaceContainerLow: dt ? dt.surfaceContainerLow : (_inferDark ? "#1F2229" : "#F6F8FC")
    readonly property color _dangerContainer: dt ? dt.dangerContainer : (_inferDark ? "#93000A" : "#FFDAD6")
    readonly property color _onDangerContainer: dt ? dt.onDangerContainer : (_inferDark ? "#FFDAD6" : "#410002")
    readonly property color _border: dt ? dt.border : (_inferDark ? "#8C919842" : "#71788057")
    readonly property color _textDisabled: dt ? dt.textDisabled : (_inferDark ? "#E2E2E561" : "#1A1C1E61")
    readonly property bool _isDark: dt ? dt.isDark : _inferDark
    readonly property int _inputFieldRadius: dt ? dt.inputFieldRadius : 12
    readonly property int _label: dt ? dt.label : 13
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"
    readonly property int _animFast: dt ? dt.animFast : 120

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
