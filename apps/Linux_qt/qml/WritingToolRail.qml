// =============================================================================
// WritingToolRail.qml — 写作区最右竖向工具栏（Core 的 ToolRail 角色 / Issue #900 KDE）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：只做两件事 —— 选哪个工具、展开/收起右边的工具 pane
// 约束（Issue #825）：
//   - 工具内容在 WritingWorkspace 里的 RightDrawer（ToolPane），rail 不持有内容状态；
//   - 顶部工具条带的 ToolbarTrailing 同步 / 搜索 / 设置不搬到这里；
//   - Issue #829：手稿把最右 rail 固定为“星图 / AI”；冲突只在确有冲突时追加；
//   - 只发信号，由 WritingWorkspace 单向写 selectedTool，避免双向写 binding。
//
// Issue #900：自绘 Rectangle 工具项换成 Breeze Controls.ItemDelegate
//   （自带 hover / 选中态），展开收起按钮用 Breeze go-previous / go-next 图标，
//   颜色 / 间距走 Kirigami.Theme / Kirigami.Units。tools 模型与所有信号保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt

    // Core 给 ToolRail 的宽度（由 WritingWorkspace 从 workbench plan 注入）。
    property real railWidth: 56
    property bool hasConflicts: false
    // 当前选中的工具 key（"" = 工具 pane 收起）。单向输入，由 WritingWorkspace 写。
    property string selectedTool: ""

    // Issue #842 任务3：toolPaneOpen 改为独立输入属性，不再从 selectedTool 派生。
    // 由 WritingWorkspace 绑定到 rightPaneCollapsed 的反值，与工具选择解耦。
    property bool toolPaneOpen: false

    // Issue #829：右侧工具 rail 的空间骨架按手稿固定。
    // 星图始终占这个入口；真实内容在 RightDrawer 里接，不再改 rail 结构。
    // 冲突属于运行时异常入口，只在确实存在冲突时追加到下面。
    // Issue #835 评论 6020221770: AI 暂无 Linux_Qt 真实会话面板，rail 位置保留
    // 但 enabled:false 禁用，降 opacity，不发 toolRequested，不伪造 AI 页面。
    // 以后真实 AI 面板接进来把 enabled 改 true 即可。
    readonly property var tools: {
        var list = [
            { key: "starmap", label: qsTr("星图"), icon: "view-grid", enabled: true },
            { key: "ai", label: qsTr("AI"), enabled: false }
        ]
        if (root.hasConflicts) {
            list.push({ key: "conflict", label: qsTr("冲突"), icon: "dialog-warning" })
        }
        return list
    }

    signal toolRequested(string toolKey)
    signal toolPaneToggled()

    color: Kirigami.Theme.alternateBackgroundColor

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: Kirigami.Units.smallSpacing
        anchors.bottomMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        // ── 工具入口 ──
        Repeater {
            model: root.tools

            ItemDelegate {
                id: toolDelegate
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: Math.round(root.railWidth - Kirigami.Units.smallSpacing)
                Layout.preferredHeight: 44
                text: modelData.label
                highlighted: root.selectedTool === modelData.key
                enabled: modelData.enabled !== false
                // Issue #835 评论 6020221770: disabled 入口（如 AI）降 opacity，
                // 保留 rail 位置但不诱导点击。
                opacity: modelData.enabled !== false ? 1.0 : 0.4

                ToolTip.visible: hovered
                ToolTip.text: modelData.label

                icon.name: modelData.icon || "tools-wizard"
                icon.width: Kirigami.Units.iconSizes.small
                icon.height: Kirigami.Units.iconSizes.small
                display: AbstractButton.TextUnderIcon

                onClicked: root.toolRequested(modelData.key)
            }
        }

        Item { Layout.fillHeight: true }

        // ── 工具 pane 展开 / 收起 ──
        ToolButton {
            id: toggleButton
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: Math.round(root.railWidth - Kirigami.Units.smallSpacing)
            Layout.preferredHeight: Kirigami.Units.gridUnit * 2.4
            flat: true
            // 展开时按钮指向右（收起），收起时指向左（展开）。
            icon.name: root.toolPaneOpen ? "go-next" : "go-previous"
            icon.width: Kirigami.Units.iconSizes.small
            icon.height: Kirigami.Units.iconSizes.small
            display: AbstractButton.IconOnly

            ToolTip.visible: hovered
            ToolTip.text: root.toolPaneOpen ? qsTr("收起工具面板") : qsTr("展开工具面板")

            onClicked: root.toolPaneToggled()
        }
    }
}
