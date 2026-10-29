// =============================================================================
// StatsPreviewPage.qml — 统计预览页面
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责（Issue #946 / #960 / #976）：按宽屏手稿重做统计主体 ——
//   - 顶部生涯条：累积人工码字、首末记录日期、平均字/时、平均字/天、
//     已持续自然日历天数；点击原位展开「左纪录 / 右热力图」；
//   - 四个时段卡横排（近七天 / 近三十天 / 近半年 / 自选区间）；点一张后
//     四卡区域原位替换为「左摘要 / 中纪录 / 右热力图」，可返回四卡总览；
//   - 自选区间先弹起止日期 Dialog，确认后以实际区间请求；
//   - 统计主体横向填满导航与顶栏之外的主画布，不再套 Hub 的限宽阅读栏；
//   - 热力图只出现在展开区右侧，随当前区间切换日期，不再另占页底一行；
//   - 不展示折线趋势图与「详细统计」。
// 约束：
//   - 纯展示层：只消费 editorBackendRef 的 Explorer 快照，不读文件系统、
//     不做统计聚合（每日字数、纪录、热力强度、连续天数、逐日贡献明细全在 Core）；
//     字段缺失显示 "—"，不用 0 冒充。
//   - Explorer 异步请求用递增 requestId 过滤迟到结果；失败保留上次数据，
//     只提示本次读取失败；不在 UI 线程同步读盘（沿用 #935 的 worker 队列）。
//   - 一级导航由 CreativeHub 桌面外壳负责，本页不造手机式底栏。
// 历史：
//   - Issue #935：统计页改为「单次异步 dashboard 快照」。
//   - Issue #946：主体改用 Explorer 快照（生涯条 + 四卡 + 原位展开）。
//   - Issue #960：填满主画布，并按手稿恢复比例。
//   - Issue #976：右侧恢复为区间热力图，移除折线图、页底全年热力图与
//     折叠「详细统计」；逐日热力格随 Explorer 一次返回。
//   - Issue #980：移除页内追踪副标题，主体含底部陪伴条整体上移，横条下留白。
//   - Issue #1014：移除独立「贡献来源」卡；逐日 actor 操作明细随
//     contributions.dayActors 交给热力图，在选中日期后就地展示。
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
    // selectedPeriod：当前选中时段；expandedPanel："" 四卡总览、"lifetime" 生涯，
    // 其余为原位展开的 preset。
    property string selectedPeriod: "last7Days"
    property string expandedPanel: ""
    property string startDate: ""
    property string endDate: ""

    // ── 桌面统计布局尺寸 ──
    // 宽屏展开区的最小可读高度；有余量时由 Layout.fillHeight 继续伸展。
    readonly property int detailMinHeight: Kirigami.Units.gridUnit * 20
    readonly property int lifetimeCollapsedHeight: Kirigami.Units.gridUnit * 7.5

    // ── 派生数据：字段缺失时退化为空，渲染层再决定显示 "—" ──
    readonly property var lifetime: explorer && explorer.lifetime ? explorer.lifetime : null
    readonly property var selected: explorer && explorer.selected ? explorer.selected : null
    readonly property var selectedRecords: selected ? selected.records : null
    readonly property var contributions: explorer && explorer.contributions ? explorer.contributions : null
    // 逐日 actor 明细（#1014）：只从 Core 的 contributions.dayActors 搬运，
    // 端侧不按区间总量拆日；旧快照没有该字段时为空数组。
    readonly property var contributionDays: contributions && contributions.dayActors ? contributions.dayActors : []
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

    function formatNum(n) {
        if (n === null || n === undefined) return "—"
        var v = Number(n)
        return isNaN(v) ? "—" : v.toLocaleString()
    }

    // Core 已按真实计时样本返回字/时；这里只做展示，不再乘 60。
    function formatAveragePerHour(n) {
        if (n === null || n === undefined) return "—"
        var v = Number(n)
        if (isNaN(v)) return "—"
        return formatAverage(v)
    }

    // 已测量写作时长：秒 → 「X 时 Y 分」；无计时样本返回 "—"。
    function formatMeasuredDuration(seconds) {
        if (seconds === null || seconds === undefined) return "—"
        var s = Number(seconds)
        if (isNaN(s)) return "—"
        s = Math.max(0, s)
        var h = Math.floor(s / 3600)
        var m = Math.round((s % 3600) / 60)
        if (m === 60) {
            h += 1
            m = 0
        }
        return h > 0 ? (h + qsTr("时") + m + qsTr("分")) : (m + qsTr("分"))
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

    function lifetimeRangeText() {
        if (!root.lifetime) return "—"
        var first = root.formatDate(root.lifetime.firstWritingDate)
        var last = root.formatDate(root.lifetime.lastWritingDate)
        if (first === "" && last === "") {
            return root.historyScope === "localEvents" ? qsTr("暂无本机记录") : qsTr("暂无记录")
        }
        return first + " – " + last
    }

    // 历史口径标注：Core 返回 localEvents / syncedDevices / partialSync，未知值不显示。
    function historyScopeText() {
        switch (root.historyScope) {
        case "localEvents": return qsTr("本机记录")
        case "syncedDevices": return qsTr("已同步设备记录")
        case "partialSync": return qsTr("同步不完整")
        }
        return ""
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

    function collapsePanel() {
        root.expandedPanel = ""
    }

    function toggleLifetime() {
        if (root.expandedPanel === "lifetime") {
            root.expandedPanel = ""
            return
        }
        root.expandedPanel = "lifetime"
        root.requestStats()
    }

    function applyCustomRange(startIso, endIso) {
        root.startDate = startIso
        root.endDate = endIso
        root.selectedPeriod = "custom"
        root.expandedPanel = "custom"
        root.requestStats()
    }

    Component.onCompleted: {
        requestStats()
    }
    onVisibleChanged: if (visible) {
        requestStats()
    }

    // ── Explorer 快照按 requestId 过滤迟到结果 ──
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
    }

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt
        // Issue #960：统计页填满导航与顶栏之外的主画布，不再套限宽阅读栏。
        useFullContentWidth: true
        // Issue #980：统计页不再显示页内“统计 / 追踪你的写作节奏与习惯”头部，
        // 让生涯条、四张卡片和底部陪伴条整体上移。
        showHeader: false

        Flickable {
            id: pageFlick
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: width
            contentHeight: pageColumn.height
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            // Issue #842：桌面鼠标左键不能按住空白处拖页面，保留滚轮/触摸板/滚动条。
            acceptedButtons: Qt.NoButton

            ColumnLayout {
                id: pageColumn
                width: pageFlick.width
                // Flickable 不是 Layout，可直接给高度；内容不足时略短于视口，
                // 使底部陪伴条下方留出自然空白（Issue #980）。
                height: Math.max(implicitHeight,
                                 pageFlick.height - Kirigami.Units.gridUnit * 4)
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

                // ── 生涯顶部条：收起时只显示摘要；展开后原位显示「左纪录 / 右热力图」──
                Kirigami.ShadowedRectangle {
                    id: lifetimeBox
                    Layout.fillWidth: root.expandedPanel === "lifetime"
                    Layout.preferredWidth: root.expandedPanel === "lifetime"
                        ? pageFlick.width
                        : Math.min(pageFlick.width,
                                   Math.max(Kirigami.Units.gridUnit * 46, pageFlick.width * 0.70))
                    Layout.alignment: Qt.AlignHCenter
                    // 生涯展开时由 Layout 把余量分给这条主展示区。
                    Layout.fillHeight: root.expandedPanel === "lifetime"
                    Layout.minimumHeight: root.expandedPanel === "lifetime"
                        ? root.detailMinHeight : root.lifetimeCollapsedHeight
                    Layout.preferredHeight: root.expandedPanel === "lifetime"
                        ? Math.max(Kirigami.Units.gridUnit * 26, Math.round(pageFlick.height * 0.55))
                        : Math.max(root.lifetimeCollapsedHeight,
                                   lifetimeColumn.implicitHeight + Kirigami.Units.largeSpacing * 2)
                    radius: Kirigami.Units.cornerRadius
                    color: Kirigami.Theme.alternateBackgroundColor
                    border.color: root.expandedPanel === "lifetime" ? Kirigami.Theme.highlightColor : Kirigami.Theme.disabledTextColor
                    border.width: 1
                    implicitHeight: lifetimeColumn.implicitHeight + Kirigami.Units.largeSpacing * 2

                    ColumnLayout {
                        id: lifetimeColumn
                        anchors.fill: parent
                        anchors.margins: Kirigami.Units.largeSpacing
                        spacing: Kirigami.Units.largeSpacing

                        Item {
                            Layout.fillWidth: true
                            implicitHeight: lifetimeHeaderRow.implicitHeight

                            RowLayout {
                                id: lifetimeHeaderRow
                                anchors.fill: parent
                                spacing: Kirigami.Units.largeSpacing

                                RowLayout {
                                    Layout.preferredWidth: implicitWidth
                                    spacing: Kirigami.Units.largeSpacing

                                    Kirigami.Heading {
                                        level: 1
                                        text: qsTr("生涯")
                                        color: Kirigami.Theme.textColor
                                    }

                                    Kirigami.Heading {
                                        level: 1
                                        Layout.fillWidth: true
                                        text: root.formatNum(root.val(root.lifetime, "totalHumanTypedChars")) + " " + qsTr("字")
                                        color: Kirigami.Theme.highlightColor
                                        elide: Text.ElideRight
                                    }
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                ColumnLayout {
                                    Layout.preferredWidth: Math.min(parent.width * 0.58,
                                                                   Math.max(Kirigami.Units.gridUnit * 24, implicitWidth))
                                    spacing: 1

                                    Label {
                                        Layout.fillWidth: true
                                        text: root.lifetimeRangeText()
                                        color: Kirigami.Theme.textColor
                                        elide: Text.ElideRight
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        text: qsTr("平均 %1 字/时 · %2 字/天")
                                            .arg(root.formatAveragePerHour(root.val(root.lifetime, "averageCharsPerHour")))
                                            .arg(root.formatAverage(root.val(root.lifetime, "averageCharsPerDay")))
                                        color: Kirigami.Theme.disabledTextColor
                                        elide: Text.ElideRight
                                    }

                                    Label {
                                        Layout.fillWidth: true
                                        text: root.val(root.lifetime, "hasMeasuredTiming") === true
                                            ? qsTr("共计 %1 天 · 实际计时 %2")
                                                .arg(root.formatNum(root.val(root.lifetime, "elapsedCalendarDays")))
                                                .arg(root.formatMeasuredDuration(root.val(root.lifetime, "measuredWritingSeconds")))
                                            : qsTr("共计 %1 天 · 仅统计新记录")
                                                .arg(root.formatNum(root.val(root.lifetime, "elapsedCalendarDays")))
                                        color: Kirigami.Theme.disabledTextColor
                                        elide: Text.ElideRight
                                    }

                                    // 历史口径来自 Core：本机 / 已同步设备 / 同步不完整，不因打开同步开关就宣称全量。
                                    Label {
                                        Layout.fillWidth: true
                                        visible: root.historyScopeText() !== ""
                                        text: root.historyScopeText()
                                        color: Kirigami.Theme.disabledTextColor
                                        font: Kirigami.Theme.smallFont
                                        elide: Text.ElideRight
                                    }
                                }

                                Label {
                                    Layout.alignment: Qt.AlignVCenter
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

                        // 桌面生涯展开固定为双列；窗口较窄时只滚动此局部内容。
                        Flickable {
                            id: lifetimeDetailFlick
                            Layout.fillWidth: true
                            Layout.fillHeight: root.expandedPanel === "lifetime"
                            Layout.minimumHeight: root.detailMinHeight
                            visible: root.expandedPanel === "lifetime"
                            clip: true
                            acceptedButtons: Qt.NoButton
                            contentWidth: Math.max(width, lifetimeDetailGrid.implicitWidth)
                            contentHeight: height
                            boundsBehavior: Flickable.StopAtBounds
                            ScrollBar.horizontal: ScrollBar {
                                policy: lifetimeDetailFlick.contentWidth > lifetimeDetailFlick.width
                                        ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff
                            }

                            GridLayout {
                                id: lifetimeDetailGrid
                                width: lifetimeDetailFlick.contentWidth
                                height: lifetimeDetailFlick.height
                                columns: 2
                                columnSpacing: Kirigami.Units.largeSpacing
                                rowSpacing: Kirigami.Units.largeSpacing

                                StatsRecordPanel {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    Layout.minimumWidth: Kirigami.Units.gridUnit * 18
                                    Layout.preferredWidth: 42
                                    records: root.selectedRecords
                                    flat: true
                                }

                                StatsCalendarHeatmap {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    Layout.minimumWidth: Kirigami.Units.gridUnit * 23
                                    Layout.preferredWidth: 58
                                    startDate: root.selected && root.selected.range ? root.selected.range.startDate : ""
                                    endDate: root.selected && root.selected.range ? root.selected.range.endDate : ""
                                    days: root.selected && root.selected.heatmapDays ? root.selected.heatmapDays : []
                                    dayDetails: root.contributionDays
                                    hasContributionData: root.contributions !== null
                                    historyScope: root.historyScope
                                }
                            }
                        }
                    }
                }

                // ── 宽屏：四卡横排总览，点一张原位展开三栏（生涯展开在顶部条内）──
                ColumnLayout {
                    Layout.fillWidth: true
                    // 总览/时段展开吃掉视口余量。
                    Layout.fillHeight: root.expandedPanel !== "lifetime"
                    visible: root.expandedPanel !== "lifetime"
                    spacing: Kirigami.Units.largeSpacing

                    Flickable {
                        id: periodCardsFlick
                        Layout.fillWidth: true
                        Layout.fillHeight: root.expandedPanel === ""
                        Layout.minimumHeight: Kirigami.Units.gridUnit * 16
                        visible: root.expandedPanel === ""
                        clip: true
                        acceptedButtons: Qt.NoButton
                        contentWidth: Math.max(width, periodCardGrid.implicitWidth)
                        contentHeight: height
                        boundsBehavior: Flickable.StopAtBounds
                        ScrollBar.horizontal: ScrollBar {
                            policy: periodCardsFlick.contentWidth > periodCardsFlick.width
                                    ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff
                        }

                        GridLayout {
                            id: periodCardGrid
                            width: periodCardsFlick.contentWidth
                            height: periodCardsFlick.height
                            columns: 4
                            columnSpacing: Kirigami.Units.largeSpacing
                            rowSpacing: Kirigami.Units.largeSpacing

                            Repeater {
                                model: root.periodCards

                                delegate: StatsPeriodCard {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    Layout.minimumWidth: Kirigami.Units.gridUnit * 12
                                    Layout.preferredHeight: Kirigami.Units.gridUnit * 18
                                    periodSummary: modelData
                                    expanded: false
                                    onClicked: root.openPeriod(modelData.preset)
                                }
                            }
                        }
                    }

                    Kirigami.ShadowedRectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: root.expandedPanel !== ""
                        Layout.minimumHeight: root.detailMinHeight
                        Layout.preferredHeight: Math.max(root.detailMinHeight,
                                                         Math.round(pageFlick.height * 0.48))
                        visible: root.expandedPanel !== ""
                        color: Kirigami.Theme.alternateBackgroundColor
                        radius: Kirigami.Units.cornerRadius
                        border.color: Kirigami.Theme.disabledTextColor
                        border.width: 1

                        Flickable {
                            id: periodDetailFlick
                            anchors.fill: parent
                            clip: true
                            acceptedButtons: Qt.NoButton
                            contentWidth: Math.max(width, periodDetailRow.implicitWidth
                                                   + Kirigami.Units.largeSpacing * 2)
                            contentHeight: height
                            boundsBehavior: Flickable.StopAtBounds
                            ScrollBar.horizontal: ScrollBar {
                                policy: periodDetailFlick.contentWidth > periodDetailFlick.width
                                        ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff
                            }

                            RowLayout {
                                id: periodDetailRow
                                x: Kirigami.Units.largeSpacing
                                y: Kirigami.Units.largeSpacing
                                width: Math.max(periodDetailFlick.width - Kirigami.Units.largeSpacing * 2,
                                                implicitWidth)
                                height: Math.max(0, periodDetailFlick.height - Kirigami.Units.largeSpacing * 2)
                                spacing: Kirigami.Units.largeSpacing

                                // 左：当前时段摘要；点它返回四卡总览。
                                StatsPeriodCard {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    Layout.minimumWidth: Kirigami.Units.gridUnit * 18
                                    Layout.preferredWidth: 24
                                    periodSummary: root.selected
                                    expanded: true
                                    onClicked: root.collapsePanel()
                                }

                                // 中：纪录；背景与边界由整张展开卡统一提供。
                                StatsRecordPanel {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    Layout.minimumWidth: Kirigami.Units.gridUnit * 21
                                    Layout.preferredWidth: 32
                                    records: root.selectedRecords
                                    flat: true
                                }

                                // 右：当前区间逐日热力图（与旧趋势折线图是两种图表）。
                                StatsCalendarHeatmap {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    Layout.minimumWidth: Kirigami.Units.gridUnit * 23
                                    Layout.preferredWidth: 44
                                    startDate: root.selected && root.selected.range ? root.selected.range.startDate : ""
                                    endDate: root.selected && root.selected.range ? root.selected.range.endDate : ""
                                    days: root.selected && root.selected.heatmapDays ? root.selected.heatmapDays : []
                                    dayDetails: root.contributionDays
                                    hasContributionData: root.contributions !== null
                                    historyScope: root.historyScope
                                }
                            }
                        }
                    }
                }

                // ── 底部陪伴条：口径是自然日历跨度，不是实际写作天数 ──
                Kirigami.ShadowedRectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: Kirigami.Units.gridUnit * 4
                    radius: Kirigami.Units.cornerRadius
                    color: Kirigami.Theme.alternateBackgroundColor
                    border.color: Kirigami.Theme.disabledTextColor
                    border.width: 1

                    Kirigami.Heading {
                        anchors.centerIn: parent
                        level: 3
                        text: qsTr("素笺写作已陪伴您 %1 天").arg(root.formatNum(root.val(root.lifetime, "elapsedCalendarDays")))
                        color: Kirigami.Theme.textColor
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
