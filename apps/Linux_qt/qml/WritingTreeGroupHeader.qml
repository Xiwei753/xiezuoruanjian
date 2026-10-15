// =============================================================================
// WritingTreeGroupHeader.qml — 卷分组头（Issue #829 手稿 / Issue #900 KDE 迁移）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：渲染左树里卷的可折叠分组头 —— 「卷名 ∨ / 卷名 ∧ [+ 新章]」
//
// 边界：纯展示 + 一个 toggle 回调。展开态由调用方持有，
// 组件自己不存状态，也不查后端、不碰章节数据。
//
// Issue #900：折叠箭头 / 新增入口改用 Breeze 图标（arrow-down|arrow-right、list-add），
// 颜色 / 间距走 Kirigami.Theme / Kirigami.Units。dt 属性保留以兼容注入。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    required property var dt

    // 分组标题（卷名）。
    property string title: ""
    // 当前是否展开。false 时箭头朝右，true 时箭头朝下。
    property bool expanded: true
    // 右侧是否带「+」入口（卷分组头带）。
    property bool showAddButton: false

    signal toggleExpanded()
    signal addRequested()

    readonly property real rowHeight: 36

    implicitHeight: rowHeight

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing
        anchors.rightMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        // 折叠箭头。手稿里就是一个细的箭头，不承担选中态。
        Kirigami.Icon {
            source: root.expanded ? "arrow-down" : "arrow-right"
            implicitWidth: Kirigami.Units.iconSizes.small
            implicitHeight: Kirigami.Units.iconSizes.small
            color: Kirigami.Theme.textColor
            Layout.alignment: Qt.AlignVCenter
        }

        Label {
            text: root.title
            color: Kirigami.Theme.textColor
            font.bold: true
            elide: Text.ElideRight
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
        }

        ToolButton {
            visible: root.showAddButton
            icon.name: "list-add"
            icon.width: Kirigami.Units.iconSizes.small
            icon.height: Kirigami.Units.iconSizes.small
            display: AbstractButton.IconOnly
            flat: true
            implicitWidth: Kirigami.Units.gridUnit * 1.4
            implicitHeight: Kirigami.Units.gridUnit * 1.4
            Layout.alignment: Qt.AlignVCenter
            onClicked: root.addRequested()
        }
    }

    // 整行可点（点箭头或点空白都折叠），但不让「+」的点击冒泡过去。
    MouseArea {
        anchors.fill: parent
        anchors.rightMargin: root.showAddButton ? Math.round(Kirigami.Units.gridUnit * 2) : 0
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.toggleExpanded()
    }
}
