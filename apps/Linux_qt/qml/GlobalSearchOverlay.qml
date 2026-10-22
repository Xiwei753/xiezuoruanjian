// =============================================================================
// GlobalSearchOverlay.qml — Linux Qt 全局搜索浮层
// =============================================================================
//
// 搜索数据、排序、分页游标与原文定位全部来自 Core；本文件只负责输入、
// 结果展示和把用户点击交回 main.qml 导航。打开时自动聚焦，Esc 关闭，
// 只有点击结果才跳转。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root
    anchors.fill: parent
    visible: false
    focus: visible
    z: 1000

    property var searchBackendRef: null
    property var context: ({})
    property var results: []
    property string nextCursor: ""
    property string pendingRequestId: ""
    property string message: ""
    property bool loading: false
    property bool loadingMore: false
    property bool searched: false

    signal resultActivated(var result)
    signal closed()

    function open(ctx) {
        context = ctx || ({})
        message = ""
        results = []
        nextCursor = ""
        pendingRequestId = ""
        searched = false
        loading = false
        loadingMore = false
        debounceTimer.stop()
        queryField.text = ""
        visible = true
        Qt.callLater(function() {
            queryField.forceActiveFocus()
        })
    }

    function close() {
        debounceTimer.stop()
        pendingRequestId = ""
        loading = false
        loadingMore = false
        visible = false
        closed()
    }

    function showMessage(text) {
        message = text || ""
    }

    function clearResults() {
        results = []
        nextCursor = ""
        searched = false
        loading = false
        loadingMore = false
        pendingRequestId = ""
    }

    function executeSearch(append) {
        if (!searchBackendRef) return
        var keyword = queryField.text.trim()
        if (keyword.length === 0) {
            clearResults()
            return
        }
        if (append && (!nextCursor || loading || loadingMore)) return

        loading = !append
        loadingMore = append
        message = ""
        pendingRequestId = searchBackendRef.search(
            keyword,
            "all",
            40,
            append ? nextCursor : "",
            JSON.stringify(context || ({})),
            append
        )
    }

    function finishSearch(requestId, pageJson, append, errorText) {
        if (String(requestId) !== String(pendingRequestId)) return
        pendingRequestId = ""
        loading = false
        loadingMore = false
        if (errorText && String(errorText).length > 0) {
            message = qsTr("搜索失败：%1").arg(String(errorText))
            return
        }
        var page = null
        try {
            page = JSON.parse(String(pageJson || "{}"))
        } catch (e) {
            message = qsTr("搜索结果格式异常")
            return
        }
        var items = page && page.items ? page.items : []
        results = append ? results.concat(items) : items
        nextCursor = page && page.nextCursor ? String(page.nextCursor) : ""
        searched = true
    }

    function utf16IndexForByte(text, targetByte) {
        var bytes = 0
        var index = 0
        while (index < text.length && bytes < targetByte) {
            var codePoint = text.codePointAt(index)
            var width = codePoint <= 0x7f ? 1
                : codePoint <= 0x7ff ? 2
                : codePoint <= 0xffff ? 3 : 4
            if (bytes + width > targetByte) break
            bytes += width
            index += codePoint > 0xffff ? 2 : 1
        }
        return index
    }

    function escapeHtml(text) {
        return String(text)
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#39;")
    }

    function summaryHtml(item) {
        var text = item && item.summary ? String(item.summary) : ""
        var ranges = item && item.summaryMatchRanges ? item.summaryMatchRanges : []
        if (!ranges || ranges.length === 0) return escapeHtml(text)

        var marks = []
        for (var i = 0; i < ranges.length; i += 1) {
            var range = ranges[i]
            if (!range || range.length < 2) continue
            var start = utf16IndexForByte(text, Number(range[0]))
            var end = utf16IndexForByte(text, Number(range[1]))
            if (end > start) marks.push({ start: start, end: end })
        }
        marks.sort(function(a, b) { return a.start - b.start || a.end - b.end })
        if (marks.length === 0) return escapeHtml(text)

        var highlight = String(Kirigami.Theme.highlightColor)
        var html = ""
        var cursor = 0
        for (var markIndex = 0; markIndex < marks.length; markIndex += 1) {
            var mark = marks[markIndex]
            if (mark.start < cursor || mark.end <= mark.start) continue
            html += escapeHtml(text.slice(cursor, mark.start))
            html += "<b><font color=\"" + highlight + "\">"
            html += escapeHtml(text.slice(mark.start, mark.end))
            html += "</font></b>"
            cursor = mark.end
        }
        html += escapeHtml(text.slice(cursor))
        return html
    }

    function fieldLabel(item) {
        if (!item) return ""
        if (item.field === "title") return qsTr("标题")
        if (item.field === "body") return qsTr("正文")
        return ""
    }

    Keys.onEscapePressed: function(event) {
        root.close()
        event.accepted = true
    }

    Rectangle {
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.38)
        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        id: panel
        anchors.centerIn: parent
        width: Math.min(760, Math.max(360, root.width - Kirigami.Units.gridUnit * 4))
        height: Math.min(620, Math.max(320, root.height - Kirigami.Units.gridUnit * 5))
        radius: Kirigami.Units.largeSpacing
        color: Kirigami.Theme.backgroundColor
        border.width: 1
        border.color: Kirigami.Theme.disabledTextColor
        clip: true

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Icon {
                    source: "edit-find"
                    implicitWidth: Kirigami.Units.iconSizes.medium
                    implicitHeight: implicitWidth
                    color: Kirigami.Theme.textColor
                }

                TextField {
                    id: queryField
                    Layout.fillWidth: true
                    placeholderText: qsTr("搜索作品、卷、章节、星图与设置…")
                    selectByMouse: true
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize + 1
                    onTextChanged: {
                        root.message = ""
                        root.pendingRequestId = ""
                        debounceTimer.stop()
                        if (text.trim().length === 0) {
                            root.clearResults()
                        } else {
                            debounceTimer.restart()
                        }
                    }
                    onAccepted: {
                        debounceTimer.stop()
                        root.executeSearch(false)
                    }
                    Keys.onEscapePressed: function(event) {
                        root.close()
                        event.accepted = true
                    }
                }

                BusyIndicator {
                    running: root.loading || root.loadingMore
                    visible: running
                    implicitWidth: Kirigami.Units.iconSizes.medium
                    implicitHeight: implicitWidth
                }

                ToolButton {
                    icon.name: "window-close"
                    text: qsTr("关闭")
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("关闭搜索（Esc）")
                    onClicked: root.close()
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 1
                color: Kirigami.Theme.disabledTextColor
                opacity: 0.35
            }

            Label {
                Layout.fillWidth: true
                visible: root.message.length > 0
                text: root.message
                color: Kirigami.Theme.negativeTextColor
                wrapMode: Text.WordWrap
            }

            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.minimumHeight: 120

                ListView {
                    id: resultList
                    anchors.fill: parent
                    visible: root.results.length > 0
                    clip: true
                    spacing: Kirigami.Units.smallSpacing
                    model: root.results

                    ScrollBar.vertical: ScrollBar {}

                    delegate: Rectangle {
                        id: resultRow
                        required property var modelData
                        width: resultList.width
                        height: resultContent.implicitHeight + Kirigami.Units.largeSpacing * 2
                        radius: Kirigami.Units.smallSpacing
                        color: rowMouse.containsMouse
                            ? Kirigami.Theme.highlightColor
                            : Kirigami.Theme.alternateBackgroundColor
                        opacity: rowMouse.containsMouse ? 1.0 : 0.92

                        MouseArea {
                            id: rowMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.resultActivated(resultRow.modelData)
                        }

                        ColumnLayout {
                            id: resultContent
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.margins: Kirigami.Units.largeSpacing
                            spacing: Kirigami.Units.smallSpacing

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Kirigami.Units.smallSpacing

                                Label {
                                    Layout.fillWidth: true
                                    text: resultRow.modelData.title || qsTr("未命名")
                                    font.weight: Font.DemiBold
                                    elide: Text.ElideRight
                                    color: rowMouse.containsMouse
                                        ? Kirigami.Theme.highlightedTextColor
                                        : Kirigami.Theme.textColor
                                }

                                Rectangle {
                                    visible: root.fieldLabel(resultRow.modelData).length > 0
                                    implicitWidth: fieldText.implicitWidth + Kirigami.Units.smallSpacing * 2
                                    implicitHeight: fieldText.implicitHeight + 2
                                    radius: height / 2
                                    color: rowMouse.containsMouse
                                        ? Kirigami.Theme.highlightedTextColor
                                        : Kirigami.Theme.highlightColor
                                    opacity: 0.22

                                    Label {
                                        id: fieldText
                                        anchors.centerIn: parent
                                        text: root.fieldLabel(resultRow.modelData)
                                        font.pointSize: Kirigami.Theme.defaultFont.pointSize - 1
                                        color: rowMouse.containsMouse
                                            ? Kirigami.Theme.highlightedTextColor
                                            : Kirigami.Theme.textColor
                                    }
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                text: resultRow.modelData.path || ""
                                font.pointSize: Kirigami.Theme.defaultFont.pointSize - 1
                                color: rowMouse.containsMouse
                                    ? Kirigami.Theme.highlightedTextColor
                                    : Kirigami.Theme.disabledTextColor
                                elide: Text.ElideMiddle
                            }

                            Label {
                                Layout.fillWidth: true
                                textFormat: Text.StyledText
                                text: root.summaryHtml(resultRow.modelData)
                                wrapMode: Text.Wrap
                                maximumLineCount: 3
                                elide: Text.ElideRight
                                color: rowMouse.containsMouse
                                    ? Kirigami.Theme.highlightedTextColor
                                    : Kirigami.Theme.textColor
                            }
                        }
                    }
                }

                ColumnLayout {
                    anchors.centerIn: parent
                    width: parent.width * 0.8
                    spacing: Kirigami.Units.smallSpacing
                    visible: root.results.length === 0 && !root.loading

                    Kirigami.Icon {
                        Layout.alignment: Qt.AlignHCenter
                        source: "edit-find"
                        implicitWidth: Kirigami.Units.iconSizes.huge
                        implicitHeight: implicitWidth
                        opacity: 0.35
                        color: Kirigami.Theme.textColor
                    }

                    Label {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        text: root.searched
                            ? qsTr("没有找到匹配内容")
                            : qsTr("输入关键词开始搜索")
                        color: Kirigami.Theme.disabledTextColor
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true

                Label {
                    Layout.fillWidth: true
                    text: root.results.length > 0
                        ? qsTr("已显示 %1 条结果").arg(root.results.length)
                        : ""
                    color: Kirigami.Theme.disabledTextColor
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize - 1
                }

                Button {
                    visible: root.nextCursor.length > 0
                    enabled: !root.loading && !root.loadingMore
                    text: root.loadingMore ? qsTr("正在加载…") : qsTr("加载更多")
                    onClicked: root.executeSearch(true)
                }
            }
        }
    }

    Connections {
        target: root.searchBackendRef
        function onResults_ready(request_id, page_json, append, error) {
            root.finishSearch(request_id, page_json, append, error)
        }
    }

    Timer {
        id: debounceTimer
        interval: 220
        repeat: false
        onTriggered: root.executeSearch(false)
    }
}
