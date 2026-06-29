// =============================================================================
// SettingCard.qml — 设置卡片组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：设置页面的卡片容器，带标题和圆角背景
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
    default property alias contentData: contentColumn.data
    radius: dt ? dt.cardRadius : dt.radiusCard
    color: dt.surfaceContainerLow
    border.color: dt.border
    border.width: 1

    implicitHeight: contentColumn.implicitHeight + (dt ? dt.sp32 : 32)

    ColumnLayout {
        id: contentColumn
        anchors.fill: parent
        anchors.margins: dt ? dt.sp20 : 20
        spacing: dt ? dt.sp16 : 16

        AppText {
            text: root.title
            color: dt.accent
            font.pixelSize: dt ? dt.fontMd : 14
            font.weight: Font.Bold
        }
    }
}
