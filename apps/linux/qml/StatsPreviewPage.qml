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
    color: dt ? dt.bg : "#111318"

    function getVal(obj, key, fallback) { return (obj && obj[key] !== undefined && obj[key] !== null) ? obj[key] : fallback; }
    function formatNum(n) { n = Number(n || 0); return n.toLocaleString(); }
    function formatDuration(seconds) { seconds = Number(seconds || 0); var h = Math.floor(seconds / 3600); var m = Math.floor((seconds % 3600) / 60); return h > 0 ? (h + "时" + m + "分") : (m + "分"); }
    function topStats() { return getVal(todayStats, "total_human_typed_chars", 0) + getVal(weekStats, "total_human_typed_chars", 0) + getVal(monthStats, "total_human_typed_chars", 0); }

    function loadStats() {
        if (!backendRef) return;
        try {
            var t = new Date();
            var td = t.getFullYear() + "-" + String(t.getMonth() + 1).padStart(2, "0") + "-" + String(t.getDate()).padStart(2, "0");
            var ws = new Date(t); ws.setDate(t.getDate() - t.getDay());
            var wd = ws.getFullYear() + "-" + String(ws.getMonth() + 1).padStart(2, "0") + "-" + String(ws.getDate()).padStart(2, "0");
            var ms = new Date(t.getFullYear(), t.getMonth(), 1);
            var md = ms.getFullYear() + "-" + String(ms.getMonth() + 1).padStart(2, "0") + "-" + String(ms.getDate()).padStart(2, "0");
            todayStats = JSON.parse(backendRef.get_writing_stats_summary(td, td)) || {};
            weekStats = JSON.parse(backendRef.get_writing_stats_summary(wd, td)) || {};
            monthStats = JSON.parse(backendRef.get_writing_stats_summary(md, td)) || {};
            projectStats = (JSON.parse(backendRef.get_writing_stats_by_project(wd, td)) || {}).projects || [];
            chapterStats = (JSON.parse(backendRef.get_writing_stats_by_chapter(wd, td)) || {}).chapters || [];
            deviceStats = (JSON.parse(backendRef.get_writing_stats_by_device(wd, td)) || {}).devices || [];
            speedCurve = (JSON.parse(backendRef.get_writing_speed_curve(wd, td, 60)) || {}).buckets || [];
        } catch (e) {
            todayStats = {}; weekStats = {}; monthStats = {}; projectStats = []; chapterStats = []; deviceStats = []; speedCurve = [];
        }
    }

    Component.onCompleted: loadStats()
    onVisibleChanged: if (visible) loadStats()

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt
        headerData: [ HubPageHeader { dt: root.dt; title: "统计"; subtitle: "追踪你的写作节奏与习惯" } ]

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            contentWidth: availableWidth

            ColumnLayout {
                width: parent.width
                spacing: dt ? dt.cardGap : 16

                GridLayout {
                    columns: root.width > 1200 ? 6 : (root.width > 860 ? 3 : 2)
                    columnSpacing: dt ? dt.gridGap : 16
                    rowSpacing: dt ? dt.gridGap : 16
                    Layout.fillWidth: true

                    StatCard { dt: root.dt; Layout.columnSpan: 2; Layout.fillWidth: true; height: 110; title: "今日"; value: formatNum(getVal(todayStats, "total_human_typed_chars", 0)); caption: "纯输入字数" }
                    StatCard { dt: root.dt; Layout.columnSpan: 2; Layout.fillWidth: true; height: 110; title: "本周"; value: formatNum(getVal(weekStats, "total_human_typed_chars", 0)); caption: "纯输入字数" }
                    StatCard { dt: root.dt; Layout.columnSpan: 2; Layout.fillWidth: true; height: 110; title: "本月"; value: formatNum(getVal(monthStats, "total_human_typed_chars", 0)); caption: "纯输入字数" }

                    Rectangle {
                        Layout.columnSpan: root.width > 1000 ? 4 : 2
                        Layout.fillWidth: true
                        height: 180
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"
                        border.color: dt ? dt.border : "#2A2E36"
                        border.width: 1
                        Text { anchors.centerIn: parent; text: speedCurve.length > 0 ? "速度曲线已记录 " + speedCurve.length + " 段" : "暂无速度曲线数据"; color: dt ? dt.textSecondary : "#9CA0AB" }
                    }

                    StatCard { dt: root.dt; Layout.columnSpan: 2; Layout.fillWidth: true; height: 180; title: "写作时长"; value: formatDuration(getVal(weekStats, "total_active_seconds", 0)); caption: "本周活跃时长" }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: dt ? dt.gridGap : 16

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        height: 220
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"
                        border.color: dt ? dt.border : "#2A2E36"
                        border.width: 1
                        Text { anchors.left: parent.left; anchors.leftMargin: 12; anchors.top: parent.top; anchors.topMargin: 10; text: "作品排行"; color: dt ? dt.textSecondary : "#9CA0AB" }
                        Text { anchors.centerIn: parent; text: projectStats.length > 0 ? (projectStats[0].project_id || "") : "暂无数据"; color: dt ? dt.textPrimary : "#E2E4E9" }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        height: 220
                        radius: dt ? dt.radiusMd : 12
                        color: dt ? dt.card : "#1E2128"
                        border.color: dt ? dt.border : "#2A2E36"
                        border.width: 1
                        Text { anchors.left: parent.left; anchors.leftMargin: 12; anchors.top: parent.top; anchors.topMargin: 10; text: "章节排行 / 设备统计"; color: dt ? dt.textSecondary : "#9CA0AB" }
                        Text { anchors.centerIn: parent; text: (chapterStats.length + deviceStats.length) > 0 ? "已统计" : "暂无数据"; color: dt ? dt.textPrimary : "#E2E4E9" }
                    }
                }

                Rectangle {
                    visible: topStats() === 0
                    Layout.fillWidth: true
                    height: 92
                    radius: dt ? dt.radiusMd : 12
                    color: dt ? dt.surfaceVariant : "#242933"
                    border.color: dt ? dt.border : "#2A2E36"
                    border.width: 1
                    Text { anchors.centerIn: parent; text: "还没有写作数据，开始写作后这里会展开完整统计。"; color: dt ? dt.textSecondary : "#9CA0AB" }
                }
            }
        }
    }
}
