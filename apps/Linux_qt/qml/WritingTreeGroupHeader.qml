// =============================================================================
// WritingTreeGroupHeader.qml — 卷分组头（Issue #829 手稿）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：渲染左树里卷的可折叠分组头 —— 「卷名 ∨ / 卷名 ∧ [+ 新章]」
//
// 边界：纯展示 + 一个 toggle 回调。展开态由调用方持有，
// 组件自己不存状态，也不查后端、不碰章节数据。
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root

    required property var dt

    // 分组标题（卷名）。
    property string title: ""
    // 当前是否展开。false 时画 ∧，true 时画 ∨。
    property bool expanded: true
    // 右侧是否带「+」入口（卷分组头带）。
    property bool showAddButton: false

    signal toggleExpanded()
    signal addRequested()

    readonly property real rowHeight: 36

    implicitHeight: rowHeight
    color: "transparent"

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: root.dt.sp8
        anchors.rightMargin: root.dt.sp8
        spacing: root.dt.sp6

        // 折叠箭头。手稿里就是一个细的 ∨ / ∧，不承担选中态。
        AppText {
            dt: root.dt
            text: root.expanded ? "⌄" : "⌃"
            color: root.dt.textSecondary
            font.pointSize: root.dt.fontSmPt
            font.family: root.dt.fontFamily
            Layout.alignment: Qt.AlignVCenter
        }

        AppText {
            dt: root.dt
            text: root.title
            color: root.dt.textPrimary
            font.pointSize: root.dt.labelPt
            font.family: root.dt.fontFamily
            font.weight: Font.DemiBold
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        Rectangle {
            visible: root.showAddButton
            width: 20; height: 20
            radius: 10
            color: addHover.containsMouse ? root.dt.primaryContainer : "transparent"
            Layout.alignment: Qt.AlignVCenter

            AppText {
                dt: root.dt
                anchors.centerIn: parent
                text: "+"
                color: root.dt.primary
                font.pointSize: root.dt.fontSmPt
                font.weight: Font.Bold
            }

            MouseArea {
                id: addHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.addRequested()
            }
        }
    }

    // 整行可点（点箭头或点空白都折叠），但不让「+」的点击冒泡过去。
    MouseArea {
        anchors.fill: parent
        anchors.rightMargin: root.showAddButton ? 28 : 0
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.toggleExpanded()
    }
}
