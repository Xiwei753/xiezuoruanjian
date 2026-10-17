// =============================================================================
// SectionHeader.qml — 分区标题组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：分区标题文本，支持主题适配
// Issue #900：迁移到 Breeze/Kirigami，使用 Kirigami.Heading + Kirigami.Theme.*，
// 不再继承自绘 AppText。
// 约束：
//   - 公开 API 保持不变：dt / variant / text（title 为 text 的别名）。
//   - 纯展示组件，dt 仍为 required 只是内部不再消费 DesignTokens 字段。
// =============================================================================

import QtQuick
import org.kde.kirigami as Kirigami

Kirigami.Heading {
    id: control

    // 调用方仍注入根 dt（保持公开 API 与构建期约定）。
    required property var dt
    // 兼容旧 AppText 的 variant 语义（secondary 用弱化文字色）。
    property string variant: "primary"
    // 旧调用方以 title 传文本；与 text 指向同一属性。
    property alias title: control.text

    level: 3
    color: control.variant === "secondary" ? Kirigami.Theme.disabledTextColor : Kirigami.Theme.textColor
}
