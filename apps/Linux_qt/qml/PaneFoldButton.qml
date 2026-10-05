// =============================================================================
// PaneFoldButton.qml — 面板顶部折叠钮（Issue #829 手稿对齐）
// =============================================================================
//
// 手稿基准：docs/ui/reference/widescreen/宽屏全打开.png + 左右缩回.png
//   - 展开态：正文区顶部居中一个 `∨`，右侧工具面板顶部一个 `∨`；
//   - 收起态：正文区顶部居中变成 `^`（左树收起后正文变宽，按钮位置不动）。
// 折叠钮只改 Qt UI 布局（SplitView 子项可见性），不进入 Core 编辑事务——
// 编辑会话、IME、撤销栈都不受影响。
//
// 和贴边悬浮把手（WritingWorkspace 的 leftPaneHandle / rightPaneHandle）是
// 两个不同层级的入口：把手负责「收起后重新拉开」，本按钮负责「面板还开着时
// 主动收起」。手稿两者都画了，所以这里不删把手。
// =============================================================================

import QtQuick

Rectangle {
    id: root

    required property var dt

    // 手稿里手绘的三角：展开态朝下（可往下收），收起态朝上（可往上拉）。
    property string glyph: "\u2304" // ⌄

    implicitWidth: 28
    implicitHeight: 28
    radius: dt.radiusPill
    color: hovered ? dt.surfaceVariant : "transparent"
    border.color: hovered ? dt.border : "transparent"
    border.width: 1

    signal triggered()

    // 悬停态显式提上来：id 只在声明它的 MouseArea 作用域内可见，
    // 外层 Rectangle / AppText 不能通过 root.hover 读它。
    readonly property bool hovered: hoverArea.containsMouse

    Behavior on color { ColorAnimation { duration: dt.animFast } }

    AppText {
        dt: root.dt
        anchors.centerIn: parent
        text: root.glyph
        color: root.hovered ? dt.textPrimary : dt.textSecondary
        font.pointSize: root.dt.fontSmPt
    }

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.triggered()
    }
}
