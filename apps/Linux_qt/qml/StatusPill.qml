// =============================================================================
// StatusPill.qml — 状态指示点组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：状态胶囊，用于展示同步状态等
// Issue #900：迁移到 Breeze/Kirigami 语义色，不再自绘 Material 3 胶囊，
// 内部只消费 Kirigami.Theme.* / Kirigami.Units.*，不再读 DesignTokens 字段。
// 约束：
//   - 公开 API 保持不变：dt / resolvedDt / status / text / pillColor / contentColor。
//   - dt 仍为 required（Issue #701/#715 语义：漏传就是调用错误），只是内部不再消费。
// =============================================================================

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: control

    // 调用方仍注入根 dt（保持公开 API 与构建期约定）。
    required property var dt
    readonly property var resolvedDt: dt

    property string status: "info"
    property string text: ""

    // 状态 → Breeze 语义色（success/warning/error/默认）。
    readonly property color _statusColor: {
        if (control.status === "success")
            return Kirigami.Theme.positiveTextColor
        if (control.status === "warning")
            return Kirigami.Theme.neutralTextColor
        if (control.status === "error")
            return Kirigami.Theme.negativeTextColor
        return Kirigami.Theme.highlightColor
    }

    // pillColor / contentColor 保持可写的公开属性，默认由 status 派生。
    property color pillColor: control._statusColor
    property color contentColor: Kirigami.Theme.backgroundColor

    implicitWidth: pillRow.implicitWidth + Kirigami.Units.largeSpacing * 2
    implicitHeight: pillRow.implicitHeight + Kirigami.Units.smallSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: control.pillColor

    RowLayout {
        id: pillRow
        anchors.centerIn: parent
        spacing: Kirigami.Units.smallSpacing

        Rectangle {
            Layout.alignment: Qt.AlignVCenter
            implicitWidth: Math.round(Kirigami.Units.smallSpacing)
            implicitHeight: Math.round(Kirigami.Units.smallSpacing)
            radius: width / 2
            color: control.contentColor
            opacity: 0.8
        }

        Controls.Label {
            text: control.text
            visible: control.text.length > 0
            color: control.contentColor
            verticalAlignment: Text.AlignVCenter
            font.bold: control.status === "error"
        }
    }
}
