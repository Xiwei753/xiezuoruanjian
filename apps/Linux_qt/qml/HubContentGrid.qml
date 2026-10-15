// =============================================================================
// HubContentGrid.qml — Hub 内容网格
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：Hub 页面的内容网格布局，支持响应式卡片排列
// 约束：
//   - 纯布局组件，卡片通过 delegate 传入
// Issue #900：空状态文案/图标改用 Breeze（Kirigami.Heading / Controls.Label /
// Kirigami.Icon），间距默认取 Kirigami.Units。
// 公共 API（dt / dataModel / delegate / minCardWidth / cardHeight / gridGap /
// emptyTitle / emptySubtitle / emptyIcon / 一系列只读几何属性）保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root
    required property var dt
    property var dataModel: null
    property Component delegate: null
    property int minCardWidth: 280
    property int cardHeight: 220
    property int gridGap: Kirigami.Units.largeSpacing
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
        model: root.dataModel
        delegate: root.delegate
        interactive: true
        cellWidth: root.cellWidth
        cellHeight: root.cellHeight
        leftMargin: root.horizontalPadding
        rightMargin: root.horizontalPadding
        topMargin: 0
        bottomMargin: 0
        boundsBehavior: Flickable.StopAtBounds
        // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面，
        // 保留滚轮/触摸板/滚动条/触屏滚动。
        acceptedButtons: Qt.NoButton
    }

    Item {
        anchors.fill: parent
        // Issue #798: 兼容 JS 数组(.length)和 ListModel(.count)两种 model 类型。
        visible: !root.dataModel || root.dataModel.count === 0 || root.dataModel.length === 0

        ColumnLayout {
            anchors.centerIn: parent
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: root.emptyIcon
                visible: root.emptyIcon.length > 0
                implicitWidth: Kirigami.Units.iconSizes.huge
                implicitHeight: Kirigami.Units.iconSizes.huge
                Layout.alignment: Qt.AlignHCenter
            }
            Kirigami.Heading {
                level: 3
                text: root.emptyTitle
                Layout.alignment: Qt.AlignHCenter
            }
            Label {
                text: root.emptySubtitle
                color: Kirigami.Theme.disabledTextColor
                visible: text.length > 0
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }
}
