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
    property bool active: false

    implicitHeight: 36
    implicitWidth: Math.max(tm.width + 24, 52)

    TextMetrics { id: tm; text: control.text; font.pixelSize: dt ? dt.fontSm : 12 }

    contentItem: AppText {
        text: control.text
        color: control.active ? dt.onPrimaryContainer : (control.hovered ? dt.primary : dt.onSurfaceVariant)
        font.pixelSize: dt ? dt.label : 13
        font.family: dt ? dt.fontFamily : "sans-serif"
        font.weight: control.active ? Font.Medium : Font.Normal
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: control.active ? dt.primaryContainer : (control.hovered ? dt.surfaceVariant : "transparent")
        radius: dt ? dt.radiusPill : 999
    }
}
