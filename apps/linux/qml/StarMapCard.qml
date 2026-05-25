import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property var starmapData: ({})
    property bool isChild: false

    signal clicked(string starmapId, string title)
    signal menuRequested(string starmapId, string title)

    width: isChild ? 200 : 260
    height: isChild ? 140 : 180
    radius: dt ? (isChild ? dt.radiusMd : dt.radiusCard) : 12
    color: dt ? dt.card : "#1E2128"
    border.color: hoverArea.containsMouse ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
    border.width: 1

    Behavior on border.color { ColorAnimation { duration: dt ? dt.animFast : 120 } }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? (root.isChild ? dt.sp12 : dt.sp16) : 12
        spacing: dt ? dt.sp8 : 8

        // Top row: icon + title + badge
        RowLayout {
            Layout.fillWidth: true
            spacing: dt ? dt.sp8 : 8

            Rectangle {
                width: isChild ? 32 : 40
                height: isChild ? 32 : 40
                radius: dt ? dt.radiusSm : 8
                color: {
                    var c = starmapData.accent_color || (dt ? dt.accent : "#7B8CDE");
                    return Qt.rgba(Qt.color(c).r, Qt.color(c).g, Qt.color(c).b, 0.15);
                }
                Layout.alignment: Qt.AlignTop

                Text {
                    anchors.centerIn: parent
                    text: "\u2B50"
                    font.pixelSize: isChild ? 14 : 18
                }
            }

            Column {
                Layout.fillWidth: true
                spacing: 2

                Text {
                    text: starmapData.title || "未命名星图"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? (root.isChild ? dt.fontSm : dt.fontMd) : 14
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    maximumLineCount: 1
                }

                Text {
                    text: {
                        if (starmapData.is_main_for_project) return "主星图";
                        if (starmapData.project_id) return "已绑定";
                        return "独立星图";
                    }
                    color: starmapData.is_main_for_project ?
                           (dt ? dt.accent : "#7B8CDE") :
                           (dt ? dt.textMuted : "#606470")
                    font.pixelSize: dt ? dt.fontXs : 11
                }
            }
        }

        // Stats row
        Row {
            Layout.fillWidth: true
            spacing: dt ? dt.sp12 : 12

            Repeater {
                model: [
                    { label: "节点", value: starmapData.node_count || 0 },
                    { label: "关系", value: starmapData.edge_count || 0 },
                    { label: "链接", value: starmapData.linked_chapter_count || 0 }
                ]

                Column {
                    spacing: 1
                    visible: !root.isChild || index < 2

                    Text {
                        text: modelData.value
                        color: dt ? dt.textPrimary : "#E2E4E9"
                        font.pixelSize: dt ? (root.isChild ? dt.fontSm : dt.fontMd) : 14
                        font.weight: Font.Bold
                    }
                    Text {
                        text: modelData.label
                        color: dt ? dt.textMuted : "#606470"
                        font.pixelSize: dt ? dt.fontXs : 11
                    }
                }
            }
        }

        Item { Layout.fillHeight: true }

        // Bottom row: child count + time
        Row {
            Layout.fillWidth: true
            spacing: dt ? dt.sp8 : 8

            Rectangle {
                visible: (starmapData.child_starmap_count || 0) > 0
                width: childCountRow.implicitWidth + (dt ? dt.sp8 : 8)
                height: 20
                radius: dt ? dt.radiusSm : 8
                color: dt ? dt.accentSoft : "rgba(123,140,222,0.12)"

                Row {
                    id: childCountRow
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp4 : 4
                    Text {
                        text: "\u25BC"
                        color: dt ? dt.accentText : "#3D4D9E"
                        font.pixelSize: dt ? dt.fontXs : 11
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        text: (starmapData.child_starmap_count || 0) + " 子星图"
                        color: dt ? dt.accentText : "#3D4D9E"
                        font.pixelSize: dt ? dt.fontXs : 11
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }
            }

            Item { Layout.fillWidth: true }

            Text {
                text: {
                    if (!starmapData.updated_at) return "";
                    var d = new Date(starmapData.updated_at);
                    var now = new Date();
                    var diff = now - d;
                    if (diff < 60000) return "刚刚";
                    if (diff < 3600000) return Math.floor(diff / 60000) + "分钟前";
                    if (diff < 86400000) return Math.floor(diff / 3600000) + "小时前";
                    return Math.floor(diff / 86400000) + "天前";
                }
                color: dt ? dt.textMuted : "#606470"
                font.pixelSize: dt ? dt.fontXs : 11
                anchors.verticalCenter: parent.verticalCenter
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
            if (mouse.button === Qt.LeftButton) {
                root.clicked(root.starmapData.starmap_id || "", root.starmapData.title || "");
            } else if (mouse.button === Qt.RightButton) {
                root.menuRequested(root.starmapData.starmap_id || "", root.starmapData.title || "");
            }
        }
    }
}
