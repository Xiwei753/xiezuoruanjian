// =============================================================================
// RightDrawer.qml — 工作台工具 pane（Core 的 ToolPane 角色 / Issue #900 KDE）
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：显示最右 WritingToolRail 选中的那一个工具的真实内容
// 约束：
//   - Issue #825：工具的选择/展开收起归最右竖向 rail，本组件只渲染内容；
//     顶部横向 tab 条已删除（工具入口只有 rail 一处，避免第二套导航）。
//   - 顶部工具条带 ToolbarTrailing 的同步 / 搜索 / 设置不搬到这里。
//   - 内容全部复用现有真实组件，不新造第二份工具状态。
//
// Issue #900：外观迁移到 Breeze + Kirigami 主题色，关闭按钮换成 Breeze
//   window-close 图标；elevation / appShadow 属性签名保留（不再自绘阴影）。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt

    // 旧的自绘阴影入参，Issue #900 后不再使用，仅保留属性签名。
    property int elevation: 3
    property var appShadow: null

    property var editorBackendRef: null
    // Issue #835：星图预览所需后端与全局状态，由 WritingWorkspace 透传，
    // 再下发给 StarMapPreviewPage，不在 RightDrawer 内部自造星图数据。
    property var starmapBackendRef: null
    // Issue #835 评论 6019713847: StarMapPage 通过 StarMapController 读真实根星图，
    // 不用写死卡片的 StarMapPreviewPage 占位。starMapControllerRef 由
    // WritingWorkspace 透传（main.qml 注入 globalStarMapController）。
    property var starMapControllerRef: null
    property var appState: ({})
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

    // Issue #835 评论 6020221770: 右工具面板仍属于写作工作台，不切 route。
    // 点根星图后内嵌真实 StarMapWorkspace，返回时退回右栏里的 StarMapPage 列表。
    // embeddedStarmapId 非空 = 正在看某个根星图的 Canvas；空 = 看根星图列表。
    property string embeddedStarmapId: ""
    property string embeddedStarmapTitle: ""

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

    Rectangle {
        id: drawerPanel
        anchors.fill: parent
        color: Kirigami.Theme.alternateBackgroundColor

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // Issue #825：pane 头部只留「当前工具标题 + 关闭」。
            // 原来的横向 tab 条已删除，工具切换入口统一在最右 WritingToolRail。
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 2.4

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.smallSpacing
                    anchors.rightMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing

                    // Issue #829：手稿在工具面板顶部画了一个折叠钮
                    // （docs/ui/reference/widescreen/宽屏全打开.png 右侧面板左上角）。
                    // 收起后工具内容去最右 WritingToolRail，rail 本身常驻。
                    PaneFoldButton {
                        dt: root.dt
                        Layout.alignment: Qt.AlignVCenter
                        onTriggered: root.closeRequested()
                    }

                    Label {
                        text: root.toolTitle(root.selectedTool)
                        color: Kirigami.Theme.textColor
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }

                    // Close button
                    ToolButton {
                        icon.name: "window-close"
                        icon.width: Kirigami.Units.iconSizes.small
                        icon.height: Kirigami.Units.iconSizes.small
                        display: AbstractButton.IconOnly
                        flat: true
                        Layout.alignment: Qt.AlignVCenter
                        onClicked: root.closeRequested()
                    }
                }
            }

            // Divider
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Kirigami.Theme.disabledTextColor
                opacity: 0.3
            }

            // Content area
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                // Issue #835 评论 6020221770: 星图工具区分两态，都在右栏内渲染，
                // 不切 route、不销毁 WritingWorkspace。
                // 未选根星图：StarMapPage 列表，通过 StarMapController.listStarmaps()
                // 读真实根星图。点某个根星图后切到嵌入态。
                StarMapPage {
                    visible: root.selectedTool === "starmap" && root.embeddedStarmapId === ""
                    anchors.fill: parent
                    dt: root.dt
                    starMapController: root.starMapControllerRef
                    appState: root.appState

                    onOpenStarmap: function(starmapId, title) {
                        root.embeddedStarmapId = starmapId
                        root.embeddedStarmapTitle = title
                    }
                }

                // 选中根星图后：内嵌真实 StarMapWorkspace（含 StarMapCanvas），
                // starmapBackendRef 给 Workspace/Canvas，子星图沿现有焦点链工作。
                // 返回时清空 embeddedStarmapId 退回根星图列表，route 始终是 writing。
                StarMapWorkspace {
                    visible: root.selectedTool === "starmap" && root.embeddedStarmapId !== ""
                    anchors.fill: parent
                    dt: root.dt
                    starmapBackendRef: root.starmapBackendRef
                    starmapId: root.embeddedStarmapId
                    starmapTitle: root.embeddedStarmapTitle
                    appState: root.appState

                    onBackClicked: {
                        root.embeddedStarmapId = ""
                        root.embeddedStarmapTitle = ""
                    }
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
