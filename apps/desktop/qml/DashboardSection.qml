// =============================================================================
// DashboardSection.qml — 仪表盘分区组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：仪表盘页面的分区容器，带标题和圆角背景
// 约束：
//   - 纯 UI 组件，内容通过 default property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property string title: ""
    default property alias contentData: body.data

    radius: dt.radiusMd
    color: dt.card
    border.color: dt.border
    border.width: 1
    implicitHeight: (contentCol.implicitHeight + dt.sp16 * 2)

    ColumnLayout {
        id: contentCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: dt.sp16
        spacing: dt.sp12

        AppText {
            text: root.title
            color: dt.textSecondary
            font.pixelSize: dt.fontSm
            font.weight: Font.DemiBold
        }

        ColumnLayout {
            id: body
            Layout.fillWidth: true
            spacing: dt.sp8
        }
    }
}
