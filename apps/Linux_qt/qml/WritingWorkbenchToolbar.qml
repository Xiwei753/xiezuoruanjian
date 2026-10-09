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
//   - 条带高度和三个分组的宽度都来自 Core 的 workbench plan（WritingWorkspace 注入
//     bounds）；Core 判 SinglePane 时不给工具条 bounds，此时分组按内容自适应，
//     保证窄窗口仍然排得下。
//   - Issue #825 复核5：三个分组容器严格占 Core 的 bounds，段间 spacing=0，
//     条带外层不再加 margin/padding；视觉内边距只加在各组内部。
//   - Issue #825 复核6第3点：Qt Quick Layouts 的 Layout.*Margin 是 item **外部**
//     margin，会放大该 item 在布局里占的有效 cell 宽度——所以角色 wrapper 上
//     一个 Layout margin 都不能有。三个角色 wrapper 的 min/preferred/max 宽度
//     严格等于 Core 的 bounds，视觉留白一律放在 wrapper **内部**子项的 anchors 上。
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
    property real centerWidth: -1
    property real trailingWidth: -1

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
            // Issue #825 复核5：段间不插间距，三段容器严丝合缝占满 Core 的
            // ToolbarLeading / ToolbarCenter / ToolbarTrailing bounds。
            spacing: 0

            // ── ToolbarLeading 角色 wrapper：宽度严格等于 Core 的 ToolbarLeading ──
            Item {
                id: leadingSlot
                // Core 没给 bounds（SinglePane）时按内容自适应，不让内部内容被压扁。
                implicitWidth: leadingInner.implicitWidth + root.dt.sp12 + root.dt.sp8
                Layout.fillWidth: false
                Layout.fillHeight: true
                Layout.preferredWidth: root.leadingWidth > 0 ? root.leadingWidth : implicitWidth
                Layout.minimumWidth: root.leadingWidth > 0 ? root.leadingWidth : implicitWidth
                Layout.maximumWidth: root.leadingWidth > 0 ? root.leadingWidth : implicitWidth

                RowLayout {
                    id: leadingInner
                    // 视觉内边距放在 wrapper 内部，不改角色几何。
                    anchors.fill: parent
                    anchors.leftMargin: root.dt.sp12
                    anchors.rightMargin: root.dt.sp8
                    spacing: root.dt.sp4

                    // Issue #825 复核5第4点：标题不进 ToolbarLeading。
                    // Leading 只放返回 / 撤销 / 重做，作品名由章节树自己承担。
                    ToolbarIconButton {
                        dt: root.dt
                        glyph: "\u2190"
                        onTriggered: root.backRequested()
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
            }

            // ── ToolbarCenter 角色 wrapper：宽度严格等于 Core 的 ToolbarCenter ──
            Item {
                Layout.fillWidth: root.centerWidth <= 0
                Layout.fillHeight: true
                Layout.preferredWidth: root.centerWidth > 0 ? root.centerWidth : -1
                Layout.minimumWidth: root.centerWidth > 0 ? root.centerWidth : 0
                Layout.maximumWidth: root.centerWidth > 0 ? root.centerWidth : Number.POSITIVE_INFINITY

                WritingFormatGroup {
                    anchors.fill: parent
                    anchors.leftMargin: root.dt.sp8
                    anchors.rightMargin: root.dt.sp8
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
            }

            // ── ToolbarTrailing 角色 wrapper：宽度严格等于 Core 的 ToolbarTrailing ──
            Item {
                id: trailingSlot
                implicitWidth: trailingInner.implicitWidth + root.dt.sp8 + root.dt.sp16
                Layout.fillWidth: false
                Layout.fillHeight: true
                Layout.preferredWidth: root.trailingWidth > 0 ? root.trailingWidth : implicitWidth
                Layout.minimumWidth: root.trailingWidth > 0 ? root.trailingWidth : implicitWidth
                Layout.maximumWidth: root.trailingWidth > 0 ? root.trailingWidth : implicitWidth

                GlobalTopActions {
                    id: trailingInner
                    // ToolbarTrailing 内容贴 Core bounds 右缘，内边距只在内部。
                    anchors.right: parent.right
                    anchors.rightMargin: root.dt.sp16
                    anchors.verticalCenter: parent.verticalCenter
                    dt: root.dt
                    appState: root.appState
                    onRequestSync: root.requestSync()
                    onRequestSearch: root.requestSearch()
                    onOpenSettings: root.openSettings()
                }
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