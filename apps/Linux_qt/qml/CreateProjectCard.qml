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
//
// Issue #900：颜色/圆角迁移到 Kirigami.Theme / Kirigami.Units，「+」改用
// Breeze 图标 "list-add"，不再读 DesignTokens 的 M3 字段。
// 公共 API（dt / cardWidth / cardHeight / hovered / clicked）保持不变。
// =============================================================================

import QtQuick
import org.kde.kirigami as Kirigami

Rectangle {
    id: root

    required property var dt

    // 与 ProjectCard 共用同一组尺寸，由 ProjectHomePage 传入。
    property int cardWidth: 180
    property int cardHeight: 240

    property bool hovered: false
    property bool pressVisual: false

    signal clicked()

    width: cardWidth
    height: cardHeight
    radius: Kirigami.Units.cornerRadius
    // Issue #827 评论 5：和真实作品卡同面色。透明卡在浅灰页面上会退化成
    // 「很淡的空框 + 很淡的加号」，而草图里它和作品封面是同一等级的一张卡。
    color: hovered ? Kirigami.Theme.activeBackgroundColor : Kirigami.Theme.alternateBackgroundColor
    // 1px 实线边框：这是一张还没写上名字的封面，而不是一个工具按钮。
    border.color: hovered ? Kirigami.Theme.focusColor : Kirigami.Theme.disabledTextColor
    border.width: 1

    MotionTokens { id: motion }

    scale: pressVisual ? 0.985 : (hovered ? 1.01 : 1.0)

    Behavior on border.color { ColorAnimation { duration: motion.hoverMs } }
    Behavior on color { ColorAnimation { duration: motion.hoverMs } }
    Behavior on scale {
        NumberAnimation {
            duration: motion.hoverMs
            easing.type: Easing.OutCubic
        }
    }

    Kirigami.Icon {
        anchors.centerIn: parent
        source: "list-add"
        implicitWidth: Kirigami.Units.iconSizes.enormous
        implicitHeight: Kirigami.Units.iconSizes.enormous
        color: root.hovered ? Kirigami.Theme.highlightColor : Kirigami.Theme.textColor
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton
        onContainsMouseChanged: root.hovered = containsMouse
        onPressed: root.pressVisual = true
        onReleased: root.pressVisual = false
        onCanceled: root.pressVisual = false
        onClicked: function(mouse) {
            if (mouse.button === Qt.LeftButton) root.clicked();
        }
    }
}
