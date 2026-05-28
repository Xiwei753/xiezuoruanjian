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

import QtQuick 2.15
import QtQuick.Controls 2.15

Button {
    id: control
    flat: true
    property var theme: null

    implicitHeight: 32
    implicitWidth: Math.max(tm.width + 20, 48)

    TextMetrics { id: tm; text: control.text; font.pixelSize: control.theme ? control.theme.fontSm : 12 }

    contentItem: Text {
        text: control.text
        color: control.hovered ? (control.theme ? control.theme.primary : "#3b82f6") : (control.theme ? control.theme.textSecondary : "#475569")
        font.pixelSize: control.theme ? control.theme.fontSm : 12
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        color: control.hovered ? (control.theme ? control.theme.hover : "#1e293b") : "transparent"
        radius: control.theme ? control.theme.radiusSm : 4
    }
}
