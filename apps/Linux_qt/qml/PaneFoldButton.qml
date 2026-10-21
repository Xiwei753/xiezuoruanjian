// =============================================================================
// PaneFoldButton.qml — 面板顶部折叠钮（Issue #829 手稿对齐 / Issue #900 KDE 迁移）
// =============================================================================
//
// 手稿基准：docs/ui/reference/widescreen/宽屏全打开.png + 左右缩回.png
//   - 展开态：正文区顶部居中一个 `∨`，右侧工具面板顶部一个 `∨`；
//   - 收起态：正文区顶部居中变成 `^`（左树收起后正文变宽，按钮位置不动）。
// 折叠钮只改 Qt UI 布局（pane 可见性），不进入 Core 编辑事务——
// 编辑会话、IME、撤销栈都不受影响。
//
// Issue #900：视觉从自绘 AppText + 三角字形迁移到 Breeze 图标。
//   glyph 属性作为方向语义保留（WritingWorkspace / RightDrawer 仍注入它），
//   内部据此选择 Breeze 的 arrow-down / arrow-up；dt 属性保留以兼容注入。
//
// 和贴边悬浮把手（WritingWorkspace 的 leftPaneHandle / rightPaneHandle）是
// 两个不同层级的入口：把手负责「收起后重新拉开」，本按钮负责「面板还开着时
// 主动收起」。手稿两者都画了，所以这里不删把手。
// =============================================================================

import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

ToolButton {
    id: root

    // 调用方注入的 DesignTokens（Issue #900 后不再读取，仅保留属性签名）。
    required property var dt

    // 手稿里手绘的三角：展开态朝下（可往下收），收起态朝上（可往上拉）。
    property string glyph: "\u2304" // ⌄

    // 方向语义由同一个 Breeze 箭头的旋转表达（不改变 glyph 的公有签名）。
    readonly property string _iconName: "arrow-down"

    signal triggered()

    implicitWidth: Kirigami.Units.gridUnit * 1.8
    implicitHeight: Kirigami.Units.gridUnit * 1.8
    flat: true
    display: AbstractButton.IconOnly
    icon.name: root._iconName
    icon.width: Kirigami.Units.iconSizes.small
    icon.height: Kirigami.Units.iconSizes.small

    contentItem: Item {
        Kirigami.Icon {
            anchors.centerIn: parent
            source: root._iconName
            width: root.icon.width
            height: root.icon.height
            color: Kirigami.Theme.textColor
            rotation: root.glyph === "\u2303" ? 180 : 0
            Behavior on rotation {
                NumberAnimation {
                    duration: 180
                    easing.type: Easing.OutCubic
                }
            }
        }
    }

    onClicked: root.triggered()
}
