// =============================================================================
// StatCard.qml — 统计卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：展示单个统计数据的卡片（标题、数值、说明）
// 约束：
//   - 纯展示组件，数据通过 property 传入
// Issue #900：改用 Kirigami.ShadowedRectangle + Kirigami.Theme，不再使用
// AppText 等自制 M3 皮肤组件；公开属性保持兼容。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ShadowedRectangle {
    id: root
    required property var dt
    property string title: ""
    property string value: "0"
    property string caption: ""
    property color tone: Kirigami.Theme.highlightColor
    radius: Kirigami.Units.cornerRadius
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: Kirigami.Theme.disabledTextColor
    border.width: 1

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        Label {
            text: root.title
            color: Kirigami.Theme.disabledTextColor
            Layout.fillWidth: true
            elide: Text.ElideRight
        }
        Kirigami.Heading {
            level: 3
            text: root.value
            color: root.tone
            Layout.fillWidth: true
        }
        Label {
            text: root.caption
            color: Kirigami.Theme.disabledTextColor
            Layout.fillWidth: true
            elide: Text.ElideRight
        }
    }
}
