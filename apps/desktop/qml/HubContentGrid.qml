// =============================================================================
// HubContentGrid.qml — Hub 内容网格
// =============================================================================
//
// 层级：Desktop UI 层（QML 基础组件）
// 职责：Hub 页面的内容网格布局，支持响应式卡片排列
// 约束：
//   - 纯布局组件，卡片通过 delegate 传入
//   - 使用 DesignTokens 统一间距
// =============================================================================

import QtQuick

Item {
    id: root
    property var dt: null
    property var model: null
    property Component delegate: null
    property int minCardWidth: 280
    property int cardHeight: 220
    property int gridGap: dt.gridGap
    property string emptyTitle: qsTr("暂无数据")
    property string emptySubtitle: ""
    property string emptyIcon: ""

    readonly property real viewportWidth: Math.max(1, gridView.width)
    readonly property int columns: {
        var available = viewportWidth
        return Math.max(1, Math.floor((available + gridGap) / (minCardWidth + gridGap)))
    }
    readonly property real cardWidth: {
        var allGap = (columns - 1) * gridGap
        return Math.floor((viewportWidth - allGap) / columns)
    }
    readonly property real cellWidth: cardWidth + gridGap
    readonly property real cellHeight: cardHeight + gridGap
    readonly property real contentGridWidth: columns * cardWidth + Math.max(0, columns - 1) * gridGap
    readonly property real horizontalPadding: Math.max(0, Math.floor((viewportWidth - contentGridWidth) / 2))

    GridView {
        id: gridView
        property var gridRoot: root
        anchors.fill: parent
        clip: true
        model: root.model
        delegate: root.delegate
        interactive: true
        cellWidth: root.cellWidth
        cellHeight: root.cellHeight
        leftMargin: root.horizontalPadding
        rightMargin: root.horizontalPadding
        topMargin: 0
        bottomMargin: 0
        boundsBehavior: Flickable.StopAtBounds
    }

    Item {
        anchors.fill: parent
        visible: !root.model || root.model.count === 0

        Column {
            anchors.centerIn: parent
            spacing: dt.sp8

            AppText {
                text: root.emptyIcon
                dt: root.dt
                font.pixelSize: 36
                visible: text.length > 0
                anchors.horizontalCenter: parent.horizontalCenter
            }
            AppText {
                text: root.emptyTitle
                color: dt.textPrimary
                font.pixelSize: dt.fontXl
                font.weight: Font.DemiBold
                anchors.horizontalCenter: parent.horizontalCenter
            }
            AppText {
                text: root.emptySubtitle
                color: dt.textSecondary
                font.pixelSize: dt.fontMd
                visible: text.length > 0
                anchors.horizontalCenter: parent.horizontalCenter
            }
        }
    }
}
