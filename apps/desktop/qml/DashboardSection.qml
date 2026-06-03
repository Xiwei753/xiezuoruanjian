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

    radius: dt ? dt.radiusMd : 12
    color: dt ? dt.card : "#1E2128"
    border.color: dt ? dt.border : "#2A2E36"
    border.width: 1
    implicitHeight: (contentCol.implicitHeight + (dt ? dt.sp16 : 16) * 2)

    ColumnLayout {
        id: contentCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: dt ? dt.sp16 : 16
        spacing: dt ? dt.sp12 : 12

        AppText {
            text: root.title
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontSm : 12
            font.weight: Font.DemiBold
        }

        ColumnLayout {
            id: body
            Layout.fillWidth: true
            spacing: dt ? dt.sp8 : 8
        }
    }
}
