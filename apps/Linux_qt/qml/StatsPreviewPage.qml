// =============================================================================
// StatsPreviewPage.qml — 统计预览页面
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责（Issue #946）：按新宽屏手稿重做统计主体 ——
//   - 顶部生涯条：累积人工码字、首末记录日期、平均字/分、平均字/天、
//     已持续自然日历天数；点击原位展开「左纪录 / 右趋势」；
//   - 四个时段卡横排（近七天 / 近三十天 / 近半年 / 自选区间）；点一张后
//     四卡区域原位替换为「左摘要 / 中纪录 / 右趋势」，可返回四卡总览；
//   - 自选区间先弹起止日期 Dialog，确认后以实际区间请求；
//   - 窄窗口改为纵向滚动：范围切换 → 总量与区间 → 2×2 纪录 → 趋势图；
//   - 作品 / 章节 / 设备排行与本周速度曲线收进可折叠「详细统计」。
// 约束：
//   - 纯展示层：只消费 editorBackendRef 的 Explorer / Dashboard 快照，
//     不读文件系统、不做统计聚合（纪录、速度、连续天数全在 Core）；
//     字段缺失显示 "—"，不用 0 冒充。
//   - 两次异步请求各自用递增 requestId 过滤迟到结果；失败保留上次数据，
//     只提示本次读取失败；不在 UI 线程同步读盘（沿用 #935 的 worker 队列）。
//   - 一级导航由 CreativeHub 桌面外壳负责，本页不造手机式底栏。
// 历史：
//   - Issue #935：统计页改为「单次异步 dashboard 快照」，本页保留旧 Dashboard
//     请求/信号，只服务下方「详细统计」；主体改用 Explorer 快照（#946）。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.dateandtime as DateAndTime

