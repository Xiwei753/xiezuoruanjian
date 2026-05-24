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

    function val(obj, key) { var v = obj && obj[key] !== undefined ? Number(obj[key]) : 0; return isNaN(v) ? 0 : v }
    function formatNum(n) { return Number(n || 0).toLocaleString() }
    function formatDuration(seconds) { var s = Math.max(0, Number(seconds || 0)); var h = Math.floor(s / 3600); var m = Math.floor((s % 3600) / 60); return h > 0 ? (h + "时" + m + "分") : (m + "分") }

    function loadStats() {
        if (!backendRef) return
        try {
            var t = new Date()
            var td = t.getFullYear() + "-" + String(t.getMonth() + 1).padStart(2, "0") + "-" + String(t.getDate()).padStart(2, "0")
            var ws = new Date(t); ws.setDate(t.getDate() - t.getDay())
            var wd = ws.getFullYear() + "-" + String(ws.getMonth() + 1).padStart(2, "0") + "-" + String(ws.getDate()).padStart(2, "0")
            var ms = new Date(t.getFullYear(), t.getMonth(), 1)
            var md = ms.getFullYear() + "-" + String(ms.getMonth() + 1).padStart(2, "0") + "-" + String(ms.getDate()).padStart(2, "0")
            todayStats = JSON.parse(backendRef.get_writing_stats_summary(td, td)) || {}
            weekStats = JSON.parse(backendRef.get_writing_stats_summary(wd, td)) || {}
            monthStats = JSON.parse(backendRef.get_writing_stats_summary(md, td)) || {}
            projectStats = (JSON.parse(backendRef.get_writing_stats_by_project(wd, td)) || {}).projects || []
            chapterStats = (JSON.parse(backendRef.get_writing_stats_by_chapter(wd, td)) || {}).chapters || []
            deviceStats = (JSON.parse(backendRef.get_writing_stats_by_device(wd, td)) || {}).devices || []
            speedCurve = (JSON.parse(backendRef.get_writing_speed_curve(wd, td, 60)) || {}).buckets || []
        } catch (e) {
            todayStats = {}; weekStats = {}; monthStats = {}; projectStats = []; chapterStats = []; deviceStats = []; speedCurve = []
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

            DashboardGrid {
                id: dashboard
                width: root.width - ((dt ? dt.pageMarginNarrow : 24) * 2)
                dt: root.dt

                GridLayout {
                    Layout.fillWidth: true
                    columns: dashboard.wide ? 4 : (dashboard.medium ? 2 : 1)
                    columnSpacing: dashboard.gap
                    rowSpacing: dashboard.gap
                    StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: "今日纯输入"; value: formatNum(val(todayStats, "total_human_typed_chars")); caption: "字" }
                    StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: "本周纯输入"; value: formatNum(val(weekStats, "total_human_typed_chars")); caption: "字" }
                    StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: "本月纯输入"; value: formatNum(val(monthStats, "total_human_typed_chars")); caption: "字" }
                    StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: "本周活跃时长"; value: formatDuration(val(weekStats, "total_active_seconds")); caption: "活跃写作" }
                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: dashboard.wide ? 3 : (dashboard.medium ? 2 : 1)
                    columnSpacing: dashboard.gap
                    rowSpacing: dashboard.gap

                    DashboardSection {
                        dt: root.dt
                        title: "速度曲线"
                        Layout.fillWidth: true
                        Layout.columnSpan: dashboard.wide ? 2 : 1
                        implicitHeight: 220
                        Text { text: speedCurve.length > 0 ? ("已记录 " + speedCurve.length + " 段") : "暂无数据"; color: dt ? dt.textPrimary : "#E2E4E9" }
                    }

                    DashboardSection {
                        dt: root.dt
                        title: "设备统计"
                        Layout.fillWidth: true
                        implicitHeight: 220
                        Repeater {
                            model: deviceStats.length > 0 ? deviceStats.slice(0, 4) : [{ device_name: "暂无数据", typed_chars: 0 }]
                            delegate: Text { text: (modelData.device_name || "未知设备") + "  " + formatNum(modelData.typed_chars || 0); color: dt ? dt.textSecondary : "#9CA0AB" }
                        }
                    }
                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: dashboard.wide || dashboard.medium ? 2 : 1
                    columnSpacing: dashboard.gap
                    rowSpacing: dashboard.gap

                    DashboardSection {
                        dt: root.dt
                        title: "作品排行"
                        Layout.fillWidth: true
                        implicitHeight: 240
                        Repeater {
                            model: projectStats.length > 0 ? projectStats.slice(0, 5) : [{ project_title: "暂无数据", human_typed_chars: 0 }]
                            delegate: Text { text: (index + 1) + ". " + (modelData.project_title || "未命名") + "  " + formatNum(modelData.human_typed_chars || 0); color: dt ? dt.textPrimary : "#E2E4E9" }
                        }
                    }

                    DashboardSection {
                        dt: root.dt
                        title: "章节排行"
                        Layout.fillWidth: true
                        implicitHeight: 240
                        Repeater {
                            model: chapterStats.length > 0 ? chapterStats.slice(0, 5) : [{ chapter_title: "暂无数据", human_typed_chars: 0 }]
                            delegate: Text { text: (index + 1) + ". " + (modelData.chapter_title || "未命名") + "  " + formatNum(modelData.human_typed_chars || 0); color: dt ? dt.textPrimary : "#E2E4E9" }
                        }
                    }
                }
            }
        }
    }
}
