// =============================================================================
// HubPageFrame.qml — Hub 页面框架
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：Hub 页面的通用布局框架，提供响应式边距和最大宽度限制
// 约束：
//   - 纯布局组件，内容通过 default property 传入
//   - 自动适配宽屏/窄屏边距
// Issue #900：边距/间距改用 Kirigami.Units，不再读 DesignTokens 的 M3 间距。
// 公共 API（dt / pageMargin / maxContentWidth / contentWidth / sideMargin /
// contentData / headerData）保持不变，供 StarMapPage / StatsPreviewPage 复用。
// =============================================================================

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root
    required property var dt
    readonly property int pageMargin: width >= 980 ? Kirigami.Units.largeSpacing * 2 : Kirigami.Units.largeSpacing
    property int maxContentWidth: Kirigami.Units.gridUnit * 60
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
        anchors.bottomMargin: Kirigami.Units.largeSpacing
        anchors.leftMargin: root.sideMargin
        anchors.rightMargin: root.sideMargin
        spacing: Kirigami.Units.largeSpacing

        Item {
            id: headerContainer
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 4
        }

        ColumnLayout {
            id: contentColumn
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Kirigami.Units.largeSpacing
        }
    }
}
