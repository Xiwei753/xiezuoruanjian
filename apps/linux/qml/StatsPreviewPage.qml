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
    property var monthStats: ({})
    property var projectStats: []
    property var chapterStats: []
    property var deviceStats: []
    property var speedCurve: []
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

            var monthStart = new Date(today.getFullYear(), today.getMonth(), 1);
            var monthStr = monthStart.getFullYear() + "-" +
                String(monthStart.getMonth() + 1).padStart(2, '0') + "-" +
                String(monthStart.getDate()).padStart(2, '0');

            // Today
            var todayRaw = backendRef.get_writing_stats_summary(todayStr, todayStr);
            todayStats = JSON.parse(todayRaw) || {};

            // This week
            var weekRaw = backendRef.get_writing_stats_summary(weekStr, todayStr);
            weekStats = JSON.parse(weekRaw) || {};

            // This month
            var monthRaw = backendRef.get_writing_stats_summary(monthStr, todayStr);
            monthStats = JSON.parse(monthRaw) || {};

            // By project
            var projRaw = backendRef.get_writing_stats_by_project(weekStr, todayStr);
            var projObj = JSON.parse(projRaw) || {};
            projectStats = projObj.projects || [];
            projectStats.sort(function(a, b) {
                return (b.human_typed_chars || 0) - (a.human_typed_chars || 0);
            });

            // By chapter
            var chapRaw = backendRef.get_writing_stats_by_chapter(weekStr, todayStr);
            var chapObj = JSON.parse(chapRaw) || {};
            chapterStats = chapObj.chapters || [];
            chapterStats.sort(function(a, b) {
                return (b.human_typed_chars || 0) - (a.human_typed_chars || 0);
            });

            // By device
            var devRaw = backendRef.get_writing_stats_by_device(weekStr, todayStr);
            var devObj = JSON.parse(devRaw) || {};
            deviceStats = devObj.devices || [];

            // Speed curve (7 days, 60-min buckets)
            var speedRaw = backendRef.get_writing_speed_curve(weekStr, todayStr, 60);
            var speedObj = JSON.parse(speedRaw) || {};
            speedCurve = speedObj.buckets || [];

        } catch (e) {
            todayStats = {};
            weekStats = {};
            monthStats = {};
            projectStats = [];
            chapterStats = [];
            deviceStats = [];
            speedCurve = [];
        }
    }

    function getVal(obj, key, fallback) {
        if (obj && obj[key] !== undefined && obj[key] !== null) return obj[key];
        return fallback;
    }

    function formatNum(n) {
        if (n === undefined || n === null) return "0";
        n = Number(n);
        if (n >= 10000) return (n / 10000).toFixed(1) + "w";
        if (n >= 1000) return (n / 1000).toFixed(1) + "k";
        return String(n);
    }

    function formatNumFull(n) {
        if (n === undefined || n === null) return "0";
        n = Number(n);
        return n.toLocaleString();
    }

    function getProjectTitle(projectId) {
        if (!appState || !appState.tree) return projectId;
        for (var i = 0; i < appState.tree.length; i++) {
            if (appState.tree[i].id === projectId && appState.tree[i].type === "project") {
                return appState.tree[i].title;
            }
        }
        return projectId;
    }

    function getChapterTitle(chapterId) {
        if (!appState || !appState.tree) return chapterId;
        for (var i = 0; i < appState.tree.length; i++) {
            if (appState.tree[i].id === chapterId && appState.tree[i].type === "chapter") {
                return appState.tree[i].title;
            }
        }
        return chapterId;
    }

    function formatDuration(seconds) {
        if (!seconds || seconds <= 0) return "0分";
        var h = Math.floor(seconds / 3600);
        var m = Math.floor((seconds % 3600) / 60);
        if (h > 0) return h + "时" + m + "分";
        return m + "分";
    }

    Component.onCompleted: loadStas()
    onVisibleChanged: { if (visible) loadStats(); }

    function loadStas() { loadStats(); }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp24 : 24
        spacing: 0

        // Header
        Text {
            text: "统计"
            color: dt ? dt.textPrimary : "#E2E4E9"
            font.pixelSize: dt ? dt.fontTitle : 26
            font.weight: Font.Bold
        }
        Text {
            Layout.fillWidth: true
            Layout.topMargin: dt ? dt.sp4 : 4
            text: "追踪你的写作节奏与习惯"
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontMd : 14
        }

        Item { Layout.preferredHeight: dt ? dt.sp20 : 20 }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

            Column {
                width: parent ? parent.width : 0
                spacing: dt ? dt.sp24 : 24

                // === Today ===
                Text {
                    text: "今日"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Medium
                }

                Grid {
                    columns: 4
                    spacing: dt ? dt.sp12 : 12
                    width: parent.width

                    Repeater {
                        model: ListModel {
                            ListElement { label: "纯输入"; key: "total_human_typed_chars"; fallback: 0; color: "#7B8CDE" }
                            ListElement { label: "删除"; key: "total_deleted_chars"; fallback: 0; color: "#E06060" }
                            ListElement { label: "粘贴"; key: "total_pasted_chars"; fallback: 0; color: "#E0A840" }
                            ListElement { label: "净增"; key: "total_net_delta_chars"; fallback: 0; color: "#5CB880" }
                        }

                        Rectangle {
                            width: (parent.width - dt.sp12 * 3) / 4
                            height: 90
                            radius: dt ? dt.radiusMd : 12
                            color: dt ? dt.card : "#1E2128"

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: dt ? dt.sp12 : 12
                                spacing: dt ? dt.sp4 : 4

                                Rectangle {
                                    width: 8; height: 8; radius: 4
                                    color: model.color
                                }

                                Text {
                                    text: formatNum(getVal(root.todayStats, model.key, model.fallback))
                                    color: dt ? dt.textPrimary : "#E2E4E9"
                                    font.pixelSize: dt ? dt.fontXxl : 22
                                    font.weight: Font.Bold
                                    Layout.topMargin: dt ? dt.sp4 : 4
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

                // === This week ===
                Text {
                    text: "本周"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Medium
                }

                Grid {
                    columns: 3
                    spacing: dt ? dt.sp12 : 12
                    width: parent.width

                    Rectangle {
                        width: (parent.width - dt.sp12 * 2) / 3
                        height: 80
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: dt ? dt.sp12 : 12
                            spacing: dt ? dt.sp4 : 4

                            Text {
                                text: formatNum(getVal(root.weekStats, "total_human_typed_chars", 0))
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
                        width: (parent.width - dt.sp12 * 2) / 3
                        height: 80
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: dt ? dt.sp12 : 12
                            spacing: dt ? dt.sp4 : 4

                            Text {
                                text: formatNum(getVal(root.weekStats, "total_net_delta_chars", 0))
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontXxl : 22
                                font.weight: Font.Bold
                            }
                            Text {
                                text: "本周净增"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }
                    }

                    Rectangle {
                        width: (parent.width - dt.sp12 * 2) / 3
                        height: 80
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: dt ? dt.sp12 : 12
                            spacing: dt ? dt.sp4 : 4

                            Text {
                                text: formatDuration(getVal(root.weekStats, "total_active_seconds", 0))
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontXxl : 22
                                font.weight: Font.Bold
                            }
                            Text {
                                text: "本周写作时长"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }
                    }
                }

                // === This month ===
                Text {
                    text: "本月"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Medium
                }

                Rectangle {
                    width: parent.width
                    height: 80
                    radius: dt ? dt.radiusMd : 12
                    color: dt ? dt.card : "#1E2128"

                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: dt ? dt.sp16 : 16
                        spacing: dt ? dt.sp32 : 32

                        Column {
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: formatNum(getVal(root.monthStats, "total_human_typed_chars", 0))
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontXxl : 22
                                font.weight: Font.Bold
                            }
                            Text {
                                text: "本月纯输入"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }

                        Column {
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: formatNum(getVal(root.monthStats, "total_net_delta_chars", 0))
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontXxl : 22
                                font.weight: Font.Bold
                            }
                            Text {
                                text: "本月净增"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }

                        Column {
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: formatDuration(getVal(root.monthStats, "total_active_seconds", 0))
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontXxl : 22
                                font.weight: Font.Bold
                            }
                            Text {
                                text: "本月写作时长"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }
                    }
                }

                // === Speed curve ===
                Text {
                    text: "速度曲线"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Medium
                }

                Rectangle {
                    width: parent.width
                    height: 140
                    radius: dt ? dt.radiusMd : 12
                    color: dt ? dt.card : "#1E2128"

                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: dt ? dt.sp16 : 16
                        visible: speedCurve.length > 0

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: dt ? dt.sp8 : 8

                            Repeater {
                                model: Math.min(speedCurve.length, 14)

                                Rectangle {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    color: "transparent"

                                    property var bucket: speedCurve[speedCurve.length - 1 - index] || ({})
                                    property real cpm: bucket.chars_per_minute || 0
                                    property real maxCpm: {
                                        var mx = 1;
                                        for (var i = 0; i < speedCurve.length; i++) {
                                            if ((speedCurve[i].chars_per_minute || 0) > mx) mx = speedCurve[i].chars_per_minute;
                                        }
                                        return mx;
                                    }

                                    Rectangle {
                                        anchors.bottom: parent.bottom
                                        width: parent.width
                                        height: Math.max(2, (cpm / maxCpm) * (parent.height - 20))
                                        radius: 2
                                        color: cpm > 0 ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
                                        opacity: cpm > 0 ? 0.7 : 0.3
                                    }
                                }
                            }
                        }

                        Text {
                            text: "最近 14 小时输入速度"
                            color: dt ? dt.textMuted : "#606470"
                            font.pixelSize: dt ? dt.fontXs : 11
                            Layout.alignment: Qt.AlignRight
                        }
                    }

                    // Empty state
                    ColumnLayout {
                        anchors.centerIn: parent
                        spacing: dt ? dt.sp8 : 8
                        visible: speedCurve.length === 0

                        Text {
                            text: "\uD83D\uDCC8"
                            font.pixelSize: 24
                            Layout.alignment: Qt.AlignHCenter
                        }
                        Text {
                            text: "开始写作后将显示速度曲线"
                            color: dt ? dt.textMuted : "#606470"
                            font.pixelSize: dt ? dt.fontSm : 12
                            Layout.alignment: Qt.AlignHCenter
                        }
                    }
                }

                // === Project ranking ===
                Text {
                    text: "按作品排行 (本周)"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Medium
                    visible: projectStats.length > 0
                }

                Repeater {
                    model: Math.min(projectStats.length, 5)

                    Rectangle {
                        width: parent.width
                        height: 56
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: dt ? dt.sp16 : 16
                            anchors.rightMargin: dt ? dt.sp16 : 16
                            spacing: dt ? dt.sp12 : 12

                            Rectangle {
                                width: 28; height: 28
                                radius: 14
                                color: index === 0 ? "#E0A840" : index === 1 ? "#9CA0AB" : index === 2 ? "#CD7F32" : (dt ? dt.border : "#2A2E36")

                                Text {
                                    anchors.centerIn: parent
                                    text: String(index + 1)
                                    color: index < 3 ? "#FFFFFF" : (dt ? dt.textMuted : "#606470")
                                    font.pixelSize: dt ? dt.fontSm : 12
                                    font.weight: Font.Bold
                                }
                            }

                            Text {
                                text: getProjectTitle(projectStats[index].project_id || "")
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                            }

                            Text {
                                text: formatNum(projectStats[index].human_typed_chars || 0)
                                color: dt ? dt.accent : "#7B8CDE"
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.weight: Font.Bold
                            }

                            Text {
                                text: "字"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }
                    }
                }

                // === Chapter ranking ===
                Text {
                    text: "按章节排行 (本周)"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Medium
                    visible: chapterStats.length > 0
                }

                Repeater {
                    model: Math.min(chapterStats.length, 5)

                    Rectangle {
                        width: parent.width
                        height: 56
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: dt ? dt.sp16 : 16
                            anchors.rightMargin: dt ? dt.sp16 : 16
                            spacing: dt ? dt.sp12 : 12

                            Rectangle {
                                width: 28; height: 28
                                radius: 14
                                color: index === 0 ? "#E0A840" : index === 1 ? "#9CA0AB" : index === 2 ? "#CD7F32" : (dt ? dt.border : "#2A2E36")

                                Text {
                                    anchors.centerIn: parent
                                    text: String(index + 1)
                                    color: index < 3 ? "#FFFFFF" : (dt ? dt.textMuted : "#606470")
                                    font.pixelSize: dt ? dt.fontSm : 12
                                    font.weight: Font.Bold
                                }
                            }

                            Text {
                                text: getChapterTitle(chapterStats[index].chapter_id || "")
                                color: dt ? dt.textPrimary : "#E2E4E9"
                                font.pixelSize: dt ? dt.fontMd : 14
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                            }

                            Text {
                                text: formatNum(chapterStats[index].human_typed_chars || 0)
                                color: dt ? dt.accent : "#7B8CDE"
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.weight: Font.Bold
                            }

                            Text {
                                text: "字"
                                color: dt ? dt.textMuted : "#606470"
                                font.pixelSize: dt ? dt.fontSm : 12
                            }
                        }
                    }
                }

                // === Device stats ===
                Text {
                    text: "设备统计"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontSm : 12
                    font.weight: Font.Medium
                    visible: deviceStats.length > 0
                }

                Repeater {
                    model: deviceStats

                    Rectangle {
                        width: parent.width
                        height: 56
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: dt ? dt.sp16 : 16
                            anchors.rightMargin: dt ? dt.sp16 : 16
                            spacing: dt ? dt.sp12 : 12

                            Rectangle {
                                width: 28; height: 28
                                radius: 14
                                color: dt ? dt.accentSoft : "rgba(123,140,222,0.12)"

                                Text {
                                    anchors.centerIn: parent
                                    text: modelData.platform === "Linux" ? "\uD83D\uDCBB" : "\uD83D\uDCF1"
                                    font.pixelSize: dt ? dt.fontSm : 12
                                }
                            }

                            Column {
                                Layout.fillWidth: true
                                spacing: 2
                                Text {
                                    text: modelData.platform || modelData.device_id || "未知设备"
                                    color: dt ? dt.textPrimary : "#E2E4E9"
                                    font.pixelSize: dt ? dt.fontMd : 14
                                    elide: Text.ElideRight
                                }
                                Text {
                                    text: formatNum(modelData.human_typed_chars || 0) + " 字输入 · " + (modelData.sessions_count || 0) + " 次写作"
                                    color: dt ? dt.textMuted : "#606470"
                                    font.pixelSize: dt ? dt.fontXs : 11
                                }
                            }
                        }
                    }
                }

                // === Empty state ===
                ColumnLayout {
                    anchors.centerIn: undefined
                    width: parent.width
                    spacing: dt ? dt.sp16 : 16
                    visible: getVal(todayStats, "total_human_typed_chars", 0) === 0 &&
                             getVal(weekStats, "total_human_typed_chars", 0) === 0

                    Rectangle {
                        width: 60; height: 60
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.accentSoft : "rgba(123,140,222,0.12)"
                        Layout.alignment: Qt.AlignHCenter

                        Text {
                            anchors.centerIn: parent
                            text: "\uD83D\uDCC8"
                            font.pixelSize: 28
                        }
                    }

                    Text {
                        text: "还没有写作数据"
                        color: dt ? dt.textPrimary : "#E2E4E9"
                        font.pixelSize: dt ? dt.fontLg : 16
                        font.weight: Font.DemiBold
                        Layout.alignment: Qt.AlignHCenter
                    }

                    Text {
                        text: "开始写作后，这里会显示你的写作统计"
                        color: dt ? dt.textMuted : "#606470"
                        font.pixelSize: dt ? dt.fontSm : 12
                        Layout.alignment: Qt.AlignHCenter
                    }
                }

                Item { height: dt ? dt.sp32 : 32 }
            }
        }
    }
}
