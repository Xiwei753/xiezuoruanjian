// =============================================================================
// WritingToolRail.qml — 写作区最右竖向工具栏（Core 的 ToolRail 角色）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只做两件事 —— 选哪个工具、展开/收起右边的工具 pane
// 约束（Issue #825）：
//   - 工具内容在 WritingWorkspace 里的 RightDrawer（ToolPane），rail 不持有内容状态；
//   - 顶部工具条带的 ToolbarTrailing 同步 / 搜索 / 设置不搬到这里；
//   - Issue #829：手稿把最右 rail 固定为“星图 / AI”；冲突只在确有冲突时追加；
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

    // Issue #829：右侧工具 rail 的空间骨架按手稿固定。
    // 星图始终占这个入口；真实内容在 RightDrawer 里接，不再改 rail 结构。
    // 冲突属于运行时异常入口，只在确实存在冲突时追加到下面。
    // Issue #835 评论 6020221770: AI 暂无 Linux_Qt 真实会话面板，rail 位置保留
    // 但 enabled:false 禁用，降 opacity，不发 toolRequested，不伪造 AI 页面。
    // 以后真实 AI 面板接进来把 enabled 改 true 即可。
    readonly property var tools: {
        var list = [
            { key: "starmap", label: qsTr("星图"), enabled: true },
            { key: "ai", label: qsTr("AI"), enabled: false }
        ]
        if (root.hasConflicts) {
            list.push({ key: "conflict", label: qsTr("冲突") })
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
                    text: modelData.label
                    color: root.selectedTool === modelData.key ? dt.accentText : dt.textPrimary
                    font.pointSize: modelData.key === "ai" ? dt.fontSmPt : dt.captionPt
                    font.weight: root.selectedTool === modelData.key ? Font.DemiBold : Font.Normal
                    // Issue #835 评论 6020221770: disabled 入口（如 AI）降 opacity，
                    // 保留 rail 位置但不诱导点击。
                    opacity: modelData.enabled !== false ? 1.0 : 0.4
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
                    // Issue #835 评论 6020221770: disabled 入口不响应点击，
                    // 不发 toolRequested，RightDrawer 不需要造 AI 占位内容。
                    enabled: modelData.enabled !== false
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