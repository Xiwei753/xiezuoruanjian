// =============================================================================
// CreateProjectCard.qml — 作品网格里的「+」卡
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只画一个居中的大「+」，点击后请求宿主打开命名框
// 约束：
//   - 纯展示组件，实际宽高由 ProjectHomePage 从作品卡的同一组属性传入，
//     自己不另定一套尺寸（草图里它和作品卡同尺寸同间距）
//   - 不直接创建作品：点击只发 clicked 信号，真正的命名与创建在 ProjectHomePage / main.qml
//
// Issue #827 评论 2：「+」不是右上按钮、不是 FAB、也不是底栏按钮，
// 它就是作品网格中的一张卡 —— 一个虚拟作品封面：
//   - 跟真实作品一起排版、一起换行、一起滚动
//   - 永远跟在最后一张作品卡后面；没有作品时它就是第一张卡
//   - 点击只打开命名框，输入名称后才真正创建作品
// =============================================================================

import QtQuick

Rectangle {
    id: root

    required property var dt

    // 与 ProjectCard 共用同一组尺寸，由 ProjectHomePage 传入。
    property int cardWidth: 180
    property int cardHeight: 240

    property bool hovered: false

    signal clicked()

    width: cardWidth
    height: cardHeight
    radius: dt.cardRadius
    color: "transparent"
    // 虚线边框：这是一张还没写上名字的封面，而不是一个工具按钮。
    border.color: hovered ? dt.primary : dt.border
    border.width: 1

    Behavior on border.color { ColorAnimation { duration: dt.animFast } }
    Behavior on color { ColorAnimation { duration: dt.animFast } }

    AppText {
        anchors.centerIn: parent
        text: "+"
        color: root.hovered ? root.dt.primary : root.dt.textMuted
        font.pointSize: 44
        font.family: root.dt.fontFamily
        font.weight: Font.Normal
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton
        onContainsMouseChanged: root.hovered = containsMouse
        onClicked: function(mouse) {
            if (mouse.button === Qt.LeftButton) root.clicked();
        }
    }
}