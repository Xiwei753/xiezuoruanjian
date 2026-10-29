// =============================================================================
// StarMapCard.qml — 星图卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个星图的卡片展示（标题、节点数、连线数、描述）
// 约束：
//   - 纯展示组件，数据通过 property 传入
//   - 点击和右键菜单通过 signal 传递给 StarMapPage
// Issue #900：卡片改用 Kirigami.ShadowedRectangle + Kirigami.Theme，
// 不再使用 AppText 等自制 M3 皮肤组件。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ShadowedRectangle {
    id: root
    required property var dt
    property var starmapData: ({})
    property bool isChild: false
    /// 首页拖放时由 DropArea 置位，仅用于高亮目标卡片。
    property bool dropActive: false

    signal clicked(string starmapId, string title)
    signal menuRequested(string starmapId, string title)

    width: isChild ? 200 : 260
    height: isChild ? 140 : 180
    radius: isChild ? Kirigami.Units.smallSpacing : Kirigami.Units.cornerRadius
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: (dropActive || hoverArea.containsMouse)
                  ? Kirigami.Theme.highlightColor
                  : Kirigami.Theme.disabledTextColor
    border.width: dropActive ? 2 : 1

    // 首页根星图拖动：拖过阈值后由 DragHandler 接管，MouseArea 不再触发点击；
    // 右键菜单仍由 MouseArea 处理。拖动只传递真实 starmapId，不改变 GridView 几何。
    Drag.active: dragHandler.active
    Drag.source: root
    Drag.dragType: Drag.Automatic
    Drag.mimeData: ({ "application/x-sujian-starmap-id": root.starmapData.starmapId || "" })
    Drag.supportedActions: Qt.MoveAction
    Drag.hotSpot.x: root.width / 2
    Drag.hotSpot.y: root.height / 2

    Behavior on border.color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: root.isChild ? Kirigami.Units.smallSpacing : Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        // Top row: icon + title + badge
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Kirigami.ShadowedRectangle {
                Layout.preferredWidth: isChild ? 32 : 40
                Layout.preferredHeight: isChild ? 32 : 40
                Layout.alignment: Qt.AlignTop
                radius: Kirigami.Units.smallSpacing
                color: {
                    var c = starmapData.accentColor || Kirigami.Theme.highlightColor;
                    return Qt.rgba(c.r, c.g, c.b, 0.15);
                }

                Kirigami.Icon {
                    anchors.centerIn: parent
                    width: isChild ? 18 : 22
                    height: isChild ? 18 : 22
                    source: "starred"
                    color: Kirigami.Theme.textColor
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                Label {
                    text: starmapData.title || qsTr("未命名星图")
                    color: Kirigami.Theme.textColor
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    maximumLineCount: 1
                }

                Label {
                    text: {
                        if (starmapData.isMainForProject) return qsTr("主星图");
                        if (starmapData.projectId) return qsTr("已绑定");
                        return qsTr("独立星图");
                    }
                    color: starmapData.isMainForProject ?
                           Kirigami.Theme.highlightColor :
                           Kirigami.Theme.disabledTextColor
                }
            }
        }

        // Stats row
        Row {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            Repeater {
                model: [
                    { label: qsTr("节点"), value: starmapData.nodeCount || 0 },
                    { label: qsTr("关系"), value: starmapData.edgeCount || 0 },
                    { label: qsTr("链接"), value: starmapData.linkedChapterCount || 0 }
                ]

                Column {
                    spacing: 1
                    visible: !root.isChild || index < 2

                    Label {
                        text: modelData.value
                        color: Kirigami.Theme.textColor
                        font.bold: true
                    }
                    Label {
                        text: modelData.label
                        color: Kirigami.Theme.disabledTextColor
                    }
                }
            }
        }

        Item { Layout.fillHeight: true }

        // Bottom row: time
        Row {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Item { Layout.fillWidth: true }

            Label {
                text: {
                    if (!starmapData.updatedAt) return "";
                    var d = new Date(starmapData.updatedAt);
                    var now = new Date();
                    var diff = now - d;
                    if (diff < 60000) return qsTr("刚刚");
                    if (diff < 3600000) return Math.floor(diff / 60000) + qsTr("分钟前");
                    if (diff < 86400000) return Math.floor(diff / 3600000) + qsTr("小时前");
                    return Math.floor(diff / 86400000) + qsTr("天前");
                }
                color: Kirigami.Theme.disabledTextColor
                Layout.alignment: Qt.AlignVCenter
            }
        }
    }

    DragHandler {
        id: dragHandler
        target: null
        acceptedButtons: Qt.LeftButton
        enabled: !root.isChild
    }

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: function(mouse) {
            var smId = root.starmapData.starmapId || "";
            if (mouse.button === Qt.LeftButton) {
                root.clicked(smId, root.starmapData.title || "");
            } else if (mouse.button === Qt.RightButton) {
                root.menuRequested(smId, root.starmapData.title || "");
            }
        }
    }
}
