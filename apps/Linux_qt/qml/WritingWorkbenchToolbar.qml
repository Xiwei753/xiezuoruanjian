// =============================================================================
// WritingWorkbenchToolbar.qml — 写作区工作台外壳（工具条带 + 内容区）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：把 Core 的七角色布局真正落到 Qt 树结构上
//   - 上层是一条贯通整窗的工具条带，按 Core 的 ToolbarLeading / ToolbarCenter /
//     ToolbarTrailing 三个 bounds 分组；
//   - 下层是内容区，装 ChapterNavigation | Editor | ToolPane | ToolRail。
// 约束（Issue #825）：
//   - 条带高度和三个分组的宽度都来自 Core 的 workbench plan（TopWritingWorkspace 注入
//     bounds）；Core 判 SinglePane 时不给工具条 bounds，此时分组按内容自适应，
//     保证窄窗口仍然排得下。
//   - 章节树 / 工具 pane / 工具 rail 只占内容区，不穿进工具条带。
//   - 返回 / 撤销 / 重做走真实路径（sujianEditor.undo()/redo()），不摆假按钮。
// =============================================================================

import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root

    required property var dt

    // ── Core 注入的工具条 bounds（dp）。-1 表示 Core 没给（SinglePane），按内容自适应。──
    property real toolbarHeight: -1
    property real leadingWidth: -1
    property real trailingWidth: -1

    // 章节标题（ToolbarLeading 里的作品名）
    property string projectTitle: ""

    // Center 组（WritingFormatGroup）的入参
    property real currentFontSize: 16
    property real currentLineSpacing: 1.5
    property bool firstLineIndent: false
    property string saveStatus: ""

    // Trailing 组（GlobalTopActions）的入参
    property var appState: ({})

    signal backRequested()
    signal undoRequested()
    signal redoRequested()
    signal fontSizeChanged(real size)
    signal lineSpacingChanged(real spacing)
    signal firstLineIndentToggled()
    signal formatOneClick()
    signal requestSync()
    signal requestSearch()
    signal openSettings()

    // Issue #825：内容区插槽。WritingWorkspace 把 SplitView（四角色）放进来。
    default property alias contentData: contentSlot.data

    spacing: 0

    // ── 工具条带 ──
    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: root.toolbarHeight > 0 ? root.toolbarHeight : 48
        color: root.dt.surface

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: root.dt.sp16
            anchors.rightMargin: root.dt.sp16
            spacing: root.dt.sp8

            // ── ToolbarLeading：返回 + 作品名 + 撤销/重做 ──
            RowLayout {
                spacing: root.dt.sp4
                Layout.fillWidth: false
                // Core 给了 ToolbarLeading bounds 就按它定量（含 min/max，
                // 保证不会因为内容长度挤走 Center 组）。
                Layout.preferredWidth: root.leadingWidth > 0 ? root.leadingWidth : -1
                Layout.minimumWidth: root.leadingWidth > 0 ? root.leadingWidth : 0
                Layout.maximumWidth: root.leadingWidth > 0 ? root.leadingWidth : Number.POSITIVE_INFINITY

                // 返回作品列表
                ToolbarIconButton {
                    dt: root.dt
                    glyph: "\u2190"
                    onTriggered: root.backRequested()
                }

                AppText {
                    dt: root.dt
                    text: root.projectTitle || qsTr("作品")
                    color: root.dt.textPrimary
                    font.pointSize: root.dt.fontMdPt
                    font.family: root.dt.fontFamily
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                    Layout.maximumWidth: 160
                }

                // 撤销 / 重做：走 SujianEditorItem 真实实现的 undo()/redo()，
                // 没有可撤销的事务时 Rust 侧直接 no-op，所以按钮不必做可用态判断
                // （端侧不再自己维护一份"能不能撤"的状态）。
                ToolbarIconButton {
                    dt: root.dt
                    glyph: "\u21B6"
                    onTriggered: root.undoRequested()
                }

                ToolbarIconButton {
                    dt: root.dt
                    glyph: "\u21B7"
                    onTriggered: root.redoRequested()
                }
            }

            // ── ToolbarCenter：字号 / 行距 / 段落 / 一键排版 + 保存状态 ──
            WritingFormatGroup {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                dt: root.dt
                currentFontSize: root.currentFontSize
                currentLineSpacing: root.currentLineSpacing
                firstLineIndent: root.firstLineIndent
                saveStatus: root.saveStatus
                onFontSizeChanged: function(size) { root.fontSizeChanged(size) }
                onLineSpacingChanged: function(spacing) { root.lineSpacingChanged(spacing) }
                onFirstLineIndentToggled: root.firstLineIndentToggled()
                onFormatOneClick: root.formatOneClick()
            }

            // ── ToolbarTrailing：同步 / 搜索 / 设置 ──
            GlobalTopActions {
                Layout.fillWidth: false
                Layout.preferredWidth: root.trailingWidth > 0 ? root.trailingWidth : -1
                Layout.minimumWidth: root.trailingWidth > 0 ? root.trailingWidth : 0
                Layout.maximumWidth: root.trailingWidth > 0 ? root.trailingWidth : Number.POSITIVE_INFINITY
                dt: root.dt
                appState: root.appState
                onRequestSync: root.requestSync()
                onRequestSearch: root.requestSearch()
                onOpenSettings: root.openSettings()
            }
        }
    }

    // ── 内容区：ChapterNavigation | Editor | ToolPane | ToolRail ──
    Item {
        id: contentSlot
        Layout.fillWidth: true
        Layout.fillHeight: true
    }
}