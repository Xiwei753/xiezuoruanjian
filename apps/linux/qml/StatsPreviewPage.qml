// =============================================================================
// StatsPreviewPage.qml — 统计预览页面
// =============================================================================
//
// 层级：Linux UI 层（QML 页面）
// 职责：展示今日/本周/月度写作统计、项目/章节/设备统计、速度曲线
// 约束：
//   - 纯展示层，数据通过 backendRef 从 AppBackend 获取
//   - 不直接操作文件系统或 Core 层
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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
    function formatDuration(seconds) { var s = Math.max(0, Number(seconds || 0)); var h = Math.floor(s / 3600); var m = Math.floor((s % 3600) / 60); return h > 0 ? (h + qsTr("时") + m + qsTr("分")) : (m + qsTr("分")) }

    function loadStats() {
        if (!backendRef) return
        try {
            var t = new Date()
            var td = t.getFullYear() + "-" + String(t.getMonth() + 1).padStart(2, "0") + "-" + String(t.getDate()).padStart(2, "0")
            var ws = new Date(t); ws.setDate(t.getDate() - t.getDay())
            var wd = ws.getFullYear() + "-" + String(ws.getMonth() + 1).padStart(2, "0") + "-" + String(ws.getDate()).padStart(2, "0")
            var ms = new Date(t.getFullYear(), t.getMonth(), 1)
            var md = ms.getFullYear() + "-" + String(ms.getMonth() + 1).padStart(2, "0") + "-" + String(ms.getDate()).padStart(2, "0")
            todayStats = backendRef.get_writing_stats_summary_object(td, td) || {}
            weekStats = backendRef.get_writing_stats_summary_object(wd, td) || {}
            monthStats = backendRef.get_writing_stats_summary_object(md, td) || {}
            projectStats = (backendRef.get_writing_stats_by_project_object(wd, td) || {}).projects || []
            chapterStats = (backendRef.get_writing_stats_by_chapter_object(wd, td) || {}).chapters || []
            deviceStats = (backendRef.get_writing_stats_by_device_object(wd, td) || {}).devices || []
            speedCurve = (backendRef.get_writing_speed_curve_object(wd, td, 60) || {}).buckets || []
        } catch (e) {
            todayStats = {}; weekStats = {}; monthStats = {}; projectStats = []; chapterStats = []; deviceStats = []; speedCurve = []
        }
    }

    Component.onCompleted: loadStats()
    onVisibleChanged: if (visible) loadStats()

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt
        headerData: [ HubPageHeader { anchors.fill: parent; dt: root.dt; title: qsTr("统计"); subtitle: qsTr("追踪你的写作节奏与习惯") } ]

        DashboardGrid {
            id: dashboard
            Layout.fillWidth: true
            dt: root.dt

            GridLayout {
                Layout.fillWidth: true
                columns: dashboard.wide ? 4 : (dashboard.medium ? 2 : 1)
                columnSpacing: dashboard.gap
                rowSpacing: dashboard.gap
                StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: qsTr("今日纯输入"); value: formatNum(val(todayStats, "total_human_typed_chars")); caption: qsTr("字") }
                StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: qsTr("本周纯输入"); value: formatNum(val(weekStats, "total_human_typed_chars")); caption: qsTr("字") }
                StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: qsTr("本月纯输入"); value: formatNum(val(monthStats, "total_human_typed_chars")); caption: qsTr("字") }
                StatCard { dt: root.dt; Layout.fillWidth: true; height: 120; title: qsTr("本周活跃时长"); value: formatDuration(val(weekStats, "total_active_seconds")); caption: qsTr("活跃写作") }
            }

            GridLayout {
                Layout.fillWidth: true
                columns: dashboard.wide ? 3 : (dashboard.medium ? 2 : 1)
                columnSpacing: dashboard.gap
                rowSpacing: dashboard.gap

                DashboardSection {
                    dt: root.dt
                    title: qsTr("速度曲线")
                    Layout.fillWidth: true
                    Layout.columnSpan: dashboard.wide ? 2 : 1
                    Text { text: speedCurve.length > 0 ? qsTr("已记录 %1 段").arg(speedCurve.length) : qsTr("暂无数据"); color: dt ? dt.textPrimary : root.palette.text }
                }

                DashboardSection {
                    dt: root.dt
                    title: qsTr("设备统计")
                    Layout.fillWidth: true
                    Repeater {
                        model: deviceStats.length > 0 ? deviceStats.slice(0, 4) : [{ device_name: qsTr("暂无数据"), typed_chars: 0 }]
                        delegate: Text { text: (modelData.device_name || qsTr("未知设备")) + "  " + formatNum(modelData.typed_chars || 0); color: dt ? dt.textSecondary : root.palette.text }
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
                    title: qsTr("作品排行")
                    Layout.fillWidth: true
                    Repeater {
                        model: projectStats.length > 0 ? projectStats.slice(0, 5) : [{ project_title: qsTr("暂无数据"), human_typed_chars: 0 }]
                        delegate: Text { text: (index + 1) + ". " + (modelData.project_title || qsTr("未命名")) + "  " + formatNum(modelData.human_typed_chars || 0); color: dt ? dt.textPrimary : root.palette.text }
                    }
                }

                DashboardSection {
                    dt: root.dt
                    title: qsTr("章节排行")
                    Layout.fillWidth: true
                    Repeater {
                        model: chapterStats.length > 0 ? chapterStats.slice(0, 5) : [{ chapter_title: qsTr("暂无数据"), human_typed_chars: 0 }]
                        delegate: Text { text: (index + 1) + ". " + (modelData.chapter_title || qsTr("未命名")) + "  " + formatNum(modelData.human_typed_chars || 0); color: dt ? dt.textPrimary : root.palette.text }
                    }
                }
            }
        }
    }
}
