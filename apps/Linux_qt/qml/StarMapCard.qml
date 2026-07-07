// =============================================================================
// StarMapCard.qml — 星图卡片组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：单个星图的卡片展示（标题、节点数、连线数、描述）
// 约束：
//   - 纯展示组件，数据通过 property 传入
//   - 点击和右键菜单通过 signal 传递给 StarMapPage
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var starmapData: ({})
    property bool isChild: false

    signal clicked(string starmapId, string title)
    signal menuRequested(string starmapId, string title)

    width: isChild ? 200 : 260
    height: isChild ? 140 : 180
    radius: isChild ? dt.radiusMd : dt.radiusCard
    color: dt.card
    border.color: hoverArea.containsMouse ? dt.accent : dt.border
    border.width: 1

    Behavior on border.color { ColorAnimation { duration: dt.animFast } }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: root.isChild ? dt.sp12 : dt.sp16
        spacing: dt.sp8

        // Top row: icon + title + badge
        RowLayout {
            Layout.fillWidth: true
            spacing: dt.sp8

            Rectangle {
                width: isChild ? 32 : 40
                height: isChild ? 32 : 40
                radius: dt.radiusSm
                color: {
                    var c = starmapData.accentColor || dt.accent;
                    return c + "26"; // 15% opacity
                }
                Layout.alignment: Qt.AlignTop

                AppText {
                    anchors.centerIn: parent
                    text: "\u2B50"
                    dt: root.dt
                    font.pixelSize: isChild ? 14 : 18
                }
            }

            Column {
                Layout.fillWidth: true
                spacing: 2

                AppText {
                    dt: root.dt
                    text: starmapData.title || qsTr("未命名星图")
                    color: dt.textPrimary
                    font.pixelSize: root.isChild ? dt.fontSm : dt.fontMd
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    maximumLineCount: 1
                }

                AppText {
                    dt: root.dt
                    text: {
                        if (starmapData.isMainForProject) return qsTr("主星图");
                        if (starmapData.projectId) return qsTr("已绑定");
                        return qsTr("独立星图");
                    }
                    color: starmapData.isMainForProject ?
                           dt.accent :
                           dt.textMuted
                    font.pixelSize: dt.fontXs
                }
            }
        }

        // Stats row
        Row {
            Layout.fillWidth: true
            spacing: dt.sp12

            Repeater {
                model: [
                    { label: qsTr("节点"), value: starmapData.nodeCount || 0 },
                    { label: qsTr("关系"), value: starmapData.edgeCount || 0 },
                    { label: qsTr("链接"), value: starmapData.linkedChapterCount || 0 }
                ]

                Column {
                    spacing: 1
                    visible: !root.isChild || index < 2

                    AppText {
                        dt: root.dt
                        text: modelData.value
                        color: dt.textPrimary
                        font.pixelSize: root.isChild ? dt.fontSm : dt.fontMd
                        font.weight: Font.Bold
                    }
                    AppText {
                        dt: root.dt
                        text: modelData.label
                        color: dt.textMuted
                        font.pixelSize: dt.fontXs
                    }
                }
            }
        }

        Item { Layout.fillHeight: true }

        // Bottom row: child count + time
        Row {
            Layout.fillWidth: true
            spacing: dt.sp8

            Rectangle {
                visible: (starmapData.childStarmapCount || 0) > 0
                width: childCountRow.implicitWidth + dt.sp8
                height: 20
                radius: dt.radiusSm
                color: dt.accentSoft

                Row {
                    id: childCountRow
                    anchors.centerIn: parent
                    spacing: dt.sp4
                    AppText {
                        dt: root.dt
                        text: "\u25BC"
                        color: dt.accentText
                        font.pixelSize: dt.fontXs
                        Layout.alignment: Qt.AlignVCenter
                    }
                    AppText {
                        dt: root.dt
                        text: (starmapData.childStarmapCount || 0) + " " + qsTr("子星图")
                        color: dt.accentText
                        font.pixelSize: dt.fontXs
                        Layout.alignment: Qt.AlignVCenter
                    }
                }
            }

            Item { Layout.fillWidth: true }

            AppText {
                dt: root.dt
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
                color: dt.textMuted
                font.pixelSize: dt.fontXs
                Layout.alignment: Qt.AlignVCenter
            }
        }
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
