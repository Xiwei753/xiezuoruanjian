// =============================================================================
// RightDrawer.qml — 工作台工具 pane（Core 的 ToolPane 角色）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：显示最右 WritingToolRail 选中的那一个工具的真实内容
// 约束：
//   - Issue #825：工具的选择/展开收起归最右竖向 rail，本组件只渲染内容；
//     顶部横向 tab 条已删除（工具入口只有 rail 一处，避免第二套导航）。
//   - 顶部工具条带 ToolbarTrailing 的同步 / 搜索 / 设置不搬到这里。
//   - 内容全部复用现有真实组件，不新造第二份工具状态。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt

    // Elevation shadow support
    property int elevation: 3
    property var appShadow: null

    readonly property color _sidebar: dt.sidebar
    readonly property color _border: dt.border
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _accentText: dt.accentText
    readonly property color _card: dt.card
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textSecondary: dt.textSecondary
    readonly property color _textMuted: dt.textMuted
    readonly property int _sp4: dt.sp4
    readonly property int _sp8: dt.sp8
    readonly property int _sp16: dt.sp16
    readonly property int _radiusSm: dt.radiusSm
    readonly property real _fontSm: dt.fontSmPt
    readonly property real _fontLg: dt.fontLgPt

    property var editorBackendRef: null
    property bool isOpen: false
    // Issue #829：当前工具 key（"" 表示 pane 收起）。
    // 手稿固定入口是 starmap / ai；conflict 只在运行时确有冲突时追加。
    // stats 旧内容仍保留给已有调用，不再作为宽屏 rail 的常驻入口。
    // 由 WritingWorkspace 单向注入，本组件不自己改。
    property string selectedTool: ""
    // Issue #757 评论 5818193510 第 5 点：冲突侧栏支持。
    // syncBackendRef + workspaceProjectId 透传给 SyncConflictPanel。
    // hasConflicts 控制 rail 上「冲突」入口显隐。
    property var syncBackendRef: null
    property string workspaceProjectId: ""
    property bool hasConflicts: false
    // Issue #770 评论 5842877986: 完整冲突快照，由 WritingWorkspace 透传，
    // 再下发给 SyncConflictPanel，不在 RightDrawer 内部调 list_sync_conflicts。
    property var syncConflicts: []
    // Issue #762 评论 5826175490 第 4 点：外部请求的冲突路径，透传给 SyncConflictPanel。
    // requestedConflictPath 是单向输入，SyncConflictPanel 绝不在内部赋值。
    property string requestedConflictPath: ""

    signal closeRequested()
    // 冲突入口被请求时发出（hasConflicts 从 false 变 true），外部据此选中冲突工具。
    signal conflictToolRequested()

    // Issue #825：工具 key → 标题 + 是否可用。
    // 可用性与入口都由 rail 表达，这里只用于标题文字，不再自行决定显隐。
    function toolTitle(toolKey) {
        if (toolKey === "starmap") return qsTr("星图")
        if (toolKey === "ai") return qsTr("AI")
        if (toolKey === "conflict") return qsTr("冲突")
        return qsTr("统计")
    }

    color: "transparent"
    clip: true
    visible: isOpen

    // Shadow layer (behind the drawer panel)
    Rectangle {
        anchors.fill: parent
        anchors.topMargin: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).verticalOffset : 0
        radius: 0
        color: root.elevation > 0 && root.appShadow ? root.appShadow.forElevation(root.elevation).color : "transparent"
        opacity: 0.2
        visible: root.elevation > 0 && root.appShadow !== null
        z: -1
    }

    Rectangle {
        id: drawerPanel
        anchors.fill: parent
        color: _sidebar
        border.color: _border
        border.width: 1

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // Issue #825：pane 头部只留「当前工具标题 + 关闭」。
            // 原来的横向 tab 条已删除，工具切换入口统一在最右 WritingToolRail。
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 44
                color: "transparent"

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: _sp8
                    anchors.rightMargin: _sp8
                    spacing: _sp4

                    // Issue #829：手稿在工具面板顶部画了一个 `∨` 折叠钮
                    // （docs/ui/reference/widescreen/宽屏全打开.png 右侧面板左上角）。
                    // 收起后工具内容去最右 WritingToolRail，rail 本身常驻。
                    PaneFoldButton {
                        dt: root.dt
                        onTriggered: root.closeRequested()
                    }

                    AppText {
                        dt: root.dt
                        text: root.toolTitle(root.selectedTool)
                        color: _textPrimary
                        font.pointSize: _fontLg
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }

                    // Close button
                    Rectangle {
                        width: 24; height: 24
                        radius: 12
                        color: closeHover.containsMouse ? _card : "transparent"

                        AppText {
                            dt: root.dt
                            anchors.centerIn: parent
                            text: "✕"
                            color: _textMuted
                            font.pointSize: _fontSm
                        }

                        MouseArea {
                            id: closeHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.closeRequested()
                        }
                    }
                }
            }

            // Divider
            Rectangle { Layout.fillWidth: true; height: 1; color: _border }

            // Content area
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                // Issue #829：手稿里的星图 / AI 内容槽。
                // 本轮只固定 ToolPane 的区域和切换键，不在 QML 里伪造业务数据；
                // 后续把真实星图 / AI 组件直接挂进这两个 Item 即可，不再改外层工作台骨架。
                Item {
                    visible: root.selectedTool === "starmap"
                    anchors.fill: parent
                }

                Item {
                    visible: root.selectedTool === "ai"
                    anchors.fill: parent
                }

                // 统计 — 复用现有 StatsPreviewPage
                StatsPreviewPage {
                    dt: root.dt
                    editorBackendRef: root.editorBackendRef
                    visible: root.selectedTool === "stats"
                    anchors.fill: parent
                }

                // Issue #757 评论 5818193510 第 5 点：冲突 — 复用现有 SyncConflictPanel。
                // SyncConflictPanel 负责调 SyncBackend 拿冲突列表/预览/解决动作，
                // 不在 QML 维护第二份可编辑正文，不自己拼磁盘路径读 conflicts.json。
                SyncConflictPanel {
                    visible: root.selectedTool === "conflict" && root.hasConflicts
                    anchors.fill: parent
                    dt: root.dt
                    syncBackendRef: root.syncBackendRef
                    projectId: root.workspaceProjectId
                    // Issue #770 评论 5842877986: 透传完整冲突快照和请求路径，
                    // 不再让 SyncConflictPanel 自己查一份。
                    conflicts: root.syncConflicts
                    requestedConflictPath: root.requestedConflictPath
                    onCloseRequested: root.closeRequested()
                    onConflictsResolved: {
                        // 解决一个冲突后刷新列表；若全部解决，外部应把 hasConflicts 置 false。
                        root.conflictToolRequested()
                    }
                }
            }
        }
    }

    // Issue #757 评论 5818193510 第 5 点：冲突刚产生时请求外部选中冲突工具。
    // 打开/切换由外部（WritingWorkspace）统一完成，RightDrawer 不自己控制 isOpen
    // 也不自己改 selectedTool（单向属性，由外部绑定）。
    onHasConflictsChanged: {
        if (root.hasConflicts) {
            root.conflictToolRequested()
        }
    }
}