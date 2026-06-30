// =============================================================================
// SettingsSection.qml — 设置分区组件
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：设置页面的分区容器，带标题和圆角背景
// 约束：
//   - 纯 UI 组件，设置行通过 default property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property string title: ""
    default property alias contentData: rows.data

    radius: dt.radiusLg
    color: dt.card
    border.color: dt.border
    border.width: 1

    readonly property int _sectionPadding: dt.sp20
    implicitHeight: contentCol.implicitHeight + _sectionPadding * 2

    ColumnLayout {
        id: contentCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: root._sectionPadding
        spacing: dt.sp12

        AppText {
            text: root.title
            color: dt.textPrimary
            font.pixelSize: dt.subtitle
            font.family: dt.fontFamily
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }

        ColumnLayout {
            id: rows
            Layout.fillWidth: true
            spacing: dt.sp12
        }
    }
}
