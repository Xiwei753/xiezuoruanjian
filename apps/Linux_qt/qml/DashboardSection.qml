// =============================================================================
// DashboardSection.qml — 仪表盘分区组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：仪表盘页面的分区容器，带标题和圆角背景
// 约束：
//   - 纯 UI 组件，内容通过 default property 传入
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
    default property alias contentData: body.data

    radius: Kirigami.Units.cornerRadius
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: Kirigami.Theme.disabledTextColor
    border.width: 1
    implicitHeight: (contentCol.implicitHeight + Kirigami.Units.largeSpacing * 2)

    ColumnLayout {
        id: contentCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        Kirigami.Heading {
            level: 5
            text: root.title
            color: Kirigami.Theme.textColor
        }

        ColumnLayout {
            id: body
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
        }
    }
}
