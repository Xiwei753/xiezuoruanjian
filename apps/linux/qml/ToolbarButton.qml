// =============================================================================
// ToolbarButton.qml — 工具栏按钮组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
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
    property var theme: null
    property bool active: false

    implicitHeight: 36
    implicitWidth: Math.max(tm.width + 24, 52)

    TextMetrics { id: tm; text: control.text; font.pixelSize: control.theme ? control.theme.fontSm : 12 }

    contentItem: AppText {
        text: control.text
        color: control.active ? (control.theme ? control.theme.onPrimaryContainer : "#001E31") : (control.hovered ? (control.theme ? control.theme.primary : "#006497") : (control.theme ? control.theme.onSurfaceVariant : "#42474E"))
        font.pixelSize: control.theme ? control.theme.label : 13
        font.family: control.theme ? control.theme.fontFamily : "sans-serif"
        font.weight: control.active ? Font.Medium : Font.Normal
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: control.active ? (control.theme ? control.theme.primaryContainer : "#CCE5FF") : (control.hovered ? (control.theme ? control.theme.surfaceVariant : "#DFE3EB") : "transparent")
        radius: control.theme ? control.theme.radiusPill : 999
    }
}
