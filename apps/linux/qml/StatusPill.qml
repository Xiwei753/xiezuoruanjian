// =============================================================================
// StatusPill.qml — 状态指示点组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：小型状态指示点，用于展示同步状态等
// 约束：
//   - 纯展示组件，颜色通过 pillColor property 传入
// =============================================================================

import QtQuick

Rectangle {
    id: control
    property var theme: null
    property string pillColor: control.theme ? control.theme.success : "#22c55e"

    width: 8
    height: 8
    radius: 4
    color: pillColor
}
