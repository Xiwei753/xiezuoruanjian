import QtQuick 2.15
import QtQuick.Layouts 1.15

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
    property string emptyTitle: "暂无数据"
    property string emptySubtitle: ""
    property string emptyIcon: ""
    signal actionClicked()

    readonly property real cardWidth: hubContent.cardWidth

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt

        headerData: [
            HubPageHeader {
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
