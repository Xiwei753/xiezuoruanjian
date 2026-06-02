// =============================================================================
// SettingsSection.qml — 设置分区组件
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
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

    radius: dt ? dt.radiusLg : 16
    color: dt ? dt.card : "#1E2128"
    border.color: dt ? dt.border : "#2A2E36"
    border.width: 1

    readonly property int _sectionPadding: dt ? dt.sp20 : 20
    implicitHeight: contentCol.implicitHeight + _sectionPadding * 2

    ColumnLayout {
        id: contentCol
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: root._sectionPadding
        spacing: dt ? dt.sp12 : 12

        AppText {
            text: root.title
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: dt ? dt.subtitle : 18
            font.family: dt ? dt.fontFamily : "sans-serif"
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }

        ColumnLayout {
            id: rows
            Layout.fillWidth: true
            spacing: dt ? dt.sp12 : 12
        }
    }
}
