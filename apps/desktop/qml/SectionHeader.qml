// =============================================================================
// SectionHeader.qml — 分区标题组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：分区标题文本，支持主题适配
// 约束：
//   - 纯展示组件，文本通过 text property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls

Text {
    id: control
    property var theme: null

    font.pixelSize: control.theme.fontXl
    font.weight: Font.Bold
    color: control.theme.textPrimary
}
