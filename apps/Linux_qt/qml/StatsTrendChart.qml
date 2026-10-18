// =============================================================================
// StatsTrendChart.qml — 统计页趋势折线图（按业务日期的人工码字量）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：用 Qt Quick Canvas 画 Core Explorer 快照 selected.trend 的折线：
//   - 横轴为业务日期桶（按日 / 按周 / 按月，粒度由 Core 下发）；
//   - 纵轴为桶内人工输入字数（单位：字）；
//   - 只画线 + 最少必要日期刻度，不引第三方图表依赖。
// 约束：
//   - 纯展示组件：只消费 trend，不查询磁盘、不重算统计；
//   - 与旧仪表盘的「本周速度曲线（字/分）」语义不同，不共用另一个图表；
//   - 数据变化与宽高变化时 requestPaint()，图表不是高频编辑帧。
// 参考：https://doc.qt.io/qt-6/qml-qtquick-canvas.html （onPaint + requestPaint）
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ShadowedRectangle {
    id: root

    /// Core Explorer 快照 selected.trend（WritingStatsTrendDto）；加载前为 null。
    property var trend: null
    property string title: qsTr("趋势")

    readonly property var points: trend && trend.points ? trend.points : []
    readonly property string granularity: trend && trend.granularity ? String(trend.granularity) : ""

    readonly property string granularityText: {
        switch (granularity) {
        case "day": return qsTr("按日")
        case "week": return qsTr("按周")
        case "month": return qsTr("按月")
        }
        return ""
    }

    radius: Kirigami.Units.cornerRadius
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: Kirigami.Theme.disabledTextColor
    border.width: 1

    // 窄屏自然高度按内容算；宽屏展开态由页面给 fillHeight。
    implicitHeight: panelColumn.implicitHeight + Kirigami.Units.largeSpacing * 2

    function formatNum(n) {
        if (n === null || n === undefined) return "—"
        var v = Number(n)
        return isNaN(v) ? "—" : v.toLocaleString()
    }

    // 桶起始日期 → 刻度文本：按月取「年/月」，其余取「月/日」。
    function formatTick(iso) {
        if (!iso) return ""
        var parts = String(iso).split("-")
        if (parts.length !== 3) return String(iso)
        if (root.granularity === "month") return Number(parts[0]) + "/" + Number(parts[1])
        return Number(parts[1]) + "/" + Number(parts[2])
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
                level: 5
                Layout.fillWidth: true
                text: root.title
                color: Kirigami.Theme.textColor
                elide: Text.ElideRight
            }

            Label {
                visible: root.granularityText.length > 0 && root.points.length > 0
                text: root.granularityText + " · " + qsTr("单位：字")
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
            }
        }

        Canvas {
            id: chartCanvas
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 8
            visible: root.points.length > 0

            onWidthChanged: requestPaint()
            onHeightChanged: requestPaint()

            Connections {
                target: root
                function onTrendChanged() { chartCanvas.requestPaint() }
            }

            onPaint: {
                var ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                var points = root.points
                if (points.length === 0)
                    return

                var fontSize = Math.max(8, Math.round(Kirigami.Units.gridUnit * 0.55))
                ctx.font = fontSize + "px sans-serif"

                var padLeft = Math.max(Kirigami.Units.gridUnit * 1.6, fontSize * 3)
                var padRight = Kirigami.Units.smallSpacing * 2
                var padTop = fontSize * 1.6
                var padBottom = fontSize * 1.8
                var plotWidth = Math.max(1, width - padLeft - padRight)
                var plotHeight = Math.max(1, height - padTop - padBottom)

                var labelColor = Kirigami.Theme.disabledTextColor
                var lineColor = Kirigami.Theme.highlightColor

                // 峰值：全 0 时按 1 处理，画一条贴底的平线；不把空数据画成高点。
                var maxValue = 0
                for (var i = 0; i < points.length; i++) {
                    var value = Number(points[i] && points[i].humanTypedChars)
                    if (!isNaN(value) && value > maxValue) maxValue = value
                }
                if (maxValue <= 0) maxValue = 1

                // Y 轴：0 / 中值 / 峰值三条横线 + 刻度文本。
                ctx.strokeStyle = Qt.rgba(labelColor.r, labelColor.g, labelColor.b, 0.3)
                ctx.lineWidth = 1
                ctx.fillStyle = labelColor
                ctx.textAlign = "right"
                ctx.textBaseline = "middle"
                for (var tick = 0; tick <= 2; tick++) {
                    var ratio = tick / 2
                    var y = padTop + plotHeight * (1 - ratio)
                    ctx.beginPath()
                    ctx.moveTo(padLeft, y)
                    ctx.lineTo(padLeft + plotWidth, y)
                    ctx.stroke()
                    ctx.fillText(root.formatNum(Math.round(maxValue * ratio)), padLeft - Kirigami.Units.smallSpacing, y)
                }

                // Y 轴单位。
                ctx.fillText(qsTr("字"), padLeft - Kirigami.Units.smallSpacing, padTop - fontSize * 0.6)

                // 曲线：x 按桶序均分，y 按峰值归一化。
                var xAt = function(index) {
                    if (points.length === 1) return padLeft + plotWidth / 2
                    return padLeft + plotWidth * index / (points.length - 1)
                }
                var yAt = function(value) {
                    var v = Number(value)
                    if (isNaN(v) || v < 0) v = 0
                    return padTop + plotHeight * (1 - v / maxValue)
                }

                ctx.beginPath()
                for (var j = 0; j < points.length; j++) {
                    var x = xAt(j)
                    var py = yAt(points[j] && points[j].humanTypedChars)
                    if (j === 0) ctx.moveTo(x, py)
                    else ctx.lineTo(x, py)
                }
                // 曲线下方淡淡填充，不遮挡折线本身。
                ctx.fillStyle = Qt.rgba(lineColor.r, lineColor.g, lineColor.b, 0.14)
                ctx.lineTo(xAt(points.length - 1), padTop + plotHeight)
                ctx.lineTo(xAt(0), padTop + plotHeight)
                ctx.closePath()
                ctx.fill()

                ctx.beginPath()
                for (var k = 0; k < points.length; k++) {
                    var lx = xAt(k)
                    var ly = yAt(points[k] && points[k].humanTypedChars)
                    if (k === 0) ctx.moveTo(lx, ly)
                    else ctx.lineTo(lx, ly)
                }
                ctx.strokeStyle = lineColor
                ctx.lineWidth = 2
                ctx.lineJoin = "round"
                ctx.lineCap = "round"
                ctx.stroke()

                // 点数不多时补数据点，让稀疏数据在折线上也可见。
                if (points.length <= 31) {
                    ctx.fillStyle = lineColor
                    for (var d = 0; d < points.length; d++) {
                        ctx.beginPath()
                        ctx.arc(xAt(d), yAt(points[d] && points[d].humanTypedChars), 2, 0, Math.PI * 2)
                        ctx.fill()
                    }
                }

                // X 轴：最多 5 个日期刻度（含首尾），避免互相重叠。
                var tickCount = Math.min(5, points.length)
                ctx.fillStyle = labelColor
                ctx.textAlign = "center"
                ctx.textBaseline = "top"
                var lastLabelX = -1
                for (var t = 0; t < tickCount; t++) {
                    var index = tickCount === 1 ? 0 : Math.round(t * (points.length - 1) / (tickCount - 1))
                    var text = root.formatTick(points[index] && points[index].date)
                    var textWidth = ctx.measureText(text).width
                    var tx = Math.min(Math.max(xAt(index), textWidth / 2 + 1), width - textWidth / 2 - 1)
                    if (tx - lastLabelX < textWidth * 0.9)
                        continue
                    ctx.fillText(text, tx, padTop + plotHeight + Kirigami.Units.smallSpacing)
                    lastLabelX = tx
                }
            }
        }

        Label {
            Layout.fillWidth: true
            visible: root.points.length === 0
            text: qsTr("暂无数据")
            color: Kirigami.Theme.disabledTextColor
        }
    }
}
