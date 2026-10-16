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

    // Issue #912：行高不再用固定 36，改为跟随标题字体实际高度 + KDE 间距派生，
    // 桌面默认字号下卷头/箭头/「+」入口不再偏小。
    readonly property real rowHeight: Math.max(Kirigami.Units.gridUnit * 2, titleMetrics.height + Kirigami.Units.smallSpacing * 2)

    implicitHeight: rowHeight

    TextMetrics {
        id: titleMetrics
        font: Kirigami.Theme.defaultFont
        text: root.title
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing
        anchors.rightMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        // 折叠箭头。手稿里就是一个细的箭头，不承担选中态。
        Kirigami.Icon {
            source: root.expanded ? "arrow-down" : "arrow-right"
            implicitWidth: Kirigami.Units.iconSizes.smallMedium
            implicitHeight: Kirigami.Units.iconSizes.smallMedium
            color: Kirigami.Theme.textColor
            Layout.alignment: Qt.AlignVCenter
        }

        Label {
            text: root.title
            color: Kirigami.Theme.textColor
            font.family: Kirigami.Theme.defaultFont.family
            font.pointSize: Kirigami.Theme.defaultFont.pointSize
            font.bold: true
            elide: Text.ElideRight
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
        }

        ToolButton {
            visible: root.showAddButton
            icon.name: "list-add"
            icon.width: Kirigami.Units.iconSizes.smallMedium
            icon.height: Kirigami.Units.iconSizes.smallMedium
            display: AbstractButton.IconOnly
            flat: true
            // Issue #912：给「+」入口完整的可点区域，不再用偏小的 gridUnit*1.4。
            implicitWidth: Math.max(Kirigami.Units.gridUnit * 1.8, Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing * 2)
            implicitHeight: Math.max(Kirigami.Units.gridUnit * 1.8, Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing * 2)
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
