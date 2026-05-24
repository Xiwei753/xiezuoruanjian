import QtQuick 2.15
import QtQuick.Controls 2.15

Item {
    id: root
    property var dt: null
    property var model: null
    property Component delegate: null
    property int minCardWidth: 280
    property int cardHeight: 220
    property int gridGap: dt ? dt.gridGap : 16
    property string emptyTitle: "暂无数据"
    property string emptySubtitle: ""
    property string emptyIcon: ""

    readonly property int columns: {
        var available = Math.max(1, width)
        return Math.max(1, Math.floor((available + gridGap) / (minCardWidth + gridGap)))
    }
    readonly property real cardWidth: {
        var allGap = (columns - 1) * gridGap
        return Math.floor((Math.max(1, width) - allGap) / columns)
    }
    readonly property real cellWidth: cardWidth + gridGap

    ScrollView {
        anchors.fill: parent
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        GridView {
            id: gridView
            width: root.width
            implicitHeight: contentHeight
            model: root.model
            delegate: root.delegate
            interactive: false
            cellWidth: root.cellWidth
            cellHeight: root.cardHeight + root.gridGap
            boundsBehavior: Flickable.StopAtBounds
        }
    }

    Item {
        anchors.fill: parent
        visible: !root.model || root.model.count === 0

        Column {
            anchors.centerIn: parent
            spacing: dt ? dt.sp8 : 8

            Text {
                text: root.emptyIcon
                font.pixelSize: 36
                visible: text.length > 0
                anchors.horizontalCenter: parent.horizontalCenter
            }
            Text {
                text: root.emptyTitle
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontXl : 18
                font.weight: Font.DemiBold
                anchors.horizontalCenter: parent.horizontalCenter
            }
            Text {
                text: root.emptySubtitle
                color: dt ? dt.textSecondary : "#9CA0AB"
                font.pixelSize: dt ? dt.fontMd : 14
                visible: text.length > 0
                anchors.horizontalCenter: parent.horizontalCenter
            }
        }
    }
}
