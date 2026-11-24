// =============================================================================
// StarMapTreeRow.qml — 星图左栏手绘静态目录行
// =============================================================================
//
// Issue #1048：目录行自己绘制缩进、展开箭头、标题、悬停/选中背景。
// 行只处理左键选择、右键菜单、箭头展开；不设置 drag.target，
// 也不挂任何可移动的 DragHandler / Drag 附加属性。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: row

    // ListView 委托通过必需属性 modelData 注入行快照；行内绘制与点击逻辑
    // 统一读取 rowData，避免各处直接依赖委托注入名。
    required property var modelData
    readonly property var rowData: modelData
    property bool selected: false

    signal activated()
    signal menuRequested()
    signal expansionToggled()

    readonly property bool navigable: String(rowData.rowKind || "") !== "node"
    readonly property bool hasArrow: navigable && rowData.hasChildren === true
    readonly property int rowDepth: Math.max(0, Number(rowData.depth || 0))

    implicitHeight: Kirigami.Units.gridUnit * 1.75
    color: selected ? Kirigami.Theme.highlightColor
                    : (rowMouse.containsMouse ? Kirigami.Theme.hoverColor
                                              : "transparent")

    MouseArea {
        id: rowMouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: function(mouse) {
            if (mouse.button === Qt.RightButton)
                row.menuRequested()
            else if (mouse.button === Qt.LeftButton)
                row.activated()
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing
                            + row.rowDepth * Kirigami.Units.gridUnit
        anchors.rightMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing / 2

        ToolButton {
            visible: row.hasArrow
            text: rowData.expanded ? "▾" : "▸"
            font.pixelSize: Kirigami.Units.gridUnit * 0.75
            implicitWidth: Kirigami.Units.gridUnit * 1.1
            implicitHeight: Kirigami.Units.gridUnit * 1.1
            flat: true
            onClicked: row.expansionToggled()
        }
        Item {
            visible: !row.hasArrow
            Layout.preferredWidth: Kirigami.Units.gridUnit * 1.1
            Layout.preferredHeight: 1
        }
        Label {
            Layout.fillWidth: true
            text: String(rowData.title || "")
            elide: Text.ElideRight
            font.pixelSize: Kirigami.Units.gridUnit * 0.85
            color: row.selected ? Kirigami.Theme.highlightedTextColor
                                : Kirigami.Theme.textColor
            ToolTip.visible: rowMouse.containsMouse
            ToolTip.text: String(rowData.title || "")
        }
    }
}
