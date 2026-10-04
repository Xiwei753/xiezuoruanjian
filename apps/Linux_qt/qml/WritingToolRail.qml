// =============================================================================
// WritingToolRail.qml — 写作区最右竖向工具栏（Core 的 ToolRail 角色）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只做两件事 —— 选哪个工具、展开/收起右边的工具 pane
// 约束（Issue #825）：
//   - 工具内容在 WritingWorkspace 里的 RightDrawer（ToolPane），rail 不持有内容状态；
//   - 顶部工具条带的 ToolbarTrailing 同步 / 搜索 / 设置不搬到这里；
//   - 工具列表只列真实有内容的工具（统计常驻、冲突需要 hasConflicts），
//     不摆"正在施工"的占位按钮；
//   - 只发信号，由 WritingWorkspace 单向写 selectedTool，避免双向写 binding。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt

    readonly property color _sidebar: dt.sidebar
    readonly property color _border: dt.border
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _accentText: dt.accentText
    readonly property color _textMuted: dt.textMuted
    readonly property int _sp4: dt.sp4
    readonly property int _sp8: dt.sp8
    readonly property int _sp12: dt.sp12
    readonly property int _radiusSm: dt.radiusSm
    readonly property int _radiusPill: dt.radiusPill
    readonly property real _fontSm: dt.fontSmPt

    // Core 给 ToolRail 的宽度（由 WritingWorkspace 从 workbench plan 注入）。
    property real railWidth: 56
    property bool hasConflicts: false
    // 当前选中的工具 key（"" = 工具 pane 收起）。单向输入，由 WritingWorkspace 写。
    property string selectedTool: ""

    readonly property bool toolPaneOpen: root.selectedTool !== ""

    // 工具入口：key 与 RightDrawer 的 selectedTool 取值一致。
    // 星图 / 统计 常驻；AI 需要能力开关；冲突需要确实有未解决冲突。
    // Issue #825 复核4 附注：rail 只列真实有内容的工具。
    // 星图 / AI 在 ToolPane 里仍是"正在施工"占位，就不在宽屏新壳里当正式入口摆出来；
    // 等它们接到真实内容时再加回这个列表。
    readonly property var tools: {
        var list = [
            { key: "stats", label: qsTr("统计"), glyph: "📊" }
        ]
        if (root.hasConflicts) {
            list.push({ key: "conflict", label: qsTr("冲突"), glyph: "⚠" })
        }
        return list
    }

    signal toolRequested(string toolKey)
    signal toolPaneToggled()

    color: _sidebar
    border.color: _border
    border.width: 1

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: _sp8
        anchors.bottomMargin: _sp8
        spacing: _sp4

        // ── 工具入口 ──
        Repeater {
            model: root.tools

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: Math.round(root.railWidth - _sp12)
                Layout.preferredHeight: 44
                radius: _radiusSm
                color: root.selectedTool === modelData.key
                       ? _accentSoft
                       : toolHover.containsMouse ? dt.surfaceVariant : "transparent"
                border.color: root.selectedTool === modelData.key ? dt.border : "transparent"
                border.width: 1

                Behavior on color { ColorAnimation { duration: dt.animFast } }

                AppText {
                    dt: root.dt
                    anchors.centerIn: parent
                    text: modelData.glyph
                    font.pointSize: dt.fontEmojiSmPt
                }

                // 选中态左侧竖条：和 CreativeHub 左栏选中态同一套 token。
                Rectangle {
                    visible: root.selectedTool === modelData.key
                    anchors.left: parent.left
                    anchors.leftMargin: _sp4
                    anchors.verticalCenter: parent.verticalCenter
                    width: 3
                    height: 20
                    radius: _radiusPill
                    color: dt.primary
                }

                MouseArea {
                    id: toolHover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.toolRequested(modelData.key)
                }

                ToolTip {
                    visible: toolHover.containsMouse
                    text: modelData.label
                    delay: 400
                }
            }
        }

        Item { Layout.fillHeight: true }

        // ── 工具 pane 展开 / 收起 ──
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: Math.round(root.railWidth - _sp12)
            Layout.preferredHeight: 44
            radius: _radiusPill
            color: toggleHover.containsMouse ? dt.surfaceVariant : "transparent"
            border.color: _border
            border.width: 1

            Behavior on color { ColorAnimation { duration: dt.animFast } }

            AppText {
                dt: root.dt
                anchors.centerIn: parent
                // 展开时按钮指向右（收起），收起时指向左（展开）。
                text: root.toolPaneOpen ? "›" : "‹"
                color: _textMuted
                font.pointSize: dt.fontMdPt
            }

            MouseArea {
                id: toggleHover
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.toolPaneToggled()
            }
        }
    }
}