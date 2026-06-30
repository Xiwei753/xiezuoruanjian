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

    // ── SystemPalette 推断：dt 为空时从系统调色板推断深浅色 ──
    SystemPalette { id: _sysPalette; colorGroup: SystemPalette.Active }
    readonly property bool _inferDark: {
        var wL = _sysPalette.window.r * 0.2126 + _sysPalette.window.g * 0.7152 + _sysPalette.window.b * 0.0722;
        var tL = _sysPalette.windowText.r * 0.2126 + _sysPalette.windowText.g * 0.7152 + _sysPalette.windowText.b * 0.0722;
        return tL > wL;
    }

    radius: dt ? dt.radiusLg : 16
    color: dt ? dt.card : (_inferDark ? "#1E2128" : "#F6F8FC")
    border.color: dt ? dt.border : (_inferDark ? "#2A2E36" : "#CBD5E1")
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
