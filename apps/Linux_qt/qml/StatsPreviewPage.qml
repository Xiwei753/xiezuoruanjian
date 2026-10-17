// =============================================================================
// StatsPreviewPage.qml — 统计预览页面
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：展示今日/本周/月度写作统计、作品/章节/设备排行与速度曲线
// 约束：
//   - 纯展示层：只消费 editorBackendRef 的 dashboard 快照，不读文件系统、不做统计聚合
//   - 快照字段为 Core 输出的 camelCase；字段缺失显示 "—"，不用 0 冒充
//   - 作品/章节标题用 appState.tree 解析，找不到时回退显示 ID，不编造名字
// Issue #935：统计页改为「单次异步 dashboard 快照」——
//   - request_writing_stats_dashboard(requestId) 一次取回全部统计，
//     不再在 UI 线程同步扫描事件文件（移除旧 get_* 同步调用）
//   - writing_stats_dashboard_ready/failed 按 requestId 过滤迟到结果；
//     失败时保留旧快照数字，错误独立显示
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property var editorBackendRef: null
    property var appState: ({})
    color: Kirigami.Theme.backgroundColor

    // ── 请求簿记：同一时刻只允许一个在途请求，用递增 requestId 丢弃迟到结果 ──
    property int requestSerial: 0
    property int activeRequestId: 0
    property var dashboard: null
    property string statsError: ""

    // ── 派生数据：字段缺失时退化为空数组，渲染层再决定显示 "—" ──
    readonly property var sortedProjects: root.sortByHumanTypedChars(root.dashboard && root.dashboard.projects ? root.dashboard.projects : [])
    readonly property var sortedChapters: root.sortByHumanTypedChars(root.dashboard && root.dashboard.chapters ? root.dashboard.chapters : [])
    readonly property var sortedDevices: root.sortByHumanTypedChars(root.dashboard && root.dashboard.devices ? root.dashboard.devices : [])
    readonly property var speedBuckets: root.dashboard && root.dashboard.speedCurve && root.dashboard.speedCurve.buckets ? root.dashboard.speedCurve.buckets : []

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

    function formatDuration(seconds) {
        if (seconds === null || seconds === undefined) return "—"
        var s = Number(seconds)
        if (isNaN(s)) return "—"
        s = Math.max(0, s)
        var h = Math.floor(s / 3600)
        var m = Math.floor((s % 3600) / 60)
        return h > 0 ? (h + qsTr("时") + m + qsTr("分")) : (m + qsTr("分"))
    }

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

    // 单次异步请求：进入页面各触发一次；旧请求的迟到结果按 requestId 丢弃。
    function requestStats() {
        if (!root.editorBackendRef) return
        root.requestSerial += 1
        root.activeRequestId = root.requestSerial
        root.statsError = ""
        root.editorBackendRef.request_writing_stats_dashboard(root.activeRequestId)
    }

    Component.onCompleted: requestStats()
    onVisibleChanged: if (visible) requestStats()

    Connections {
        target: root.editorBackendRef

        function onWriting_stats_dashboard_ready(requestId, snapshot) {
            if (requestId !== root.activeRequestId) return
            root.dashboard = snapshot
            root.statsError = ""
        }

        function onWriting_stats_dashboard_failed(requestId, message) {
            if (requestId !== root.activeRequestId) return
            // 保留旧快照数字，只提示本次读取失败；不清空 dashboard 冒充 0。
            root.statsError = String(message || qsTr("统计数据读取失败"))
        }
    }

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt
        headerData: [ HubPageHeader { dt: root.dt; title: qsTr("统计"); subtitle: qsTr("追踪你的写作节奏与习惯") } ]

        // Issue #715: 去掉 anchors.fill: parent，改用 Layout 属性。
        // Qt Quick Layouts 不允许父 Layout 管理 child 几何时再混用 anchors。
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // ── 错误条：与正常数据无关，独立显示 ──
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

            DashboardGrid {
                id: dashboardGrid
                Layout.fillWidth: true
                Layout.fillHeight: true
                dt: root.dt

                // ── 今日 / 本周 / 本月摘要 ──
                GridLayout {
                    Layout.fillWidth: true
                    columns: dashboardGrid.wide ? 4 : 2
                    columnSpacing: dashboardGrid.gap
                    rowSpacing: dashboardGrid.gap

                    StatCard {
                        dt: root.dt
                        Layout.fillWidth: true
                        Layout.preferredHeight: 120
                        title: qsTr("今日纯输入")
                        value: root.formatNum(root.dashboard && root.dashboard.today ? root.dashboard.today.totalHumanTypedChars : null)
                        caption: qsTr("人工输入字数")
                    }

                    StatCard {
                        dt: root.dt
                        Layout.fillWidth: true
                        Layout.preferredHeight: 120
                        title: qsTr("本周纯输入")
                        value: root.formatNum(root.dashboard && root.dashboard.week ? root.dashboard.week.totalHumanTypedChars : null)
                        caption: qsTr("人工输入字数")
                    }

                    StatCard {
                        dt: root.dt
                        Layout.fillWidth: true
                        Layout.preferredHeight: 120
                        title: qsTr("本月纯输入")
                        value: root.formatNum(root.dashboard && root.dashboard.month ? root.dashboard.month.totalHumanTypedChars : null)
                        caption: qsTr("人工输入字数")
                    }

                    StatCard {
                        dt: root.dt
                        Layout.fillWidth: true
                        Layout.preferredHeight: 120
                        title: qsTr("本周活跃时长")
                        value: root.formatDuration(root.dashboard && root.dashboard.week ? root.dashboard.week.totalActiveSeconds : null)
                        caption: qsTr("本周累计活跃秒数")
                    }
                }

                // ── 速度曲线 + 设备统计（快照口径同为本周）──
                GridLayout {
                    Layout.fillWidth: true
                    columns: dashboardGrid.wide ? 3 : (dashboardGrid.medium ? 2 : 1)
                    columnSpacing: dashboardGrid.gap
                    rowSpacing: dashboardGrid.gap

                    DashboardSection {
                        dt: root.dt
                        title: qsTr("速度曲线")
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

                // ── 作品 / 章节排行（快照口径同为本周）──
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
