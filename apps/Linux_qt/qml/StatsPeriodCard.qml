// =============================================================================
// StatsPeriodCard.qml — 统计页单张时段卡（近七天 / 近三十天 / 近半年 / 自选区间）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：展示 Core Explorer 快照里的一张时段卡（字数 / 字每分 / 字每天 / 区间），
//   展开态下同一组件作为左侧当前时段摘要条。
// 约束：
//   - 纯展示组件：只消费 periodSummary（WritingStatsPeriodCardDto 或选中时段
//     DTO，同为 camelCase）与 expanded，不查询磁盘、不重算统计；
//     字段缺失显示 "—"，不用 0 冒充。
//   - 点击只发 clicked()，由 StatsPreviewPage 决定切换 expandedPanel /
//     打开自选日期 Dialog；本组件不自己改页面状态。
//   - Issue #946：自选区间尚未选择（hasSelection=false / range=null）显示未选择态。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ShadowedRectangle {
    id: root

    /// Core Explorer 快照中的时段卡对象；加载完成前为 null。
    property var periodSummary: null
    /// 是否处于展开态（左侧摘要条形态）。
    property bool expanded: false

    signal clicked()

    readonly property string preset: periodSummary && periodSummary.preset !== undefined ? String(periodSummary.preset) : ""
    /// 自选区间尚未选择时为 false；常规预设卡与选中时段 DTO 恒为 true。
    readonly property bool hasSelection: !!(periodSummary && periodSummary.hasSelection !== false && periodSummary.range)

    radius: Kirigami.Units.cornerRadius
    color: hoverArea.containsMouse ? Kirigami.Theme.activeBackgroundColor : Kirigami.Theme.alternateBackgroundColor
    border.color: expanded || hoverArea.containsMouse ? Kirigami.Theme.highlightColor : Kirigami.Theme.disabledTextColor
    border.width: 1

    Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
    Behavior on border.color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

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

    // 平均值最多保留一位小数，整数不带小数点；null 显示 "—"。
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

    readonly property string title: {
        switch (preset) {
        case "last7Days": return qsTr("近七天")
        case "last30Days": return qsTr("近三十天")
        case "last6Months": return qsTr("近半年")
        case "custom": return qsTr("自选区间")
        case "lifetime": return qsTr("生涯")
        }
        return ""
    }

    readonly property string rangeText: {
        if (!root.hasSelection || !root.periodSummary.range) return qsTr("点击选择区间")
        var range = root.periodSummary.range
        return root.formatDate(range.startDate) + " – " + root.formatDate(range.endDate)
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Math.round(Kirigami.Units.largeSpacing * 1.5)
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            // Issue #960：按宽屏手稿放大字号层次（标题 / 总字数 / 速度 / 日均 / 日期）。
            Kirigami.Heading {
                level: 4
                Layout.fillWidth: true
                text: root.title
                color: Kirigami.Theme.disabledTextColor
                font.pixelSize: Math.round(Kirigami.Units.gridUnit * 1.2)
                elide: Text.ElideRight
            }

            // 展开态箭头翻转（› → ‹）：展开卡点击即时返回四卡总览。
            Kirigami.Heading {
                level: 3
                text: "›"
                color: Kirigami.Theme.disabledTextColor
                rotation: root.expanded ? 180 : 0
            }
        }

        // 柔性间距只放在信息段之间，避免标题上方或日期下方出现大空洞。
        Item { Layout.fillHeight: true }

        Kirigami.Heading {
            level: 1
            Layout.fillWidth: true
            text: root.formatNum(root.val(root.periodSummary, "totalHumanTypedChars")) + " " + qsTr("字")
            color: Kirigami.Theme.highlightColor
            font.pixelSize: Math.round(Kirigami.Units.gridUnit * 1.65)
            elide: Text.ElideRight
        }

        Item { Layout.fillHeight: true }

        Kirigami.Heading {
            level: 3
            Layout.fillWidth: true
            text: root.formatAverage(root.val(root.periodSummary, "averageCharsPerMinute")) + " " + qsTr("字/分")
            color: Kirigami.Theme.textColor
            font.pixelSize: Math.round(Kirigami.Units.gridUnit * 1.1)
            elide: Text.ElideRight
        }

        Item { Layout.fillHeight: true }

        Kirigami.Heading {
            level: 3
            Layout.fillWidth: true
            text: root.formatAverage(root.val(root.periodSummary, "averageCharsPerDay")) + " " + qsTr("字/天")
            color: Kirigami.Theme.textColor
            font.pixelSize: Math.round(Kirigami.Units.gridUnit * 1.1)
            elide: Text.ElideRight
        }

        Item { Layout.fillHeight: true }

        Label {
            Layout.fillWidth: true
            text: root.rangeText
            color: Kirigami.Theme.disabledTextColor
            font: Kirigami.Theme.smallFont
            elide: Text.ElideRight
        }
    }

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
