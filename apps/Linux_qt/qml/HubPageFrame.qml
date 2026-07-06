// =============================================================================
// HubPageFrame.qml — Hub 页面框架
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：Hub 页面的通用布局框架，提供响应式边距和最大宽度限制
// 约束：
//   - 纯布局组件，内容通过 default property 传入
//   - 自动适配宽屏/窄屏边距
// =============================================================================

import QtQuick
import QtQuick.Layouts

Item {
    id: root
    property var dt: null
    readonly property int pageMargin: width >= 980 ? dt.pageMarginWide : dt.pageMarginNarrow
    property int maxContentWidth: dt.maxContentWidth
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
        anchors.bottomMargin: dt.sp24
        anchors.leftMargin: root.sideMargin
        anchors.rightMargin: root.sideMargin
        spacing: dt.sp16

        Item {
            id: headerContainer
            Layout.fillWidth: true
            Layout.preferredHeight: dt.pageHeaderHeight
        }

        ColumnLayout {
            id: contentColumn
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: dt.sp16
        }
    }
}
