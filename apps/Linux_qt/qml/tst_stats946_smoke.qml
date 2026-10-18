import QtQuick
import QtTest

TestCase {
    id: testCase
    name: "Stats946Smoke"

    property var window: null
    property var dt: null

    function init() {
        dt = Qt.createComponent("DesignTokens.qml").createObject(testCase)
        window = Qt.createQmlObject("import QtQuick; Window { visible: true; width: 1280; height: 800 }", testCase)
    }

    function cleanup() {
        if (window) { window.destroy(); window = null; }
        if (dt) { dt.destroy(); dt = null; }
    }

    function createPage() {
        let component = Qt.createComponent("StatsPreviewPage.qml")
        verify(component.status === Component.Ready, component.errorString())
        let page = component.createObject(window.contentItem, { "dt": dt, "width": 1280, "height": 800 })
        verify(page !== null, component.errorString())
        return page
    }

    function test_overview_period_and_lifetime_states() {
        let page = createPage()
        compare(page.narrowLayout, false, "1280px 宽窗应为宽屏布局")
        compare(page.expandedPanel, "", "初始应为四卡总览")

        page.expandedPanel = "last7Days"
        compare(page.detailPreset(), "last7Days")
        compare(page.detailHeight > 0, true)

        page.expandedPanel = "lifetime"
        compare(page.detailPreset(), "lifetime")

        page.collapsePanel()
        compare(page.expandedPanel, "")

        page.destroy()
    }

    function test_narrow_layout_switch() {
        let page = createPage()
        page.width = 700
        compare(page.narrowLayout, true, "700px 窄窗应切窄屏布局")

        page.selectNarrowPeriod("last30Days")
        compare(page.selectedPeriod, "last30Days")
        compare(page.expandedPanel, "")

        page.width = 1280
        compare(page.narrowLayout, false)

        page.destroy()
    }

    function test_custom_range_dialog_opens() {
        let page = createPage()
        page.openPeriod("custom")
        // 自选卡点开必须先弹起止日期 Dialog，而不是直接按旧区间请求。
        compare(page.startDate, "")
        compare(page.endDate, "")
        page.destroy()
    }

    function test_trend_chart_component() {
        let component = Qt.createComponent("StatsTrendChart.qml")
        verify(component.status === Component.Ready, component.errorString())
        let chart = component.createObject(window.contentItem, {
            "width": 400,
            "height": 200,
            "trend": {
                "granularity": "day",
                "points": [
                    { "date": "2026-10-12", "humanTypedChars": 512 },
                    { "date": "2026-10-13", "humanTypedChars": 0 },
                    { "date": "2026-10-14", "humanTypedChars": 906 }
                ]
            }
        })
        verify(chart !== null, component.errorString())
        compare(chart.points.length, 3)
        chart.destroy()
    }

    function test_record_panel_component() {
        let component = Qt.createComponent("StatsRecordPanel.qml")
        verify(component.status === Component.Ready, component.errorString())
        let panel = component.createObject(window.contentItem, { "width": 400, "columns": 2 })
        verify(panel !== null, component.errorString())
        compare(panel.entries.length, 4)
        compare(panel.entries[0].value, "—", "无纪录时必须显示 —，不用 0 冒充")
        panel.destroy()
    }

    function sampleSnapshot() {
        return {
            "historyScope": "localEvents",
            "lifetime": {
                "totalHumanTypedChars": 29456,
                "firstWritingDate": "2021-09-10",
                "lastWritingDate": "2026-09-13",
                "elapsedCalendarDays": 1093,
                "writingDays": 112,
                "averageCharsPerDay": 263.0,
                "averageCharsPerMinute": 90.0
            },
            "periodCards": [
                {
                    "preset": "last7Days", "hasSelection": true,
                    "range": { "startDate": "2026-10-12", "endDate": "2026-10-18" },
                    "totalHumanTypedChars": 3635, "totalActiveSeconds": 7270, "writingDays": 5,
                    "averageCharsPerDay": 727.0, "averageCharsPerMinute": 30.0
                },
                {
                    "preset": "last30Days", "hasSelection": true,
                    "range": { "startDate": "2026-09-19", "endDate": "2026-10-18" },
                    "totalHumanTypedChars": 39060, "totalActiveSeconds": 60000, "writingDays": 20,
                    "averageCharsPerDay": 1953.0, "averageCharsPerMinute": 60.0
                },
                {
                    "preset": "last6Months", "hasSelection": true,
                    "range": { "startDate": "2026-04-18", "endDate": "2026-10-18" },
                    "totalHumanTypedChars": 65434, "totalActiveSeconds": 90000, "writingDays": 70,
                    "averageCharsPerDay": 934.0, "averageCharsPerMinute": 43.0
                },
                {
                    "preset": "custom", "hasSelection": false, "range": null,
                    "totalHumanTypedChars": 0, "totalActiveSeconds": 0, "writingDays": 0,
                    "averageCharsPerDay": null, "averageCharsPerMinute": null
                }
            ],
            "selected": {
                "preset": "last7Days",
                "range": { "startDate": "2026-10-12", "endDate": "2026-10-18" },
                "totalHumanTypedChars": 3635, "totalActiveSeconds": 7270, "writingDays": 5,
                "averageCharsPerDay": 727.0, "averageCharsPerMinute": 30.0,
                "records": {
                    "bestDay": { "date": "2026-10-15", "endDate": null, "value": 906 },
                    "fastestHour": { "date": "2026-10-14", "endDate": null, "value": 36 },
                    "longestSession": { "date": "2026-10-13", "endDate": null, "value": 11040 },
                    "longestStreak": { "date": "2026-10-09", "endDate": "2026-10-11", "value": 3 }
                },
                "trend": {
                    "granularity": "day",
                    "points": [
                        { "date": "2026-10-12", "humanTypedChars": 512 },
                        { "date": "2026-10-13", "humanTypedChars": 0 },
                        { "date": "2026-10-14", "humanTypedChars": 906 }
                    ]
                }
            }
        }
    }

    function createFakeBackend() {
        return Qt.createQmlObject('
            import QtQuick
            QtObject {
                signal writing_stats_explorer_ready(int requestId, var snapshot)
                signal writing_stats_explorer_failed(int requestId, string message)
                signal writing_stats_dashboard_ready(int requestId, var snapshot)
                signal writing_stats_dashboard_failed(int requestId, string message)
                property int lastRequestId: 0
                property string lastPreset: ""
                property string lastStart: ""
                property string lastEnd: ""
                function request_writing_stats_explorer(requestId, preset, start, end) {
                    lastRequestId = requestId
                    lastPreset = preset
                    lastStart = start
                    lastEnd = end
                }
                function request_writing_stats_dashboard(requestId) {}
            }', testCase)
    }

    function test_date_popup_component_loads() {
        let popup = Qt.createQmlObject('
            import QtQuick
            import org.kde.kirigamiaddons.dateandtime as D
            D.DatePopup {
                value: new Date(2025, 2, 9)
            }', window.contentItem)
        verify(popup !== null)
        popup.open()
        compare(popup.visible, true)
        popup.close()
        popup.destroy()
    }

    function test_explorer_request_and_stale_result_filtering() {
        let backend = createFakeBackend()
        let page = createPage()
        page.editorBackendRef = backend
        page.requestStats()

        compare(backend.lastPreset, "last7Days", "初始详情请求默认近七天")
        compare(backend.lastStart, "", "非自选预设不带自选日期")
        compare(backend.lastEnd, "")

        // 迟到结果：旧 requestId 必须被丢弃。
        backend.writing_stats_explorer_ready(backend.lastRequestId - 1, { "bogus": true })
        compare(page.explorer, null, "旧 requestId 的结果必须被丢弃")

        // 当前 requestId 的结果被接受。
        backend.writing_stats_explorer_ready(backend.lastRequestId, sampleSnapshot())
        verify(page.explorer !== null)
        compare(page.lifetime.totalHumanTypedChars, 29456)
        compare(page.periodCards.length, 4)
        compare(page.periodCards[3].hasSelection, false, "未选择的自选卡不得被当成 0 字区间")
        compare(page.selectedRecords.bestDay.value, 906)
        compare(page.selectedTrend.points.length, 3)

        // 失败提示不清空上次快照。
        page.expandedPanel = "last30Days"
        page.requestStats()
        compare(backend.lastPreset, "last30Days")
        backend.writing_stats_explorer_failed(backend.lastRequestId, "read failed")
        compare(page.statsError, "read failed")
        compare(page.lifetime.totalHumanTypedChars, 29456, "失败不得清空上次数据冒充 0")

        // 生涯展开走 lifetime 预设。
        page.toggleLifetime()
        compare(backend.lastPreset, "lifetime")

        // 自选区间带两端日期。
        page.applyCustomRange("2025-03-09", "2026-07-26")
        compare(backend.lastPreset, "custom")
        compare(backend.lastStart, "2025-03-09")
        compare(backend.lastEnd, "2026-07-26")
        compare(page.expandedPanel, "custom")

        page.destroy()
        backend.destroy()
    }
}
