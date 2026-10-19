// =============================================================================
// StatsCalendarHeatmap.qml — 统计页每日码字日历热力图（Issue #960）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：用 Qt Quick Canvas 绘制 Core 每日快照（WritingStatsHeatmapDto）：
//   - X 轴一列一个自然周（52/53 列），Y 轴固定周一至周日七行；
//   - 首尾不完整周按真实星期对齐，顶部标月份、左侧标星期；
//   - 已确认的日期按 Core 下发的 intensity（0–4）着色，未知/未来单独呈现；
//   - 悬停或点击日期时显示当天日期与实际人工输入字数。
// 约束：
//   - 纯展示组件：不查询磁盘、不重算每日字数或色阶，只消费 `calendar`；
//   - 与 StatsTrendChart 的按字数折线图是两种图表，不互相替代；
//   - 宽度不足时按最小可辨认格宽横向滚动，不为压进一屏把格子缩成细线；
//   - 颜色全部来自 Kirigami.Theme，深浅色切换时重绘，不硬编码绿色。
// 参考：Qt Canvas https://doc.qt.io/qt-6/qml-qtquick-canvas.html
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ShadowedRectangle {
    id: root

    /// Core 每日快照（WritingStatsHeatmapDto）；加载前为 null。
    property var calendar: null
    /// 本次读取失败时的提示；保留上次快照继续显示。
    property string errorText: ""

    readonly property var days: calendar && calendar.days ? calendar.days : []
    readonly property int year: calendar && calendar.year !== undefined ? Number(calendar.year) : 0
    readonly property string historyScope: calendar && calendar.historyScope ? String(calendar.historyScope) : ""

    // 日期字符串 → 当日 DTO，供格子反查；不把缺失日期伪造成真实 0。
    readonly property var dayByDate: {
        var map = ({})
        for (var i = 0; i < days.length; i++) {
            var day = days[i]
            if (day && day.date)
                map[String(day.date)] = day
        }
        return map
    }

    // 一周一列：从该年 1 月 1 日所在自然周的周一开始，到 12 月 31 日所在周结束。
    // 首尾不属于本年的格子标记为 outside，不画成已写作/未写作。
    readonly property var weeks: buildWeeks()

    readonly property int writingDays: {
        var count = 0
        for (var i = 0; i < days.length; i++) {
            var day = days[i]
            if (day && Number(day.humanTypedChars) > 0)
                count += 1
        }
        return count
    }

    readonly property string historyScopeText: {
        switch (historyScope) {
        case "localEvents": return qsTr("本机记录")
        case "syncedDevices": return qsTr("已同步设备记录")
        case "partialSync": return qsTr("同步不完整")
        }
        return ""
    }

    // 格子尺寸：优先铺满可用宽度；低于最小可辨认尺寸时改为横向滚动。
    readonly property int minCellSize: 12
    readonly property int maxCellSize: 30
    readonly property int cellGap: 3
    readonly property int rightPadding: Kirigami.Units.smallSpacing * 2
    readonly property int leftGutter: Kirigami.Units.gridUnit * 1.8
    readonly property int topGutter: Kirigami.Units.gridUnit * 1.5
    // 给横向滚动条留空间，避免盖住最下面一行周日格子。
    readonly property int bottomPadding: Kirigami.Units.gridUnit

    readonly property int cellSize: {
        var columns = Math.max(1, weeks.length)
        var available = Math.max(1, chartFlick.width - leftGutter - rightPadding)
        var gaps = Math.max(0, columns - 1) * cellGap
        var size = Math.floor((available - gaps) / columns)
        return Math.max(minCellSize, Math.min(maxCellSize, size))
    }
    readonly property int gridWidth: leftGutter + weeks.length * cellSize + Math.max(0, weeks.length - 1) * cellGap
    // 超宽窗口封顶格宽后把整块网格居中，避免所有格子挤在左侧。
    readonly property int gridOffset: Math.max(0, Math.floor((chartFlick.width - gridWidth) / 2))
    readonly property int canvasWidth: Math.max(1, Math.max(chartFlick.width, gridWidth + rightPadding))
    readonly property int canvasHeight: topGutter + 7 * cellSize + 6 * cellGap + bottomPadding

    // 悬停 / 点击的日期只存字符串，快照刷新后仍从最新 dayByDate 取字数。
    property string hoveredDate: ""
    property string selectedDate: ""
    readonly property var activeDay: dayByDate[hoveredDate !== "" ? hoveredDate : selectedDate] || null

    // 主题颜色变化会更新这两个绑定；Canvas 用它们作为重绘触发器，
    // 不依赖 Attached Property 上是否存在可直接连接的 change signal。
    readonly property color themeTextColor: Kirigami.Theme.textColor
    readonly property color themeHighlightColor: Kirigami.Theme.highlightColor

    radius: Kirigami.Units.cornerRadius
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: Kirigami.Theme.disabledTextColor
    border.width: 1

    implicitHeight: panelColumn.implicitHeight + Kirigami.Units.largeSpacing * 2

    onCalendarChanged: {
        if (selectedDate !== "" && !dayByDate[selectedDate])
            selectedDate = ""
        heatmapCanvas.requestPaint()
    }
    onWeeksChanged: heatmapCanvas.requestPaint()
    onCellSizeChanged: heatmapCanvas.requestPaint()
    onGridOffsetChanged: heatmapCanvas.requestPaint()
    onThemeTextColorChanged: heatmapCanvas.requestPaint()
    onThemeHighlightColorChanged: heatmapCanvas.requestPaint()

    function dateFromIso(iso) {
        if (!iso) return null
        var parts = String(iso).split("-")
        if (parts.length !== 3) return null
        var date = new Date(Number(parts[0]), Number(parts[1]) - 1, Number(parts[2]))
        return isNaN(date.getTime()) ? null : date
    }

    function isoFromDate(date) {
        var month = String(date.getMonth() + 1)
        if (month.length < 2) month = "0" + month
        var day = String(date.getDate())
        if (day.length < 2) day = "0" + day
        return date.getFullYear() + "-" + month + "-" + day
    }

    function buildWeeks() {
        if (days.length === 0)
            return []
        var first = dateFromIso(days[0].date)
        var last = dateFromIso(days[days.length - 1].date)
        if (!first || !last)
            return []

        // 回到该周周一；首尾不完整的周保留 outside 占位。
        var cursor = new Date(first.getFullYear(), first.getMonth(), first.getDate())
        cursor.setDate(cursor.getDate() - ((cursor.getDay() + 6) % 7))
        var result = []
        while (cursor.getTime() <= last.getTime()) {
            var week = []
            for (var row = 0; row < 7; row++) {
                var date = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + row)
                var iso = isoFromDate(date)
                var known = dayByDate[iso]
                week.push(known || {
                    "date": iso,
                    "humanTypedChars": 0,
                    "intensity": 0,
                    "status": "outside"
                })
            }
            result.push(week)
            cursor = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 7)
        }
        return result
    }

    function formatNum(n) {
        if (n === null || n === undefined) return "—"
        var value = Number(n)
        return isNaN(value) ? "—" : value.toLocaleString()
    }

    // Core 下发 0–4 强度；0 为低对比度底色，1–4 用主题强调色递进。
    function intensityColor(level) {
        var text = Kirigami.Theme.textColor
        var highlight = Kirigami.Theme.highlightColor
        switch (Number(level)) {
        case 1: return Qt.rgba(highlight.r, highlight.g, highlight.b, 0.20)
        case 2: return Qt.rgba(highlight.r, highlight.g, highlight.b, 0.38)
        case 3: return Qt.rgba(highlight.r, highlight.g, highlight.b, 0.62)
        case 4: return Qt.rgba(highlight.r, highlight.g, highlight.b, 0.88)
        }
        return Qt.rgba(text.r, text.g, text.b, 0.07)
    }

    function dayDescription(day) {
        if (!day)
            return qsTr("悬停或点击日期查看当天人工输入字数")
        if (day.status === "unknown")
            return day.date + " · " + qsTr("历史覆盖不完整，无法确认当天字数")
        if (day.status === "future")
            return day.date + " · " + qsTr("尚未到来")
        return day.date + " · " + formatNum(day.humanTypedChars) + " " + qsTr("字（人工输入）")
    }

    // 鼠标坐标 → 格子：行列内的实际日期；落在间隙或本年第 0 周之外返回 null。
    function dayAt(px, py) {
        var localX = px - gridOffset
        if (weeks.length === 0 || localX < leftGutter || py < topGutter)
            return null
        var step = cellSize + cellGap
        var column = Math.floor((localX - leftGutter) / step)
        var row = Math.floor((py - topGutter) / step)
        if (column < 0 || column >= weeks.length || row < 0 || row >= 7)
            return null
        if (localX - leftGutter - column * step > cellSize || py - topGutter - row * step > cellSize)
            return null
        var day = weeks[column][row]
        return day && day.status !== "outside" ? day : null
    }

    function updateHover(px, py) {
        var day = dayAt(px, py)
        hoveredDate = day ? String(day.date) : ""
    }

    ColumnLayout {
        id: panelColumn
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Heading {
                level: 4
                text: qsTr("每日码字热力图")
                color: Kirigami.Theme.textColor
            }

            Label {
                visible: root.year > 0
                text: root.year + qsTr(" 年")
                color: Kirigami.Theme.disabledTextColor
            }

            Item { Layout.fillWidth: true }

            Label {
                visible: root.historyScopeText.length > 0
                text: root.historyScopeText
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }

            Label {
                visible: root.writingDays > 0
                text: qsTr("写作 %1 天").arg(root.formatNum(root.writingDays))
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }
        }

        Label {
            Layout.fillWidth: true
            visible: root.errorText.length > 0
            text: root.errorText
            color: Kirigami.Theme.negativeTextColor
            wrapMode: Text.Wrap
        }

        // 窄视口横向滚动；宽视口由格子尺寸铺满整行。
        Flickable {
            id: chartFlick
            Layout.fillWidth: true
            Layout.preferredHeight: root.canvasHeight
            contentWidth: heatmapCanvas.width
            contentHeight: root.canvasHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Canvas {
                id: heatmapCanvas
                width: root.canvasWidth
                height: root.canvasHeight
                onWidthChanged: requestPaint()
                onHeightChanged: requestPaint()

                onPaint: {
                    var ctx = getContext("2d")
                    ctx.clearRect(0, 0, width, height)
                    var weeks = root.weeks
                    if (weeks.length === 0)
                        return

                    var cell = root.cellSize
                    var gap = root.cellGap
                    var step = cell + gap
                    var labelColor = Kirigami.Theme.disabledTextColor
                    var unknownColor = Qt.rgba(labelColor.r, labelColor.g, labelColor.b, 0.45)
                    var fontSize = Math.max(9, Math.round(Kirigami.Units.gridUnit * 0.5))

                    ctx.font = fontSize + "px sans-serif"
                    ctx.textBaseline = "middle"
                    ctx.textAlign = "right"
                    ctx.fillStyle = labelColor
                    var weekdayLabels = [qsTr("一"), qsTr("二"), qsTr("三"), qsTr("四"), qsTr("五"), qsTr("六"), qsTr("日")]
                    for (var w = 0; w < 7; w++) {
                        ctx.fillText(weekdayLabels[w],
                                     root.gridOffset + root.leftGutter - Kirigami.Units.smallSpacing,
                                     root.topGutter + w * step + cell / 2)
                    }

                    // 月份标在该月 1 日所在的列；过近的标签跳过，避免重叠。
                    ctx.textAlign = "left"
                    ctx.textBaseline = "bottom"
                    var lastMonthX = -9999
                    for (var col = 0; col < weeks.length; col++) {
                        var monthDay = null
                        for (var row = 0; row < 7; row++) {
                            var candidate = weeks[col][row]
                            if (candidate.status !== "outside" && String(candidate.date).slice(8) === "01") {
                                monthDay = candidate
                                break
                            }
                        }
                        if (!monthDay)
                            continue
                        var x = root.gridOffset + root.leftGutter + col * step
                        if (x - lastMonthX < cell * 2.4)
                            continue
                        var month = Number(String(monthDay.date).slice(5, 7))
                        var label = month + qsTr("月")
                        var labelWidth = ctx.measureText(label).width
                        var labelX = Math.min(x, width - labelWidth - 2)
                        ctx.fillStyle = labelColor
                        ctx.fillText(label, labelX, root.topGutter - Kirigami.Units.smallSpacing)
                        lastMonthX = x
                    }

                    // 日格：known 按 intensity 实心着色，unknown 空心，future/outside 不着色。
                    for (var c = 0; c < weeks.length; c++) {
                        for (var r = 0; r < 7; r++) {
                            var day = weeks[c][r]
                            if (!day || day.status === "outside" || day.status === "future")
                                continue
                            var x0 = root.gridOffset + root.leftGutter + c * step
                            var y0 = root.topGutter + r * step
                            if (day.status === "unknown") {
                                ctx.beginPath()
                                roundedRect(ctx, x0 + 0.5, y0 + 0.5, cell - 1, cell - 1, 2)
                                ctx.setLineDash([2, 2])
                                ctx.strokeStyle = unknownColor
                                ctx.lineWidth = 1
                                ctx.stroke()
                                ctx.setLineDash([])
                            } else {
                                ctx.beginPath()
                                roundedRect(ctx, x0, y0, cell, cell, 2)
                                ctx.fillStyle = root.intensityColor(day.intensity)
                                ctx.fill()
                            }
                            if (root.selectedDate !== "" && String(day.date) === root.selectedDate) {
                                ctx.beginPath()
                                roundedRect(ctx, x0 - 1, y0 - 1, cell + 2, cell + 2, 3)
                                ctx.strokeStyle = Kirigami.Theme.highlightColor
                                ctx.lineWidth = 1.5
                                ctx.stroke()
                            }
                        }
                    }
                }

                function roundedRect(ctx, x, y, w, h, radius) {
                    var r = Math.min(radius, w / 2, h / 2)
                    ctx.moveTo(x + r, y)
                    ctx.arcTo(x + w, y, x + w, y + h, r)
                    ctx.arcTo(x + w, y + h, x, y + h, r)
                    ctx.arcTo(x, y + h, x, y, r)
                    ctx.arcTo(x, y, x + w, y, r)
                    ctx.closePath()
                }
            }

            MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton
                cursorShape: root.hoveredDate !== "" ? Qt.PointingHandCursor : Qt.ArrowCursor

                // MouseArea 与 Canvas 同属 Flickable 的 contentItem，坐标已是内容坐标。
                onPositionChanged: root.updateHover(mouse.x, mouse.y)
                onExited: root.hoveredDate = ""
                onClicked: {
                    var day = root.dayAt(mouse.x, mouse.y)
                    root.selectedDate = day ? String(day.date) : ""
                }
            }

            ScrollBar.horizontal: ScrollBar {
                policy: ScrollBar.AsNeeded
            }
        }

        Label {
            Layout.fillWidth: true
            text: root.dayDescription(root.activeDay)
            color: root.activeDay && root.activeDay.status === "unknown"
                ? Kirigami.Theme.disabledTextColor
                : Kirigami.Theme.textColor
            elide: Text.ElideRight
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Label {
                text: qsTr("少")
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }

            Repeater {
                model: [0, 1, 2, 3, 4]

                Rectangle {
                    required property var modelData
                    implicitWidth: 12
                    implicitHeight: 12
                    radius: 2
                    color: root.intensityColor(modelData)
                }
            }

            Label {
                text: qsTr("多")
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }

            Rectangle {
                implicitWidth: 12
                implicitHeight: 12
                radius: 2
                color: "transparent"
                border.color: Kirigami.Theme.disabledTextColor
                border.width: 1

                // 中性线示意画布里的 unknown 空心格子（缺失不等于真实 0）。
                Rectangle {
                    anchors.centerIn: parent
                    width: parent.width
                    height: 1
                    color: Kirigami.Theme.disabledTextColor
                }
            }

            Label {
                text: qsTr("数据不完整")
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }

            Item { Layout.fillWidth: true }
        }
    }
}
