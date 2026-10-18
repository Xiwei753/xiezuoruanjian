import QtQuick
import QtQuick.Controls
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
        compare(page.narrowLayout, false, "wide layout at 1280px")
        compare(page.expandedPanel, "", "starts on the four-card overview")

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
        compare(page.narrowLayout, true, "narrow layout at 700px")

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

    // 自选区间 Dialog 藏在页面 resources 里（非可视对象），按特征找到它。
    function customRangeDialogOf(page) {
        for (let i = 0; i < page.resources.length; i++) {
            let r = page.resources[i]
            if (r && typeof r.prepare === "function" && r.footer !== undefined)
                return r
        }
        return null
    }

    function footerButtonByRole(dialog, role) {
        for (let i = 0; i < dialog.footer.contentChildren.length; i++) {
            let b = dialog.footer.contentChildren[i]
            if (b.DialogButtonBox.buttonRole === role)
                return b
        }
        return null
    }

    function firstHiddenTextItem(item) {
        let kids = item ? (item.children || []) : []
        for (let i = 0; i < kids.length; i++) {
            let k = kids[i]
            if (k.text !== undefined && k.visible === false)
                return k
            let found = firstHiddenTextItem(k)
            if (found) return found
        }
        return null
    }

    // 回归守卫（Issue #946）：AcceptRole 会自动发出 accepted() 让 Dialog 关闭，
    // 非法区间的错误提示因此看不到；「确定」必须是 ActionRole，校验只由 onClicked 决定。
    function test_custom_range_confirm_role_and_validation() {
        let backend = createFakeBackend()
        let page = createPage()
        page.editorBackendRef = backend
        page.openPeriod("custom")
        let dialog = customRangeDialogOf(page)
        verify(dialog !== null, "custom range dialog must be a page resource")
        tryCompare(dialog, "visible", true)

        compare(footerButtonByRole(dialog, DialogButtonBox.AcceptRole), null,
                "confirm must not use AcceptRole (it would auto-close the dialog)")
        let confirm = footerButtonByRole(dialog, DialogButtonBox.ActionRole)
        verify(confirm !== null, "confirm button must be ActionRole")
        verify(footerButtonByRole(dialog, DialogButtonBox.RejectRole) !== null,
               "cancel button keeps RejectRole")

        let errorLabel = firstHiddenTextItem(dialog.contentItem)
        verify(errorLabel !== null, "error label starts hidden")

        // 非法区间：校验拒绝后弹窗保持打开、只显示错误、不发请求。
        dialog.startValue = new Date(2026, 0, 1)
        dialog.endValue = new Date(2025, 0, 1)
        mouseClick(confirm)
        compare(dialog.visible, true, "invalid range must keep the dialog open")
        compare(backend.lastRequestId, 0, "invalid range must not request stats")
        compare(errorLabel.visible, true)
        verify(errorLabel.text.length > 0, "invalid range must show an error message")

        // 合法区间：onClicked 内显式关闭并发起自选区间请求。
        dialog.startValue = new Date(2025, 0, 1)
        dialog.endValue = new Date(2025, 1, 1)
        mouseClick(confirm)
        tryCompare(dialog, "visible", false)
        compare(backend.lastPreset, "custom")
        compare(backend.lastStart, "2025-01-01")
        compare(backend.lastEnd, "2025-02-01")

        page.destroy()
        backend.destroy()
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
        compare(panel.entries[0].value, "—", "missing record must show a dash, not a fake zero")
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

        compare(backend.lastPreset, "last7Days", "default detail preset is last7Days")
        compare(backend.lastStart, "", "non-custom presets carry no custom dates")
        compare(backend.lastEnd, "")

        // 迟到结果：旧 requestId 必须被丢弃。
        backend.writing_stats_explorer_ready(backend.lastRequestId - 1, { "bogus": true })
        compare(page.explorer, null, "stale requestId result must be dropped")

        // 当前 requestId 的结果被接受。
        backend.writing_stats_explorer_ready(backend.lastRequestId, sampleSnapshot())
        verify(page.explorer !== null)
        compare(page.lifetime.totalHumanTypedChars, 29456)
        compare(page.periodCards.length, 4)
        compare(page.periodCards[3].hasSelection, false, "unselected custom card must not look like a 0-char range")
        compare(page.selectedRecords.bestDay.value, 906)
        compare(page.selectedTrend.points.length, 3)

        // 失败提示不清空上次快照。
        page.expandedPanel = "last30Days"
        page.requestStats()
        compare(backend.lastPreset, "last30Days")
        backend.writing_stats_explorer_failed(backend.lastRequestId, "read failed")
        compare(page.statsError, "read failed")
        compare(page.lifetime.totalHumanTypedChars, 29456, "failure must keep the last snapshot")

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
