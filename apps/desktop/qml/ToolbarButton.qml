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

    // Safe access: fallback to light-theme defaults when dt is null
    FallbackTokens { id: _fallback }
    readonly property var _dt: dt ?? _fallback

    property bool active: false

    implicitHeight: 36
    implicitWidth: Math.max(tm.width + 24, 52)

    TextMetrics { id: tm; text: control.text; font.pixelSize: _dt.fontSm }

    contentItem: AppText {
        text: control.text
        color: control.active ? _dt.onPrimaryContainer : (control.hovered ? _dt.primary : _dt.onSurfaceVariant)
        font.pixelSize: _dt.label
        font.family: _dt.fontFamily
        font.weight: control.active ? Font.Medium : Font.Normal
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: control.active ? _dt.primaryContainer : (control.hovered ? _dt.surfaceVariant : "transparent")
        radius: _dt.radiusPill
    }
}
