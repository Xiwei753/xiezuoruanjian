// =============================================================================
// StatsRecordPanel.qml — 统计页纪录面板（最多码字日 / 单小时最多码字 / 最长会话 / 连续码字）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：复用的纪录数据列，渲染 Core Explorer 快照 selected.records：
//   - 最多码字日：日期 + 字数（字）；
//   - 单小时最多码字：窗口起始日 + 60 分钟窗口字数（字）；
//   - 最长码字会话：开始日 + 时长（时/分）；
//   - 连续码字：起止日期段 + 天数（天）。
// 约束：
//   - 纯展示组件：只消费 records，不查询磁盘、不重算纪录；
//   - 无事实依据的纪录（value/date 为 null）显示 "— / 暂无记录"，不用 0 冒充；
//   - columns=1 为宽屏展开态的中列数据列，columns=2 为窄屏 2×2 格子。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ShadowedRectangle {
    id: root

    /// Core Explorer 快照 selected.records（WritingStatsRecordsDto）；加载前为 null。
    property var records: null
    property string title: qsTr("纪录")
    property int columns: 1
    /// 在页面统一外框内作为无边界数据列显示。
    property bool flat: false

    radius: Kirigami.Units.cornerRadius
    color: root.flat ? "transparent" : Kirigami.Theme.alternateBackgroundColor
    border.color: root.flat ? "transparent" : Kirigami.Theme.disabledTextColor
    border.width: root.flat ? 0 : 1

    // 窄屏自然高度按内容算；宽屏展开态由页面给 fillHeight。
    implicitHeight: panelColumn.implicitHeight + Kirigami.Units.largeSpacing * 2

    function recordValue(key) {
        if (!root.records || !root.records[key]) return null
        return root.records[key]
    }

    function formatNum(n) {
        if (n === null || n === undefined) return "—"
        var v = Number(n)
        return isNaN(v) ? "—" : v.toLocaleString()
    }

    function formatDate(iso) {
        if (!iso) return ""
        var parts = String(iso).split("-")
        if (parts.length !== 3) return String(iso)
        return Number(parts[0]) + "/" + Number(parts[1]) + "/" + Number(parts[2])
    }

    // 时长秒数 → 「N时N分」；不足一分钟按分钟显示，避免出现 0时0分。
    function formatDuration(seconds) {
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
        return h > 0 ? (h + qsTr("时") + m + qsTr("分")) : (Math.max(1, m) + qsTr("分"))
    }

    readonly property var entries: {
        var bestDay = root.recordValue("bestDay")
        var fastestHour = root.recordValue("fastestHour")
        var longestSession = root.recordValue("longestSession")
        var longestStreak = root.recordValue("longestStreak")
        return [
            {
                "label": qsTr("最多码字日"),
                "value": bestDay && bestDay.value !== null && bestDay.value !== undefined ? (root.formatNum(bestDay.value) + " " + qsTr("字")) : "—",
                "date": bestDay && bestDay.date ? root.formatDate(bestDay.date) : ""
            },
            {
                "label": qsTr("单小时最多码字"),
                "value": fastestHour && fastestHour.value !== null && fastestHour.value !== undefined ? (root.formatNum(fastestHour.value) + " " + qsTr("字")) : "—",
                "date": fastestHour && fastestHour.date ? root.formatDate(fastestHour.date) : ""
            },
            {
                "label": qsTr("最长码字"),
                "value": longestSession && longestSession.value !== null && longestSession.value !== undefined ? root.formatDuration(longestSession.value) : "—",
                "date": longestSession && longestSession.date ? root.formatDate(longestSession.date) : ""
            },
            {
                "label": qsTr("连续码字"),
                "value": longestStreak && longestStreak.value !== null && longestStreak.value !== undefined ? (root.formatNum(longestStreak.value) + " " + qsTr("天")) : "—",
                "date": longestStreak && longestStreak.date
                      ? (root.formatDate(longestStreak.date) + (longestStreak.endDate ? " – " + root.formatDate(longestStreak.endDate) : ""))
                      : ""
            }
        ]
    }

    ColumnLayout {
        id: panelColumn
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        Kirigami.Heading {
            level: 4
            Layout.fillWidth: true
            text: root.title
            color: Kirigami.Theme.textColor
            visible: !root.flat
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: root.columns === 1
            visible: root.columns === 1

            ColumnLayout {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: root.entries

                    delegate: RowLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.largeSpacing

                        Label {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 7
                            text: modelData.label
                            color: Kirigami.Theme.disabledTextColor
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2

                            Kirigami.Heading {
                                level: 3
                                Layout.fillWidth: true
                                text: modelData.value
                                color: Kirigami.Theme.textColor
                                wrapMode: Text.Wrap
                            }

                            Label {
                                Layout.fillWidth: true
                                text: modelData.date.length ? modelData.date : qsTr("暂无记录")
                                font: Kirigami.Theme.smallFont
                                color: Kirigami.Theme.disabledTextColor
                                wrapMode: Text.Wrap
                            }
                        }
                    }
                }
            }
        }

        GridLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: root.columns === 2
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.largeSpacing

            Repeater {
                model: root.entries

                delegate: ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: Kirigami.Units.smallSpacing

                    Kirigami.Heading {
                        level: 2
                        Layout.fillWidth: true
                        text: modelData.value
                        color: Kirigami.Theme.textColor
                        elide: Text.ElideRight
                    }

                    Label {
                        Layout.fillWidth: true
                        text: modelData.label
                        color: Kirigami.Theme.disabledTextColor
                        elide: Text.ElideRight
                    }

                    Label {
                        Layout.fillWidth: true
                        text: modelData.date.length > 0 ? modelData.date : qsTr("暂无记录")
                        color: Kirigami.Theme.disabledTextColor
                        font: Kirigami.Theme.smallFont
                        elide: Text.ElideRight
                    }

                    Item { Layout.fillHeight: true }
                }
            }
        }
    }
}