Rectangle {
    id: root
    required property var dt
    property var editorBackendRef: null
    property var appState: ({})
    color: Kirigami.Theme.backgroundColor

    // ── Explorer 请求簿记：同一时刻只允许一个在途请求，用递增 requestId 丢弃迟到结果 ──
    property int requestSerial: 0
    property int activeRequestId: 0
    property var explorer: null
    property string statsError: ""

    // ── 页面选择状态 ──
    // selectedPeriod：当前选中时段（窄屏范围切换与详情请求都以它为准）；
    // expandedPanel：宽屏展开项（"" 四卡总览、"lifetime" 生涯、其余为 preset）。
    property string selectedPeriod: "last7Days"
    property string expandedPanel: ""
    property string startDate: ""
    property string endDate: ""

    // ── 详细统计：旧 Dashboard 快照，独立请求，不与 Explorer 区间混显示 ──
    property int dashboardRequestSerial: 0
    property int activeDashboardRequestId: 0
    property var dashboard: null
    property string dashboardError: ""
    property bool detailedStatsExpanded: false

    // ── 布局断点与尺寸（Qt 本地宽度断点，不向 Core 要布局计划） ──
    readonly property bool narrowLayout: width < 840
    readonly property int detailHeight: Kirigami.Units.gridUnit * 19

    // ── 派生数据：字段缺失时退化为空，渲染层再决定显示 "—" ──
    readonly property var lifetime: explorer && explorer.lifetime ? explorer.lifetime : null
    readonly property var selected: explorer && explorer.selected ? explorer.selected : null
    readonly property var selectedRecords: selected ? selected.records : null
    readonly property var selectedTrend: selected ? selected.trend : null
    readonly property string historyScope: explorer && explorer.historyScope ? String(explorer.historyScope) : ""
    // 首个快照到达前先摆四张空卡（只有预设名，数值显示 "—"）。
    readonly property var periodCards: explorer && explorer.periodCards && explorer.periodCards.length > 0
        ? explorer.periodCards
        : [
            { "preset": "last7Days" },
            { "preset": "last30Days" },
            { "preset": "last6Months" },
            { "preset": "custom" }
        ]

    // 从对象读数值；字段缺失或非数字返回 null（交给 format* 显示 "—"，不伪造 0）。
    function val(obj, key) {
        if (!obj || obj[key] === undefined || obj[key] === null) return null
        var n = Number(obj[key])
        return isNaN(n) ? null : n
    }

    // 从对象读字符串；字段缺失返回 ""。
    function fieldString(obj, key) {
        if (!obj || obj[key] === undefined || obj[key] === null) return ""
        return String(obj[key])
    }

    function formatNum(n) {
        if (n === null || n === undefined) return "—"
        var v = Number(n)
        return isNaN(v) ? "—" : v.toLocaleString()
    }

    // 平均值最多保留一位小数；null 显示 "—"，不伪造 0。
    function formatAverage(n) {
        if (n === null || n === undefined) return "—"
        var v = Number(n)
        if (isNaN(v)) return "—"
        return String(Math.round(v * 10) / 10)
    }

    function formatDate(iso) {
        if (!iso) return ""
        var parts = String(iso).split("-")
        if (parts.length !== 3) return String(iso)
        return Number(parts[0]) + "/" + Number(parts[1]) + "/" + Number(parts[2])
    }

    function periodTitle(preset) {
        switch (preset) {
        case "last7Days": return qsTr("近七天")
        case "last30Days": return qsTr("近三十天")
        case "last6Months": return qsTr("近半年")
        case "custom": return qsTr("自选区间")
        case "lifetime": return qsTr("生涯")
        }
        return ""
    }

    function lifetimeRangeText() {
        if (!root.lifetime) return "—"
        var first = root.formatDate(root.lifetime.firstWritingDate)
        var last = root.formatDate(root.lifetime.lastWritingDate)
        if (first === "" && last === "") return qsTr("暂无本机记录")
        return first + " – " + last
    }

    function selectedRangeText() {
        if (!root.selected || !root.selected.range) return "—"
        return root.formatDate(root.selected.range.startDate) + " – " + root.formatDate(root.selected.range.endDate)
    }

    // 当前详情需要请求的 preset：生涯展开优先，其次展开的时段卡，最后是选中时段。
    function detailPreset() {
        if (root.expandedPanel === "lifetime") return "lifetime"
        if (root.expandedPanel !== "") return root.expandedPanel
        return root.selectedPeriod
    }

    function requestStats() {
        if (!root.editorBackendRef) return
        var preset = root.detailPreset()
        var customStart = ""
        var customEnd = ""
        if (preset === "custom") {
            // 还没有确认过自选区间就不发请求，交给日期 Dialog 先问区间。
            if (root.startDate === "" || root.endDate === "") return
            customStart = root.startDate
            customEnd = root.endDate
        }
        root.requestSerial += 1
        root.activeRequestId = root.requestSerial
        root.statsError = ""
        root.editorBackendRef.request_writing_stats_explorer(root.activeRequestId, preset, customStart, customEnd)
    }

    function openPeriod(preset) {
        if (preset === "custom") {
            // 手稿约定：自选卡点开先弹起止日期 Dialog，确认后才以实际区间展开。
            customRangeDialog.prepare(root.startDate, root.endDate)
            customRangeDialog.open()
            return
        }
        root.selectedPeriod = preset
        root.expandedPanel = preset
        root.requestStats()
    }

    function selectNarrowPeriod(preset) {
        if (preset === "custom") {
            if (root.startDate === "" || root.endDate === "") {
                customRangeDialog.prepare("", "")
                customRangeDialog.open()
                return
            }
            root.selectedPeriod = "custom"
            root.expandedPanel = ""
            root.requestStats()
            return
        }
        root.selectedPeriod = preset
        root.expandedPanel = ""
        root.requestStats()
    }

    function collapsePanel() {
        root.expandedPanel = ""
    }

    function toggleLifetime() {
        if (root.expandedPanel === "lifetime") {
            root.expandedPanel = ""
            // 窄屏收起生涯后回到选中时段：立即把 `selected` 换回该时段，
            // 避免图表继续挂着生涯数据冒充区间数据。
            if (root.narrowLayout) root.requestStats()
            return
        }
        root.expandedPanel = "lifetime"
        root.requestStats()
    }

    function applyCustomRange(startIso, endIso) {
        root.startDate = startIso
        root.endDate = endIso
        root.selectedPeriod = "custom"
        if (!root.narrowLayout) root.expandedPanel = "custom"
        root.requestStats()
    }

    // ── 详细统计（旧 Dashboard）：只在此折叠区使用，数据范围与上方区间无关 ──
    function requestDashboard() {
        if (!root.editorBackendRef) return
        root.dashboardRequestSerial += 1
        root.activeDashboardRequestId = root.dashboardRequestSerial
        root.dashboardError = ""
        root.editorBackendRef.request_writing_stats_dashboard(root.activeDashboardRequestId)
    }

    function toggleDetailedStats() {
        root.detailedStatsExpanded = !root.detailedStatsExpanded
        if (root.detailedStatsExpanded) requestDashboard()
    }

    function dashboardRangeText() {
        if (!root.dashboard || !root.dashboard.range) return ""
        return qsTr("数据范围 %1 – %2（本周口径，与上方区间选择分开）")
            .arg(root.formatDate(root.dashboard.range.startDate))
            .arg(root.formatDate(root.dashboard.range.endDate))
    }

    Component.onCompleted: requestStats()
    onVisibleChanged: if (visible) requestStats()

    // ── 旧 Dashboard 快照 / 新 Explorer 快照，各自按 requestId 过滤迟到结果 ──
    Connections {
        target: root.editorBackendRef

        function onWriting_stats_explorer_ready(requestId, snapshot) {
            if (requestId !== root.activeRequestId) return
            root.explorer = snapshot
            root.statsError = ""
        }

        function onWriting_stats_explorer_failed(requestId, message) {
            if (requestId !== root.activeRequestId) return
            // 保留旧快照数字，只提示本次读取失败；不清空 explorer 冒充 0。
            root.statsError = String(message || qsTr("统计数据读取失败"))
        }

        function onWriting_stats_dashboard_ready(requestId, snapshot) {
            if (requestId !== root.activeDashboardRequestId) return
            root.dashboard = snapshot
            root.dashboardError = ""
        }

        function onWriting_stats_dashboard_failed(requestId, message) {
            if (requestId !== root.activeDashboardRequestId) return
            root.dashboardError = String(message || qsTr("详细统计读取失败"))
        }
    }

    // ── 详细统计里作品 / 章节 / 设备排行与本周速度曲线仍消费旧 Dashboard 快照 ──
    readonly property var sortedProjects: root.sortByHumanTypedChars(root.dashboard && root.dashboard.projects ? root.dashboard.projects : [])
    readonly property var sortedChapters: root.sortByHumanTypedChars(root.dashboard && root.dashboard.chapters ? root.dashboard.chapters : [])
    readonly property var sortedDevices: root.sortByHumanTypedChars(root.dashboard && root.dashboard.devices ? root.dashboard.devices : [])
    readonly property var speedBuckets: root.dashboard && root.dashboard.speedCurve && root.dashboard.speedCurve.buckets ? root.dashboard.speedCurve.buckets : []

    function sortByHumanTypedChars(list) {
        var copy = (list || []).slice()
        copy.sort(function(a, b) {
            var av = Number(a && a.humanTypedChars)
            var bv = Number(b && b.humanTypedChars)
            if (isNaN(av)) av = 0
            if (isNaN(bv)) bv = 0
            return bv - av
        })
        return copy
    }

    // 作品/章节标题：遍历 appState.tree，找不到时回退显示 ID 本身。
    function projectTitleById(projectId) {
        var tree = root.appState && root.appState.tree ? root.appState.tree : []
        for (var i = 0; i < tree.length; i++) {
            var node = tree[i]
            if (node && node.type === "project" && node.id === projectId) {
                return node.title ? String(node.title) : String(projectId)
            }
        }
        return String(projectId)
    }

    function chapterTitleById(chapterId) {
        var tree = root.appState && root.appState.tree ? root.appState.tree : []
        for (var i = 0; i < tree.length; i++) {
            var node = tree[i]
            if (node && node.type === "chapter" && node.id === chapterId) {
                return node.title ? String(node.title) : String(chapterId)
            }
        }
        return String(chapterId)
    }

    // 设备标签：platform · deviceClass · deviceId，空字段跳过，不编造名字。
    function deviceLabel(device) {
        if (!device) return ""
        var parts = []
        if (device.platform) parts.push(String(device.platform))
        if (device.deviceClass) parts.push(String(device.deviceClass))
        if (device.deviceId) parts.push(String(device.deviceId))
        return parts.join(" · ")
    }

    function speedPeak() {
        var maxRate = 0
        var buckets = root.speedBuckets
        for (var i = 0; i < buckets.length; i++) {
            var rate = Number(buckets[i] && buckets[i].charsPerMinute)
            if (!isNaN(rate) && rate > maxRate) maxRate = rate
        }
        return maxRate
    }

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt
        headerData: [ HubPageHeader { dt: root.dt; title: qsTr("统计"); subtitle: qsTr("追踪你的写作节奏与习惯") } ]

        Flickable {
            id: pageFlick
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: width
            contentHeight: pageColumn.implicitHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            // Issue #842：桌面鼠标左键不能按住空白处拖页面，保留滚轮/触摸板/滚动条。
            acceptedButtons: Qt.NoButton

            ColumnLayout {
                id: pageColumn
                width: pageFlick.width
                spacing: Kirigami.Units.largeSpacing

                // ── 错误条：与正常数据无关，独立显示，不遮住上次快照 ──
                Rectangle {
                    Layout.fillWidth: true
                    visible: root.statsError.length > 0
                    implicitHeight: visible ? 40 : 0
                    color: Kirigami.Theme.negativeTextColor
                    radius: Kirigami.Units.cornerRadius

                    Label {
                        anchors.centerIn: parent
                        text: root.statsError
                        color: Kirigami.Theme.backgroundColor
                    }
                }

                // ── 生涯顶部条：收起时只显示摘要；展开后原位显示「左纪录 / 右趋势」──
                Kirigami.ShadowedRectangle {
                    id: lifetimeBox
                    Layout.fillWidth: true
                    radius: Kirigami.Units.cornerRadius
                    color: Kirigami.Theme.alternateBackgroundColor
                    border.color: root.expandedPanel === "lifetime" ? Kirigami.Theme.highlightColor : Kirigami.Theme.disabledTextColor
                    border.width: 1
                    implicitHeight: lifetimeColumn.implicitHeight + Kirigami.Units.largeSpacing * 2

                    ColumnLayout {
                        id: lifetimeColumn
                        anchors.top: parent.top
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: Kirigami.Units.largeSpacing
                        spacing: Kirigami.Units.largeSpacing

                        Item {
                            Layout.fillWidth: true
                            implicitHeight: lifetimeHeaderRow.implicitHeight

                            RowLayout {
                                id: lifetimeHeaderRow
                                anchors.fill: parent
                                spacing: Kirigami.Units.largeSpacing

                                Kirigami.Heading {
                                    level: 2
                                    text: qsTr("生涯")
                                    color: Kirigami.Theme.textColor
                                }

                                Kirigami.Heading {
                                    level: 2
                                    text: root.formatNum(root.val(root.lifetime, "totalHumanTypedChars")) + " " + qsTr("字")
                                    color: Kirigami.Theme.highlightColor
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1

                                    Label {
                                        Layout.fillWidth: true
                                        text: root.lifetimeRangeText()
                                        color: Kirigami.Theme.textColor
                                        elide: Text.ElideRight
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        text: qsTr("平均 %1 字/分 · %2 字/天")
                                            .arg(root.formatAverage(root.val(root.lifetime, "averageCharsPerMinute")))
                                            .arg(root.formatAverage(root.val(root.lifetime, "averageCharsPerDay")))
                                        color: Kirigami.Theme.disabledTextColor
                                        elide: Text.ElideRight
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        text: qsTr("共计 %1 天").arg(root.formatNum(root.val(root.lifetime, "elapsedCalendarDays")))
                                        color: Kirigami.Theme.disabledTextColor
                                        elide: Text.ElideRight
                                    }
                                }

                                // events.local 不同步：生涯只代表本机可读历史，页面明确标注。
                                Label {
                                    visible: root.historyScope === "localEvents"
                                    text: qsTr("本机记录")
                                    color: Kirigami.Theme.disabledTextColor
                                    font: Kirigami.Theme.smallFont
                                }

                                Label {
                                    text: root.expandedPanel === "lifetime" ? "▲" : "▼"
                                    color: Kirigami.Theme.disabledTextColor
                                }
                            }

                            MouseArea {
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.toggleLifetime()
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            visible: root.expandedPanel === "lifetime"
                            Layout.preferredHeight: visible ? root.detailHeight : 0
                            spacing: Kirigami.Units.largeSpacing

                            StatsRecordPanel {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                records: root.selectedRecords
                            }

                            StatsTrendChart {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                trend: root.selectedTrend
                            }
                        }
                    }
                }

                // ── 宽屏：四卡横排总览，点一张原位展开三栏（生涯展开在顶部条内）──
                ColumnLayout {
                    Layout.fillWidth: true
                    visible: !root.narrowLayout && root.expandedPanel !== "lifetime"
                    spacing: Kirigami.Units.largeSpacing

                    GridLayout {
                        Layout.fillWidth: true
                        visible: root.expandedPanel === ""
                        columns: 4
                        columnSpacing: Kirigami.Units.largeSpacing
                        rowSpacing: Kirigami.Units.largeSpacing

                        Repeater {
                            model: root.periodCards

                            delegate: StatsPeriodCard {
                                required property var modelData
                                Layout.fillWidth: true
                                Layout.preferredHeight: Kirigami.Units.gridUnit * 10
                                periodSummary: modelData
                                expanded: false
                                onClicked: root.openPeriod(modelData.preset)
                            }
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.preferredHeight: root.detailHeight
                        visible: root.expandedPanel !== ""
                        spacing: Kirigami.Units.largeSpacing

                        // 左：当前时段摘要；点它返回四卡总览。
                        StatsPeriodCard {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 11
                            Layout.fillHeight: true
                            periodSummary: root.selected
                            expanded: true
                            onClicked: root.collapsePanel()
                        }

                        // 中：纪录。
                        StatsRecordPanel {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            records: root.selectedRecords
                        }

                        // 右：按日/周/月人工字数趋势（与旧本周速度曲线语义不同）。
                        StatsTrendChart {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            trend: root.selectedTrend
                        }
                    }
                }

                // ── 窄屏：范围切换 → 总量与区间 → 2×2 纪录 → 趋势，整体纵向滚动 ──
                ColumnLayout {
                    Layout.fillWidth: true
                    visible: root.narrowLayout && root.expandedPanel !== "lifetime"
                    spacing: Kirigami.Units.largeSpacing

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        Repeater {
                            model: root.periodCards

                            delegate: Button {
                                required property var modelData
                                Layout.fillWidth: true
                                text: root.periodTitle(modelData.preset)
                                highlighted: root.selectedPeriod === modelData.preset
                                onClicked: root.selectNarrowPeriod(modelData.preset)
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        Kirigami.Heading {
                            level: 1
                            Layout.fillWidth: true
                            text: root.formatNum(root.val(root.selected, "totalHumanTypedChars")) + " " + qsTr("字")
                            color: Kirigami.Theme.highlightColor
                        }

                        Label {
                            Layout.fillWidth: true
                            text: root.selectedRangeText()
                            color: Kirigami.Theme.textColor
                            elide: Text.ElideRight
                        }

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("平均 %1 字/分 · %2 字/天 · 有写作 %3 天")
                                .arg(root.formatAverage(root.val(root.selected, "averageCharsPerMinute")))
                                .arg(root.formatAverage(root.val(root.selected, "averageCharsPerDay")))
                                .arg(root.formatNum(root.val(root.selected, "writingDays")))
                            color: Kirigami.Theme.disabledTextColor
                            elide: Text.ElideRight
                        }
                    }

                    StatsRecordPanel {
                        Layout.fillWidth: true
                        records: root.selectedRecords
                        columns: 2
                    }

                    StatsTrendChart {
                        Layout.fillWidth: true
                        trend: root.selectedTrend
                    }
                }

                // ── 底部陪伴条：口径是自然日历跨度，不是实际写作天数 ──
                Kirigami.ShadowedRectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: Kirigami.Units.gridUnit * 2.6
                    radius: Kirigami.Units.cornerRadius
                    color: Kirigami.Theme.alternateBackgroundColor
                    border.color: Kirigami.Theme.disabledTextColor
                    border.width: 1

                    Label {
                        anchors.centerIn: parent
                        text: qsTr("素笺写作已陪伴您 %1 天").arg(root.formatNum(root.val(root.lifetime, "elapsedCalendarDays")))
                        color: Kirigami.Theme.textColor
                    }
                }

                // ── 详细统计（折叠）：作品 / 章节 / 设备排行与本周速度曲线 ──
                Kirigami.ShadowedRectangle {
                    id: detailedStatsBox
                    Layout.fillWidth: true
                    radius: Kirigami.Units.cornerRadius
                    color: Kirigami.Theme.alternateBackgroundColor
                    border.color: detailedHoverArea.containsMouse ? Kirigami.Theme.highlightColor : Kirigami.Theme.disabledTextColor
                    border.width: 1
                    implicitHeight: detailedStatsColumn.implicitHeight + Kirigami.Units.largeSpacing * 2

                    ColumnLayout {
                        id: detailedStatsColumn
                        anchors.top: parent.top
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: Kirigami.Units.largeSpacing
                        spacing: Kirigami.Units.smallSpacing

                        Item {
                            Layout.fillWidth: true
                            implicitHeight: detailedStatsHeaderRow.implicitHeight

                            RowLayout {
                                id: detailedStatsHeaderRow
                                anchors.fill: parent
                                spacing: Kirigami.Units.smallSpacing

                                Kirigami.Heading {
                                    level: 5
                                    Layout.fillWidth: true
                                    text: qsTr("详细统计")
                                    color: Kirigami.Theme.textColor
                                }

                                Label {
                                    visible: root.detailedStatsExpanded
                                    text: qsTr("作品 / 章节 / 设备 · 本周速度")
                                    color: Kirigami.Theme.disabledTextColor
                                    font: Kirigami.Theme.smallFont
                                }

                                Label {
                                    text: root.detailedStatsExpanded ? "▲" : "▼"
                                    color: Kirigami.Theme.disabledTextColor
                                }
                            }

                            MouseArea {
                                id: detailedHoverArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.toggleDetailedStats()
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: root.detailedStatsExpanded && root.dashboardRangeText().length > 0
                            text: root.dashboardRangeText()
                            color: Kirigami.Theme.disabledTextColor
                            font: Kirigami.Theme.smallFont
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: root.detailedStatsExpanded && root.dashboardError.length > 0
                            text: root.dashboardError
                            color: Kirigami.Theme.negativeTextColor
                            wrapMode: Text.Wrap
                        }

                        DashboardGrid {
                            id: dashboardGrid
                            Layout.fillWidth: true
                            visible: root.detailedStatsExpanded
                            dt: root.dt

                            GridLayout {
                                Layout.fillWidth: true
                                columns: dashboardGrid.wide ? 3 : (dashboardGrid.medium ? 2 : 1)
                                columnSpacing: dashboardGrid.gap
                                rowSpacing: dashboardGrid.gap

                                DashboardSection {
                                    dt: root.dt
                                    title: qsTr("本周速度曲线")
                                    Layout.fillWidth: true
                                    Layout.columnSpan: dashboardGrid.wide ? 2 : 1

                                    Canvas {
                                        id: speedCanvas
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 136
                                        visible: root.speedBuckets.length > 0

                                        onPaint: {
                                            var ctx = getContext("2d")
                                            ctx.clearRect(0, 0, width, height)
                                            var buckets = root.speedBuckets
                                            if (buckets.length === 0)
                                                return
                                            // Issue #935 复核：x 按 startMs 实际时间轴映射，
                                            // 断档按真实时间留白；y 按本页峰值归一化，峰值 0 时画底部平线。
                                            var peak = root.speedPeak()
                                            var pad = 6
                                            var innerWidth = Math.max(1, width - pad * 2)
                                            var innerHeight = Math.max(1, height - pad * 2)
                                            var minMs = Number(buckets[0].startMs)
                                            var maxMs = minMs
                                            for (var i = 1; i < buckets.length; i++) {
                                                var bucketStart = Number(buckets[i].startMs)
                                                if (bucketStart < minMs) minMs = bucketStart
                                                if (bucketStart > maxMs) maxMs = bucketStart
                                            }
                                            if (isNaN(minMs)) minMs = 0
                                            if (isNaN(maxMs)) maxMs = minMs
                                            var span = maxMs - minMs
                                            var bucketMs = Math.max(1, Number(root.dashboard && root.dashboard.speedCurve ? root.dashboard.speedCurve.bucketMinutes : 60) || 60) * 60000
                                            ctx.strokeStyle = Kirigami.Theme.highlightColor
                                            ctx.lineWidth = 2
                                            ctx.lineJoin = "round"
                                            ctx.lineCap = "round"
                                            ctx.beginPath()
                                            var previousMs = -1
                                            for (var j = 0; j < buckets.length; j++) {
                                                var rate = Number(buckets[j] && buckets[j].charsPerMinute)
                                                if (isNaN(rate) || rate < 0) rate = 0
                                                var bucketStartMs = Number(buckets[j].startMs)
                                                var x = span > 0 ? pad + innerWidth * (bucketStartMs - minMs) / span : pad + innerWidth / 2
                                                var y = pad + innerHeight * (1 - (peak > 0 ? rate / peak : 0))
                                                // 断档（超过 1.5 个桶宽没有数据）断开折线，按真实时间留白。
                                                if (previousMs < 0 || bucketStartMs - previousMs > bucketMs * 1.5)
                                                    ctx.moveTo(x, y)
                                                else
                                                    ctx.lineTo(x, y)
                                                previousMs = bucketStartMs
                                            }
                                            ctx.stroke()
                                            if (buckets.length === 1) {
                                                // 单点没有线段，画一个圆点让数据可见。
                                                ctx.beginPath()
                                                ctx.arc(pad + innerWidth / 2, pad + innerHeight * (1 - (peak > 0 ? Number(buckets[0].charsPerMinute) / peak : 0)), 3, 0, Math.PI * 2)
                                                ctx.fillStyle = Kirigami.Theme.highlightColor
                                                ctx.fill()
                                            }
                                        }

                                        Connections {
                                            target: root
                                            function onDashboardChanged() { speedCanvas.requestPaint() }
                                        }
                                        onWidthChanged: requestPaint()
                                        onHeightChanged: requestPaint()
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        visible: root.speedBuckets.length === 0
                                        text: qsTr("暂无数据")
                                        color: Kirigami.Theme.disabledTextColor
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        visible: root.speedBuckets.length > 0
                                        text: qsTr("峰值 %1 字/分 · %2 段").arg(Math.round(root.speedPeak())).arg(root.speedBuckets.length)
                                        color: Kirigami.Theme.disabledTextColor
                                        font: Kirigami.Theme.smallFont
                                    }
                                }

                                DashboardSection {
                                    dt: root.dt
                                    title: qsTr("设备统计")
                                    Layout.fillWidth: true

                                    Repeater {
                                        model: root.sortedDevices.slice(0, 4)

                                        delegate: RowLayout {
                                            required property var modelData
                                            Layout.fillWidth: true

                                            Label {
                                                Layout.fillWidth: true
                                                text: root.deviceLabel(modelData)
                                                color: Kirigami.Theme.textColor
                                                elide: Text.ElideRight
                                            }
                                            Label {
                                                text: root.formatNum(root.val(modelData, "humanTypedChars")) + qsTr(" 字")
                                                color: Kirigami.Theme.disabledTextColor
                                            }
                                        }
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        visible: root.sortedDevices.length === 0
                                        text: qsTr("暂无数据")
                                        color: Kirigami.Theme.disabledTextColor
                                    }
                                }
                            }

                            GridLayout {
                                Layout.fillWidth: true
                                columns: dashboardGrid.wide || dashboardGrid.medium ? 2 : 1
                                columnSpacing: dashboardGrid.gap
                                rowSpacing: dashboardGrid.gap

                                DashboardSection {
                                    dt: root.dt
                                    title: qsTr("作品排行")
                                    Layout.fillWidth: true

                                    Repeater {
                                        model: root.sortedProjects.slice(0, 5)

                                        delegate: RowLayout {
                                            required property int index
                                            required property var modelData
                                            Layout.fillWidth: true

                                            Label {
                                                Layout.fillWidth: true
                                                text: (index + 1) + ". " + root.projectTitleById(root.fieldString(modelData, "projectId"))
                                                color: Kirigami.Theme.textColor
                                                elide: Text.ElideRight
                                            }
                                            Label {
                                                text: root.formatNum(root.val(modelData, "humanTypedChars")) + qsTr(" 字")
                                                color: Kirigami.Theme.disabledTextColor
                                            }
                                        }
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        visible: root.sortedProjects.length === 0
                                        text: qsTr("暂无数据")
                                        color: Kirigami.Theme.disabledTextColor
                                    }
                                }

                                DashboardSection {
                                    dt: root.dt
                                    title: qsTr("章节排行")
                                    Layout.fillWidth: true

                                    Repeater {
                                        model: root.sortedChapters.slice(0, 5)

                                        delegate: RowLayout {
                                            required property int index
                                            required property var modelData
                                            Layout.fillWidth: true

                                            Label {
                                                Layout.fillWidth: true
                                                text: (index + 1) + ". " + root.chapterTitleById(root.fieldString(modelData, "chapterId"))
                                                color: Kirigami.Theme.textColor
                                                elide: Text.ElideRight
                                            }
                                            Label {
                                                text: root.formatNum(root.val(modelData, "humanTypedChars")) + qsTr(" 字")
                                                color: Kirigami.Theme.disabledTextColor
                                            }
                                        }
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        visible: root.sortedChapters.length === 0
                                        text: qsTr("暂无数据")
                                        color: Kirigami.Theme.disabledTextColor
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ── 自选区间 Dialog：两枚 KDE DatePopup 选起止日期，确认后按实际区间请求 ──
    Dialog {
        id: customRangeDialog
        title: qsTr("选择自选区间")
        modal: true
        anchors.centerIn: parent
        width: parent ? Math.min(parent.width - Kirigami.Units.largeSpacing * 2, Kirigami.Units.gridUnit * 22) : Kirigami.Units.gridUnit * 22
        standardButtons: Dialog.NoButton

        property date startValue: new Date()
        property date endValue: new Date()

        function dateFromIso(iso) {
            if (!iso) return null
            var parts = String(iso).split("-")
            if (parts.length !== 3) return null
            var d = new Date(Number(parts[0]), Number(parts[1]) - 1, Number(parts[2]))
            return isNaN(d.getTime()) ? null : d
        }

        function isoFromDate(d) {
            var y = d.getFullYear()
            var m = String(d.getMonth() + 1)
            if (m.length < 2) m = "0" + m
            var day = String(d.getDate())
            if (day.length < 2) day = "0" + day
            return y + "-" + m + "-" + day
        }

        function prepare(startIso, endIso) {
            var end = dateFromIso(endIso) || new Date()
            var start = dateFromIso(startIso) || new Date(end.getTime() - 6 * 86400000)
            startValue = start
            endValue = end
            customRangeError.text = ""
        }

        ColumnLayout {
            width: parent.width
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Label {
                    Layout.fillWidth: true
                    text: qsTr("开始日期")
                    color: Kirigami.Theme.disabledTextColor
                }

                Button {
                    text: customRangeDialog.isoFromDate(customRangeDialog.startValue)
                    onClicked: startDatePopup.open()
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Label {
                    Layout.fillWidth: true
                    text: qsTr("结束日期")
                    color: Kirigami.Theme.disabledTextColor
                }

                Button {
                    text: customRangeDialog.isoFromDate(customRangeDialog.endValue)
                    onClicked: endDatePopup.open()
                }
            }

            Label {
                id: customRangeError
                Layout.fillWidth: true
                visible: text.length > 0
                text: ""
                color: Kirigami.Theme.negativeTextColor
                wrapMode: Text.Wrap
            }
        }

        footer: DialogButtonBox {
            Button {
                text: qsTr("取消")
                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
            }
            // 用 ActionRole：AcceptRole 会自动发出 accepted() 并让 Dialog 关闭，
            // 校验失败时再 return 也拦不住；改为 ActionRole 后由 onClicked 全权处理。
            Button {
                text: qsTr("确定")
                highlighted: true
                DialogButtonBox.buttonRole: DialogButtonBox.ActionRole
                onClicked: {
                    if (customRangeDialog.startValue > customRangeDialog.endValue) {
                        customRangeError.text = qsTr("开始日期需早于或等于结束日期")
                        return
                    }
                    root.applyCustomRange(
                        customRangeDialog.isoFromDate(customRangeDialog.startValue),
                        customRangeDialog.isoFromDate(customRangeDialog.endValue))
                    customRangeDialog.close()
                }
            }
        }
    }

    // KDE 日期选择弹窗（Kirigami Addons）：选中日期后点「选择」确认并关闭。
    DateAndTime.DatePopup {
        id: startDatePopup
        value: customRangeDialog.startValue
        onAccepted: customRangeDialog.startValue = value
    }

    DateAndTime.DatePopup {
        id: endDatePopup
        value: customRangeDialog.endValue
        onAccepted: customRangeDialog.endValue = value
    }
}
