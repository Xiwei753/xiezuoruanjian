// =============================================================================
// StatsCalendarHeatmap.qml — 统计页展开区的时间范围热力图（Issue #976）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：用 Qt Quick Canvas 绘制 Core Explorer 快照 selected.heatmapDays：
//   - 只画当前选中区间 [startDate, endDate] 的逐日真值，不固定成某个公历年；
//   - X 轴一列一个自然周，Y 轴固定周一至周日七行；
//   - 短区间放大格子并居中，长区间保持可辨认格宽并让热力图自身横向滚动；
//   - 已确认的日期按 Core 下发的 intensity（0–4）着色，未知/未来单独呈现；
//   - 悬停或点击日期时显示当天日期与实际人工输入字数；
//   - 选中日期后追加 Core contributions.dayActors 的逐 actor 对象/AI 操作与
//     本地小时切片（#1014），没有明细时明确说没有记录，不补造 0。
// 约束：
//   - 纯展示组件：不查询磁盘、不重算每日字数或色阶，只消费 days / dayDetails；
//   - 热力颜色只由 Core 的人工输入字数与 intensity 决定，绝不把 AI 操作量
//     或对象操作混进人工码字；
//   - 不把趋势的周/月聚合点拆成每日格子，也不补造区间外的日期；
//   - 颜色全部来自 Kirigami.Theme，深浅色切换时重绘，不硬编码绿色。
// 参考：Qt Canvas https://doc.qt.io/qt-6/qml-qtquick-canvas.html
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    /// 当前区间的首末日期（YYYY-MM-DD，均来自 Core Explorer selected.range）。
    property string startDate: ""
    property string endDate: ""
    /// Core 下发的逐日热力格（WritingStatsHeatmapDayDto 列表，日期升序）。
    property var days: []
    /// Core contributions.dayActors：当前区间逐日 actor 对象操作明细（可选）。
    property var dayDetails: []
    /// 当前 Explorer 是否带有贡献明细；false 时不把「没有明细」说成「没有操作」。
    property bool hasContributionData: false
    /// 历史口径：localEvents / syncedDevices / partialSync。
    property string historyScope: ""

    readonly property var dayByDate: {
        var map = ({})
        for (var i = 0; i < days.length; i++) {
            var day = days[i]
            if (day && day.date)
                map[String(day.date)] = day
        }
        return map
    }

    // 按日期归并 Core 的逐日 actor 明细；没有明细的日期不生成空对象。
    readonly property var detailByDate: {
        var map = ({})
        for (var i = 0; i < dayDetails.length; i++) {
            var detail = dayDetails[i]
            if (!detail || !detail.date)
                continue
            var key = String(detail.date)
            if (!map[key])
                map[key] = []
            map[key].push(detail)
        }
        return map
    }

    readonly property var selectedActors: selectedDate !== ""
        ? (detailByDate[selectedDate] || [])
        : []

    // 一周一列：严格按区间首末日期组成所需周列，首尾不完整周保留 outside 占位。
    readonly property var weeks: buildWeeks()
    readonly property bool hasData: weeks.length > 0

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

    readonly property string rangeText: {
        if (startDate === "" || endDate === "")
            return ""
        return formatDateLabel(startDate) + " – " + formatDateLabel(endDate)
    }

    // 格子尺寸：同时受可用宽高约束；短区间放大到可读上限并居中。
    readonly property int minCellSize: 12
    readonly property int maxCellSize: 36
    readonly property int cellGap: 3
    readonly property int rightPadding: Kirigami.Units.smallSpacing * 2
    readonly property int leftGutter: Kirigami.Units.gridUnit * 1.8
    readonly property int topGutter: Kirigami.Units.gridUnit * 1.5
    readonly property int bottomPadding: Kirigami.Units.smallSpacing * 2
    readonly property int chartMinHeight: topGutter + 7 * 16 + 6 * cellGap + bottomPadding

    readonly property int cellSize: {
        var columns = Math.max(1, weeks.length)
        var gaps = Math.max(0, columns - 1) * cellGap
        var availableWidth = Math.max(1, chartFlick.width - leftGutter - rightPadding)
        var widthSize = Math.floor((availableWidth - gaps) / columns)
        var availableHeight = Math.max(1, chartFlick.height - topGutter - bottomPadding)
        var heightSize = Math.floor((availableHeight - 6 * cellGap) / 7)
        var size = Math.min(widthSize, heightSize)
        return Math.max(minCellSize, Math.min(maxCellSize, size))
    }
    readonly property int gridWidth: leftGutter + weeks.length * cellSize + Math.max(0, weeks.length - 1) * cellGap
    // 宽视口封顶格宽后把整块网格居中，避免所有格子挤在左侧。
    readonly property int gridOffset: Math.max(0, Math.floor((chartFlick.width - gridWidth) / 2))
    readonly property int gridHeight: topGutter + 7 * cellSize + 6 * cellGap + bottomPadding
    readonly property int canvasWidth: Math.max(1, Math.max(chartFlick.width, gridWidth + rightPadding))
    readonly property int canvasHeight: Math.max(gridHeight, chartFlick.height)
    readonly property int gridOffsetY: Math.max(0, Math.floor((canvasHeight - gridHeight) / 2))

    // 悬停 / 点击的日期只存字符串，快照刷新后仍从最新 dayByDate 取字数。
    property string hoveredDate: ""
    property string selectedDate: ""
    readonly property var activeDay: dayByDate[hoveredDate !== "" ? hoveredDate : selectedDate] || null

    // 主题颜色变化会更新这两个绑定；Canvas 用它们作为重绘触发器。
    readonly property color themeTextColor: Kirigami.Theme.textColor
    readonly property color themeHighlightColor: Kirigami.Theme.highlightColor

    implicitWidth: panelColumn.implicitWidth
    implicitHeight: panelColumn.implicitHeight

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

    function formatDateLabel(iso) {
        var parts = String(iso).split("-")
        if (parts.length !== 3) return String(iso)
        return Number(parts[0]) + "/" + Number(parts[1]) + "/" + Number(parts[2])
    }

    function mondayOf(date) {
        var cursor = new Date(date.getFullYear(), date.getMonth(), date.getDate())
        cursor.setDate(cursor.getDate() - ((cursor.getDay() + 6) % 7))
        return cursor
    }

    function buildWeeks() {
        var first = dateFromIso(startDate)
        var last = dateFromIso(endDate)
        if (!first || !last) {
            // 页面尚未给出区间时，退回 Core 日格的首末日期（不自行发明范围）。
            if (days.length === 0)
                return []
            first = dateFromIso(days[0].date)
            last = dateFromIso(days[days.length - 1].date)
        }
        if (!first || !last || first.getTime() > last.getTime())
            return []

        var cursor = mondayOf(first)
        var result = []
        while (cursor.getTime() <= last.getTime()) {
            var week = []
            for (var row = 0; row < 7; row++) {
                var date = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + row)
                var iso = isoFromDate(date)
                if (date.getTime() < first.getTime() || date.getTime() > last.getTime()) {
                    week.push({
                        "date": iso,
                        "humanTypedChars": 0,
                        "intensity": 0,
                        "status": "outside"
                    })
                } else {
                    // 区间内缺失的日期按「未知」处理，不伪造真实 0。
                    week.push(dayByDate[iso] || {
                        "date": iso,
                        "humanTypedChars": 0,
                        "intensity": 0,
                        "status": "unknown"
                    })
                }
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

    function countOf(detail, key) {
        return detail && detail.counts ? detail.counts[key] : null
    }

    // 只翻译 Core 的 actor / entrypoint 键，不在这里归类或重算统计。
    function actorLabel(actor) {
        switch (String(actor)) {
        case "human": return qsTr("人工")
        case "ai": return qsTr("AI")
        case "system": return qsTr("系统")
        }
        return qsTr("未知")
    }

    function entrypointLabel(entrypoint) {
        switch (String(entrypoint)) {
        case "ui": return qsTr("界面")
        case "mcp": return qsTr("MCP")
        case "import": return qsTr("导入")
        case "sync": return qsTr("同步")
        case "system": return qsTr("系统")
        }
        return qsTr("其他")
    }

    function formatHour(hour) {
        var value = Number(hour)
        if (isNaN(value)) return "—"
        var h = Math.floor(value)
        return (h < 10 ? "0" + h : String(h)) + ":00"
    }

    // 单日单 actor：操作量、新增/删除/修改字数都来自 Core 的事件聚合。
    function actorDetailText(detail) {
        var base = actorLabel(detail ? detail.actor : "") + " · "
            + qsTr("操作 %1 次 · 新增 %2 字 · 删除 %3 字 · 修改 %4 字")
                .arg(formatNum(countOf(detail, "operations")))
                .arg(formatNum(countOf(detail, "insertedChars")))
                .arg(formatNum(countOf(detail, "deletedChars")))
                .arg(formatNum(countOf(detail, "modifiedChars")))
        if (detail && detail.actor === "ai") {
            var direction = aiDirectionSummary(detail.counts)
            base += " · " + (direction !== ""
                ? direction
                : qsTr("请求输入/模型输出未区分"))
        }
        return base
    }

    // 只有事件源显式记录方向时才展示输入/输出；文本编辑量不能冒充请求输入。
    function aiDirectionSummary(counts) {
        if (!counts) return ""
        var recorded = Number(counts.aiDirectionOperations || 0)
        if (recorded <= 0) return ""
        return qsTr("请求输入 %1 字 · 模型输出 %2 字 · 方向记录 %3 次")
            .arg(formatNum(counts.aiInputChars))
            .arg(formatNum(counts.aiOutputChars))
            .arg(formatNum(recorded))
    }

    // 入口与可信覆盖：两者都来自事件字段，缺字段时只省略对应说明。
    function actorCoverageText(detail) {
        if (!detail) return ""
        var parts = []
        var entrypoints = detail.entrypoints || []
        var labels = []
        for (var i = 0; i < entrypoints.length; i++) {
            var item = entrypoints[i]
            if (!item) continue
            labels.push(entrypointLabel(item.entrypoint) + " " + formatNum(item.operations))
        }
        if (labels.length > 0)
            parts.push(qsTr("入口：") + labels.join(" · "))
        if (detail.trustedOperations !== undefined && detail.trustedOperations !== null) {
            parts.push(qsTr("可信记录 %1/%2 次")
                .arg(formatNum(detail.trustedOperations))
                .arg(formatNum(countOf(detail, "operations"))))
        }
        return parts.join(" · ")
    }

    // 事件原产地小时切片：事件自带发生小时才有；没有切片时整行不显示。
    function actorSliceText(detail) {
        var slices = detail && detail.slices ? detail.slices : []
        if (slices.length === 0) return ""
        var parts = []
        for (var i = 0; i < slices.length; i++) {
            var slice = slices[i]
            if (!slice) continue
            var text = formatHour(slice.hour) + " "
                + qsTr("操作 %1 · 新增 %2 · 删除 %3")
                    .arg(formatNum(countOf(slice, "operations")))
                    .arg(formatNum(countOf(slice, "insertedChars")))
                    .arg(formatNum(countOf(slice, "deletedChars")))
            if (detail.actor === "ai") {
                var direction = aiDirectionSummary(slice.counts)
                if (direction !== "")
                    text += " · " + direction
            }
            parts.push(text)
        }
        return qsTr("时段：") + parts.join("；")
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
            return root.hasContributionData
                ? qsTr("点击日期查看当天人工码字与 AI / 对象操作")
                : qsTr("悬停或点击日期查看当天人工输入字数")
        if (day.status === "unknown")
            return day.date + " · " + qsTr("历史覆盖不完整，无法确认当天字数")
        if (day.status === "future")
            return day.date + " · " + qsTr("尚未到来")
        return day.date + " · " + formatNum(day.humanTypedChars) + " " + qsTr("字（人工输入）")
    }

    // 鼠标坐标 → 格子：行列内的实际日期；落在间隙或区间外返回 null。
    function dayAt(px, py) {
        var localX = px - gridOffset
        var localY = py - gridOffsetY
        if (weeks.length === 0 || localX < leftGutter || localY < topGutter)
            return null
        var step = cellSize + cellGap
        var column = Math.floor((localX - leftGutter) / step)
        var row = Math.floor((localY - topGutter) / step)
        if (column < 0 || column >= weeks.length || row < 0 || row >= 7)
            return null
        if (localX - leftGutter - column * step > cellSize || localY - topGutter - row * step > cellSize)
            return null
        var day = weeks[column][row]
        return day && day.status !== "outside" ? day : null
    }

    function updateHover(px, py) {
        var day = dayAt(px, py)
        hoveredDate = day ? String(day.date) : ""
    }

    onStartDateChanged: {
        selectedDate = ""
        heatmapCanvas.requestPaint()
    }
    onEndDateChanged: {
        selectedDate = ""
        heatmapCanvas.requestPaint()
    }
    onDaysChanged: {
        if (selectedDate !== "" && !dayByDate[selectedDate])
            selectedDate = ""
        heatmapCanvas.requestPaint()
    }
    onWeeksChanged: heatmapCanvas.requestPaint()
    onCellSizeChanged: heatmapCanvas.requestPaint()
    onGridOffsetChanged: heatmapCanvas.requestPaint()
    onGridOffsetYChanged: heatmapCanvas.requestPaint()
    // 选中框与悬停由 Canvas 绘制，状态变化后必须主动重绘。
    onSelectedDateChanged: heatmapCanvas.requestPaint()
    onThemeTextColorChanged: heatmapCanvas.requestPaint()
    onThemeHighlightColorChanged: heatmapCanvas.requestPaint()

    ColumnLayout {
        id: panelColumn
        anchors.fill: parent
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
                visible: root.rangeText.length > 0
                text: root.rangeText
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

        // 窄视口或长区间横向滚动；短区间由格子尺寸铺满内框并居中。
        Flickable {
            id: chartFlick
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: root.chartMinHeight
            visible: root.hasData
            contentWidth: heatmapCanvas.width
            contentHeight: heatmapCanvas.height
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            // 桌面统计页禁止鼠标左键拖拽画布；横向移动交给滚轮/触摸板
            // 与下方滚动条，左键保留给 MouseArea 选日期。
            acceptedButtons: Qt.NoButton

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
                    var unknownColor = Qt.rgba(labelColor.r, labelColor.g, labelColor.b, 0.20)
                    var fontSize = Math.max(9, Math.round(Kirigami.Units.gridUnit * 0.5))

                    ctx.font = fontSize + "px sans-serif"
                    ctx.textBaseline = "middle"
                    ctx.textAlign = "right"
                    ctx.fillStyle = labelColor
                    var weekdayLabels = [qsTr("一"), qsTr("二"), qsTr("三"), qsTr("四"), qsTr("五"), qsTr("六"), qsTr("日")]
                    for (var w = 0; w < 7; w++) {
                        ctx.fillText(weekdayLabels[w],
                                     root.gridOffset + root.leftGutter - Kirigami.Units.smallSpacing,
                                     root.gridOffsetY + root.topGutter + w * step + cell / 2)
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
                        ctx.fillText(label, labelX, root.gridOffsetY + root.topGutter - Kirigami.Units.smallSpacing)
                        lastMonthX = x
                    }

                    // 日格：known 按 intensity 实心着色，unknown 空心，future/outside 不着色。
                    for (var c = 0; c < weeks.length; c++) {
                        for (var r = 0; r < 7; r++) {
                            var day = weeks[c][r]
                            if (!day || day.status === "outside" || day.status === "future")
                                continue
                            var x0 = root.gridOffset + root.leftGutter + c * step
                            var y0 = root.gridOffsetY + root.topGutter + r * step
                            if (day.status === "unknown") {
                                ctx.beginPath()
                                roundedRect(ctx, x0 + 0.5, y0 + 0.5, cell - 1, cell - 1, 2)
                                ctx.strokeStyle = unknownColor
                                ctx.lineWidth = 1
                                ctx.stroke()
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

        // 没有历史时保持简洁空状态，不画假热度。
        Label {
            Layout.fillWidth: true
            visible: !root.hasData
            text: qsTr("暂无每日写作数据")
            color: Kirigami.Theme.disabledTextColor
            horizontalAlignment: Text.AlignHCenter
        }

        Label {
            Layout.fillWidth: true
            visible: root.hasData
            text: root.dayDescription(root.activeDay)
            color: root.activeDay && root.activeDay.status === "unknown"
                ? Kirigami.Theme.disabledTextColor
                : Kirigami.Theme.textColor
            elide: Text.ElideRight
        }

        // ── 选中日期的对象/AI 操作明细（#1014）：只展示 Core 已按日、按 actor
        //    聚合的事实；没有明细就说没有记录，绝不把区间总量摊到当天。──
        ColumnLayout {
            Layout.fillWidth: true
            visible: root.hasData && root.selectedDate !== "" && root.hasContributionData
            spacing: 2

            Label {
                Layout.fillWidth: true
                visible: root.selectedActors.length > 0
                text: qsTr("当天对象操作（不计入人工码字）")
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }

            Label {
                Layout.fillWidth: true
                visible: root.selectedActors.length === 0
                text: qsTr("Core 未返回当天的 AI / 对象操作明细")
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }

            Repeater {
                model: root.selectedActors

                delegate: ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 0

                    Label {
                        Layout.fillWidth: true
                        text: root.actorDetailText(modelData)
                        color: Kirigami.Theme.textColor
                        font: Kirigami.Theme.smallFont
                        elide: Text.ElideRight
                    }

                    Label {
                        Layout.fillWidth: true
                        visible: text.length > 0
                        text: root.actorCoverageText(modelData)
                        color: Kirigami.Theme.disabledTextColor
                        font: Kirigami.Theme.smallFont
                        elide: Text.ElideRight
                    }

                    Label {
                        Layout.fillWidth: true
                        visible: text.length > 0
                        text: root.actorSliceText(modelData)
                        color: Kirigami.Theme.disabledTextColor
                        font: Kirigami.Theme.smallFont
                        elide: Text.ElideRight
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            visible: root.hasData
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
