import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})

    property var todayStats: ({})
    property var weekStats: ({})
    property string currentDeviceId: ""

    color: dt ? dt.bg : "#111318"

    function loadStats() {
        if (!backendRef) return;
        try {
            var today = new Date();
            var todayStr = today.getFullYear() + "-" +
                String(today.getMonth() + 1).padStart(2, '0') + "-" +
                String(today.getDate()).padStart(2, '0');

            var weekStart = new Date(today);
            weekStart.setDate(today.getDate() - today.getDay());
            var weekStr = weekStart.getFullYear() + "-" +
                String(weekStart.getMonth() + 1).padStart(2, '0') + "-" +
                String(weekStart.getDate()).padStart(2, '0');

            var todayRaw = JSON.parse(backendRef.get_writing_stats_summary(todayStr, todayStr));
            todayStats = todayRaw || {};

            var weekRaw = JSON.parse(backendRef.get_writing_stats_summary(weekStr, todayStr));
            weekStats = weekRaw || {};
        } catch (e) {
            todayStats = {};
            weekStats = {};
        }
    }

    function getVal(obj, key, fallback) {
        if (obj && obj[key] !== undefined) return obj[key];
        return fallback;
    }

    function formatNum(n) {
        if (n >= 10000) return (n / 10000).toFixed(1) + "w";
        if (n >= 1000) return (n / 1000).toFixed(1) + "k";
        return String(n);
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp32 : 32
        spacing: 0

        Text {
            text: "统计"
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: dt ? dt.fontTitle : 26
            font.weight: Font.Bold
        }

        Text {
            Layout.fillWidth: true
            Layout.topMargin: dt ? dt.sp8 : 8
            text: "追踪你的写作节奏与习惯"
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontMd : 14
        }

        Item { Layout.preferredHeight: dt ? dt.sp24 : 24 }

        // Today stats cards
        Text {
            text: "今日"
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontSm : 12
            font.weight: Font.Medium
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: dt ? dt.sp12 : 12
            spacing: dt ? dt.sp12 : 12

            Repeater {
                model: ListModel {
                    ListElement { label: "纯输入"; key: "totalHumanTypedChars"; fallback: 0; color: "#7B8CDE" }
                    ListElement { label: "删除"; key: "totalDeletedChars"; fallback: 0; color: "#E06060" }
                    ListElement { label: "粘贴"; key: "totalPastedChars"; fallback: 0; color: "#E0A840" }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 100
                    radius: dt ? dt.radiusMd : 12
                    color: dt ? dt.card : "#1E2128"

                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: dt ? dt.sp16 : 16
                        spacing: dt ? dt.sp8 : 8

                        Rectangle {
                            width: 8; height: 8; radius: 4
                            color: model.color
                        }

                        Text {
                            text: formatNum(getVal(root.todayStats, model.key, model.fallback))
                            color: dt ? dt.textPrimary : "#E2E4E9"
                            font.pixelSize: dt ? dt.fontXxl : 22
                            font.weight: Font.Bold
                            Layout.topMargin: dt ? dt.sp8 : 8
                        }

                        Text {
                            text: model.label
                            color: dt ? dt.textMuted : "#606470"
                            font.pixelSize: dt ? dt.fontSm : 12
                        }
                    }
                }
            }
        }

        Item { Layout.preferredHeight: dt ? dt.sp24 : 24 }

        // Week stats
        Text {
            text: "本周"
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontSm : 12
            font.weight: Font.Medium
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: dt ? dt.sp12 : 12
            spacing: dt ? dt.sp12 : 12

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 80
                radius: dt ? dt.radiusMd : 12
                color: dt ? dt.card : "#1E2128"

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp16 : 16
                    spacing: dt ? dt.sp4 : 4

                    Text {
                        text: formatNum(getVal(root.weekStats, "totalHumanTypedChars", 0))
                        color: dt ? dt.textPrimary : "#E2E4E9"
                        font.pixelSize: dt ? dt.fontXxl : 22
                        font.weight: Font.Bold
                    }
                    Text {
                        text: "本周纯输入"
                        color: dt ? dt.textMuted : "#606470"
                        font.pixelSize: dt ? dt.fontSm : 12
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 80
                radius: dt ? dt.radiusMd : 12
                color: dt ? dt.card : "#1E2128"

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: dt ? dt.sp16 : 16
                    spacing: dt ? dt.sp4 : 4

                    Text {
                        text: "Linux"
                        color: dt ? dt.textPrimary : "#E2E4E9"
                        font.pixelSize: dt ? dt.fontLg : 16
                        font.weight: Font.Bold
                    }
                    Text {
                        text: "当前设备"
                        color: dt ? dt.textMuted : "#606470"
                        font.pixelSize: dt ? dt.fontSm : 12
                    }
                }
            }
        }

        Item { Layout.preferredHeight: dt ? dt.sp24 : 24 }

        // Speed curve placeholder
        Text {
            text: "速度曲线"
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontSm : 12
            font.weight: Font.Medium
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 180
            Layout.topMargin: dt ? dt.sp12 : 12
            radius: dt ? dt.radiusMd : 12
            color: dt ? dt.card : "#1E2128"

            ColumnLayout {
                anchors.centerIn: parent
                spacing: dt ? dt.sp8 : 8

                Text {
                    text: "\uD83D\uDCC8"
                    font.pixelSize: 32
                    Layout.alignment: Qt.AlignHCenter
                }
                Text {
                    text: "写作速度曲线将在后续版本实现"
                    color: dt ? dt.textMuted : "#606470"
                    font.pixelSize: dt ? dt.fontSm : 12
                    Layout.alignment: Qt.AlignHCenter
                }
            }
        }

        Item { Layout.fillHeight: true }
    }

    Component.onCompleted: loadStats()
    onBackendRefChanged: loadStats()
}
