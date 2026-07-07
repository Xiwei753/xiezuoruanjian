// =============================================================================
// CardCollectionPage.qml — 卡片集合页面模板
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：通用的卡片网格布局模板，支持标题、副标题和空状态展示
// 约束：
//   - 纯布局模板，卡片内容通过 delegate 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Item {
    id: root
    property var dt: null
    property string title: ""
    property string subtitle: ""
    property string actionText: ""
    property var model: null
    property Component delegate: null
    property int cardHeight: 220
    property int minCardWidth: 280
    property string emptyTitle: qsTr("暂无数据")
    property string emptySubtitle: ""
    property string emptyIcon: ""
    signal actionClicked()

    readonly property real cardWidth: hubContent.cardWidth
    readonly property int gridGap: hubContent.gridGap
    readonly property real cardHeightResolved: hubContent.cardHeight

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt

        headerData: [
            HubPageHeader {
                anchors.fill: parent
                dt: root.dt
                title: root.title
                subtitle: root.subtitle
                actionText: root.actionText
                onActionClicked: root.actionClicked()
            }
        ]

        HubContentGrid {
            id: hubContent
            dt: root.dt
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: root.model
            delegate: root.delegate
            cardHeight: root.cardHeight
            minCardWidth: root.minCardWidth
            emptyTitle: root.emptyTitle
            emptySubtitle: root.emptySubtitle
            emptyIcon: root.emptyIcon
        }
    }
}
