// =============================================================================
// ToolbarButton.qml — 工具栏按钮组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：工具栏的扁平按钮，支持主题适配
// 约束：
//   - 纯 UI 组件，点击通过 onClicked 处理
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Button {
    id: control
    flat: true
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
    readonly property color _onPrimaryContainer: dt ? dt.onPrimaryContainer : (_inferDark ? "#CCE5FF" : "#001E31")
    readonly property color _onSurfaceVariant: dt ? dt.onSurfaceVariant : (_inferDark ? "#C3C6CF" : "#42474E")
    readonly property color _primaryContainer: dt ? dt.primaryContainer : (_inferDark ? "#004A77" : "#CCE5FF")
    readonly property color _surfaceVariant: dt ? dt.surfaceVariant : (_inferDark ? "#42474E" : "#DFE3EB")
    readonly property int _radiusPill: dt ? dt.radiusPill : 999
    readonly property int _fontSm: dt ? dt.fontSm : 12
    readonly property int _label: dt ? dt.label : 13
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"

    property bool active: false

    implicitHeight: 36
    implicitWidth: Math.max(tm.width + 24, 52)

    TextMetrics { id: tm; text: control.text; font.pixelSize: _fontSm }

    contentItem: AppText {
        text: control.text
        color: control.active ? _onPrimaryContainer : (control.hovered ? _primary : _onSurfaceVariant)
        font.pixelSize: _label
        font.family: _fontFamily
        font.weight: control.active ? Font.Medium : Font.Normal
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: control.active ? _primaryContainer : (control.hovered ? _surfaceVariant : "transparent")
        radius: _radiusPill
    }
}
