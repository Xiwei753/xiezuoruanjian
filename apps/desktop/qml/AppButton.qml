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

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property string tooltip: ""
    property bool small: false
    property bool primary: false
    property string variant: primary ? "primary" : "secondary"

    readonly property bool isPrimary: control.variant === "primary"
    readonly property bool isDanger: control.variant === "danger"
    readonly property bool isText: control.variant === "text"
    readonly property bool isSecondary: control.variant === "secondary"

    readonly property color containerColor: {
        if (!control.enabled) return _dt.surfaceContainerLow;
        if (isPrimary) return _dt.primary;
        if (isDanger) return _dt.dangerContainer;
        if (isText) return "transparent";
        return _dt.secondaryContainer;
    }

    readonly property color contentColor: {
        if (!control.enabled) return _dt.textDisabled;
        if (isPrimary) return _dt.onPrimary;
        if (isDanger) return _dt.onDangerContainer;
        if (isText) return _dt.primary;
        return _dt.onSecondaryContainer;
    }

    implicitHeight: small ? 32 : 40
    implicitWidth: Math.max(tm.width + (small ? 20 : 28), small ? 56 : 72)

    TextMetrics { id: tm; text: control.text; font.pixelSize: _dt.label }

    contentItem: AppText {
        text: control.text
        color: control.contentColor
        font.pixelSize: _dt.label
        font.weight: Font.Medium
        font.family: _dt.fontFamily
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: {
            if (!control.enabled) return control.containerColor;
            if (control.pressed) {
                if (isText) return _dt.surfaceContainerLow;
                return _dt.isDark ? Qt.lighter(control.containerColor, 1.08) : Qt.darker(control.containerColor, 1.08);
            }
            if (control.hovered) {
                if (isText) return _dt.surfaceContainerLow;
                return _dt.isDark ? Qt.lighter(control.containerColor, 1.05) : Qt.darker(control.containerColor, 1.03);
            }
            return control.containerColor;
        }
        border.color: control.variant === "secondary" ? _dt.border : "transparent"
        border.width: control.variant === "secondary" ? 1 : 0
        radius: _dt.inputFieldRadius

        Behavior on color { ColorAnimation { duration: _dt.animFast } }
    }
}
