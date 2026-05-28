// =============================================================================
// HubPageFrame.qml — Hub 页面框架
// =============================================================================
//
// 层级：Linux UI 层（QML 基础组件）
// 职责：Hub 页面的通用布局框架，提供响应式边距和最大宽度限制
// 约束：
//   - 纯布局组件，内容通过 default property 传入
//   - 自动适配宽屏/窄屏边距
// =============================================================================

import QtQuick 2.15
import QtQuick.Layouts 1.15

Item {
    id: root
    property var dt: null
    readonly property int pageMargin: width >= 980 ? (dt ? dt.pageMarginWide : 40) : (dt ? dt.pageMarginNarrow : 24)
    property int maxContentWidth: dt ? dt.maxContentWidth : 1240
    readonly property int contentWidth: Math.min(Math.max(1, width - pageMargin * 2), maxContentWidth)
    readonly property int sideMargin: Math.max(pageMargin, Math.floor((width - contentWidth) / 2))
    default property alias contentData: contentColumn.data
    property alias headerData: headerContainer.data

    ColumnLayout {
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.topMargin: root.pageMargin
        anchors.bottomMargin: dt ? dt.sp24 : 24
        anchors.leftMargin: root.sideMargin
        anchors.rightMargin: root.sideMargin
        spacing: dt ? dt.sp16 : 16

        Item {
            id: headerContainer
            Layout.fillWidth: true
            Layout.preferredHeight: dt ? dt.pageHeaderHeight : 72
        }

        ColumnLayout {
            id: contentColumn
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: dt ? dt.sp16 : 16
        }
    }
}
