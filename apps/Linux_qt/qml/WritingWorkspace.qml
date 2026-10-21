// =============================================================================
// WritingWorkspace.qml - 写作工作区
// =============================================================================
//
// 职责：Linux_qt UI 层（QML 组件）
// 边界：只负责布局和导航（编辑区 + 侧栏 + 工具栏）
// 约束：
//   - 所有业务逻辑委托 EditorController
//   - 通过 WritingTreeController 管理章节树
//   - 不包含保存、格式化等业务操作
//
// 关于布局（Issue #920 / #971）
//
// Linux Qt 始终使用桌面 SplitView + 工具轨骨架；可用宽度不足时只隐藏次要侧栏。
// Core 不接收窗口尺寸，也不返回布局模式 / 栏宽 / bounds / 角色矩形。
//
// 本地布局不直接控制以下内容：
//   - 自研 SujianEditorItem 的 QSG 渲染
//   - 光标和选区的 IME 交互逻辑
//   - QTextLayout 的排版细节
//   - Rust Coordinator → Scene Graph 动画渲染参数
//
// 编辑器交互（包括IME处理）由 EditorController 和 SujianEditorItem
// 直接管理，不走 Qt QSG 渲染管线，不受本地布局断点约束
//
// 组成（Issue #977 起）：
//   全窗口 DesktopTopBar 由 main.qml 统一挂载，不在本页绘制。
//   本页只负责顶栏以下内容区 RowLayout：
//     ChapterNavigation (章节树) | Editor (SujianEditorItem) |
//     ToolPane (工具内容) | ToolRail (最右竖向工具栏)
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Sujian 1.0
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property var editorBackendRef: null
    property var projectBackendRef: null
    // Issue #709 评论 issue-body-709: 传入 themeController 给 EditorController，
    // 使 logRenderColorProbe 能读取 ThemeController runtime state。
    property var themeController: null
    // Issue #933: 显式注入的设置 / 工作区后端引用，替代页面里裸查全局
    // context property（settingsBackend / workspaceBackend）。由 main.qml 传入。
    property var settingsBackendRef: null
    property var workspaceBackendRef: null
    property var appState: ({})
    property string preSyncFlushFailureReason: ""
    // Issue #835：星图后端透传给 RightDrawer（任务 4 已为 RightDrawer 加 starmapBackendRef 属性）。
    // 由 main.qml 注入，WritingWorkspace 只做透传，不在本地伪造星图业务状态。
    property var starmapBackendRef: null
    // Issue #835 评论 6019713847: StarMapPage 需要真实 StarMapController 读根星图，
    // 由 main.qml 注入 globalStarMapController，透传给 RightDrawer.starMapControllerRef。
    property var starMapControllerRef: null
    property var tree: []
    // 左右 pane 的展开意图只是端侧 UI 状态（不进 Core、不进同步）。
    // 工具 pane 用「选中的工具 key」表达："" = 收起，非空 = 展开并显示该工具。
    // Issue #829 / #950：手稿里的常驻入口是 starmap / ai / correction；冲突只作为动态附加工具。
    // 不再保留 drawerOpen + drawerTab 两份状态，避免第二套开关。
    // Issue #842 任务3：drawerTool 只表示当前工具选择，不再同时表达折叠状态。
    // rightPaneCollapsed 独立保存右栏展开意图；drawerOpen 表示当前确实可见。
    property string drawerTool: "starmap"
    property bool rightPaneCollapsed: true
    property bool leftPaneCollapsed: false
    readonly property bool chapterNavigationShown:
        !root.leftPaneCollapsed && root.workbenchFitsFor(true, false)
    readonly property bool drawerOpen:
        !root.rightPaneCollapsed && root.workbenchFitsFor(root.chapterNavigationShown, true)

    // ── Issue #920 / #971：桌面 SplitView 尺寸与次要栏的有效可见状态 ──
    readonly property int listPaneMinWidth: 200
    readonly property int toolPaneMinWidth: 200
    readonly property int toolRailWidth: 56
    readonly property int editorMinWidth: 240
    readonly property int toolbarMinHeight: 64
    readonly property int toolbarLeadingMinWidth: 200
    readonly property int toolbarTrailingMinWidth: 200
    // Issue #922：SplitView 原生分隔线宽度（vp）。拖动条由 Qt 接管，只看得到这条线。
    readonly property int splitHandleWidth: 6

    // 请求展开的面板是否能与正文、最右工具轨共存；空间不足时只让面板让位。
    // 每个可见 pane 的预算都包含其原生 SplitView 分隔线。
    function workbenchFitsFor(chapterVisible, toolVisible) {
        var needed = (chapterVisible ? root.listPaneMinWidth + root.splitHandleWidth : 0)
                   + (toolVisible ? root.toolPaneMinWidth + root.splitHandleWidth : 0)
                   + root.toolRailWidth
                   + root.editorMinWidth
        return root.width >= needed
    }

    // 左右栏保存用户展开意图；窄窗只改变有效可见状态，空间恢复后自动还原。
    // 空间不足时始终优先保留正文 + 56vp 工具轨，再让右工具栏、左章节栏依次让位。
    readonly property bool toolRailVisible: true

    // 工具条带三个分组的尺寸；leading/trailing 跟随桌面 SplitView 的有效栏宽。
    // SplitView 拖动时会自己更新 sidebarRect/rightDrawerRect 的实际宽度，
    // 这里直接读当前实际显示宽度，与 SplitView 原生布局保持同一尺度。
    readonly property real toolbarHeight: root.toolbarMinHeight
    readonly property real toolbarCenterWidth: -1
    readonly property real toolbarLeadingWidth:
        Math.max(root.toolbarLeadingMinWidth, root.chapterNavigationShown ? sidebarRect.width : 0)
    readonly property real toolbarTrailingWidth:
        Math.max(root.toolbarTrailingMinWidth,
                 root.toolRailWidth + (root.drawerOpen ? rightDrawerRect.width : 0))

    // Issue #922：SplitView 拖动与折叠/收起状态之间只互补、不互相写回。
    // sidebarRect / rightDrawerRect 的 SplitView.preferredWidth 是唯一宽度状态源，
    // 根组件不再维护一套 chapterNavWidth / toolPaneWidth 去反向覆盖原生拖拽。

    // 启动恢复用户上次拖拽保存的栏宽（只做下界钳制，不因当前窗口暂时变窄而
    // 永久改写保存的期望值；实际显示宽度由 SplitView 自己约束）。
    function restorePersistedPaneWidths() {
        if (!root.settingsBackendRef)
            return
        var storedSidebar = root.settingsBackendRef.setting_desktop_sidebar_width
        if (storedSidebar !== undefined && storedSidebar > 0) {
            sidebarRect.SplitView.preferredWidth = Math.max(root.listPaneMinWidth, storedSidebar)
        }
        var storedTool = root.settingsBackendRef.setting_desktop_tool_pane_width
        if (storedTool !== undefined && storedTool > 0) {
            rightDrawerRect.SplitView.preferredWidth = Math.max(root.toolPaneMinWidth, storedTool)
        }
    }

    // 拖拽结束后把当前可见栏的期望宽度写回平台本地配置（不回写 Core 业务设置）。
    // 只在拖动结束（!resizing）调用一次，不在每一帧写 JSON；只写当前可见栏，
    // 收起栏的原宽度仍由 SplitView.preferredWidth 保留。
    function persistPaneWidths() {
        if (!root.settingsBackendRef)
            return
        if (sidebarRect.visible && sidebarRect.SplitView.preferredWidth > 0) {
            root.settingsBackendRef.setting_desktop_sidebar_width =
                Math.max(root.listPaneMinWidth, sidebarRect.SplitView.preferredWidth)
        }
        if (rightDrawerRect.visible && rightDrawerRect.SplitView.preferredWidth > 0) {
            root.settingsBackendRef.setting_desktop_tool_pane_width =
                Math.max(root.toolPaneMinWidth, rightDrawerRect.SplitView.preferredWidth)
        }
        root.settingsBackendRef.debounced_save_local_settings()
    }

    // 展开侧 pane 是一个「请求」，不是直接写状态：一个属性都不改就把候选组合
    // 先按本地最小宽度算一遍，放得下才整体提交；放不下就什么都不改，
    // 不留「已展开但看不见」的端侧状态，也没有中间态闪烁。
    function requestToolPaneOpen(toolKey, allowCollapseLeft) {
        var targetLeftCollapsed = root.leftPaneCollapsed;
        var fits = root.workbenchFitsFor(!root.leftPaneCollapsed, true);
        if (!fits && allowCollapseLeft === true) {
            fits = root.workbenchFitsFor(false, true);
            targetLeftCollapsed = true;
        }
        if (!fits) return;
        root.leftPaneCollapsed = targetLeftCollapsed;
        root.drawerTool = toolKey || "starmap";
        root.rightPaneCollapsed = false;
    }

    // 左目录栏的展开请求，与右侧对称：必要时让右 pane 让位，仍放不下就不改任何状态。
    function requestChapterNavigationOpen() {
        var targetRightPaneCollapsed = root.rightPaneCollapsed;
        var fits = root.workbenchFitsFor(true, !root.rightPaneCollapsed);
        if (!fits) {
            fits = root.workbenchFitsFor(true, false);
            targetRightPaneCollapsed = true;
        }
        if (!fits) return;
        root.rightPaneCollapsed = targetRightPaneCollapsed;
        root.leftPaneCollapsed = false;
    }

    // 关闭方向不会把窗口撑小，永远放得下，直接改状态即可。
    function closeToolPane() {
        root.rightPaneCollapsed = true;
    }

    function closeChapterNavigation() {
        root.leftPaneCollapsed = true;
    }

    // 展开/收起工具 pane 的统一入口（rail 的展开收起按钮 + 右侧折叠把手）。
    // drawerTool 独立保存当前工具，不会重新硬切星图；空值兜底 "starmap"。
    function toggleToolPane() {
        if (root.drawerOpen) {
            root.closeToolPane();
        } else {
            root.requestToolPaneOpen(root.drawerTool || "starmap", true);
        }
    }

    // ── Issue #977：供 main.qml 的统一 DesktopTopBar 调用的公共动作 ──
    // 顶栏上移后不能再直接访问编辑器内部 id / 设置 backend；由本工作区把
    // 撤销、重做、排版和格式设置收口成公开方法，壳层只负责转发点击。
    readonly property string saveStatus: editorController.saveStatus

    function undoActiveEditor() {
        editorViewport.undo()
    }

    function redoActiveEditor() {
        editorViewport.redo()
    }

    function formatActiveEditor() {
        editorController.formatText()
    }

    function setWritingFontSize(size) {
        if (!root.settingsBackendRef) return
        root.settingsBackendRef.setting_font_size = size
        root.settingsBackendRef.debounced_save_local_settings()
    }

    function setWritingLineSpacing(spacing) {
        if (!root.settingsBackendRef) return
        root.settingsBackendRef.setting_line_spacing = spacing
        root.settingsBackendRef.debounced_save_local_settings()
    }

    function toggleWritingFirstLineIndent() {
        if (!root.settingsBackendRef) return
        root.settingsBackendRef.setting_auto_indent_enabled =
            !root.settingsBackendRef.setting_auto_indent_enabled
        root.settingsBackendRef.debounced_save_local_settings()
    }

    // Issue #828：根对象上只能有一个 Component.onCompleted。
    // 底部启动初始化块是唯一入口，这里不再有并列 handler。

    // Project-level ID - set by main.qml, used for tree and create volume/chapter
    property string workspaceProjectId: ""
    // Issue #829：左树顶部分组头要显示作品名。标题从注入的 tree 里找当前作品那一条，
    // 端侧不额外查一次后端——tree 本来就是当前作品的树。
    readonly property string workspaceProjectTitle: {
        var all = root.tree || []
        for (var i = 0; i < all.length; i++) {
            if (all[i] && all[i].type === "project" && all[i].id === root.workspaceProjectId) {
                return all[i].title || ""
            }
        }
        return ""
    }
    // Issue #836：左树顶部作品标题改为静态行，不再有 projectGroupCollapsed /
    // outlineGroupExpanded 两个折叠状态。卷仍走 WritingTreeGroupHeader 的展开/收起。
    onWorkspaceProjectIdChanged: {
        // Issue #762 评论 5826175490 第 3 点：切换作品时立即刷新冲突。
        // 旧的 conflictPath 属于上一个作品，先清掉避免在新作品里误选中；
        // 外部（SyncPage 全局入口）随后会用 openConflictPath() 给出目标路径。
        root.conflictPath = ""
        root.refreshConflictList()
        // Issue #770 评论 5842877986: 通知外部（main.qml）作品已切到位，
        // 可以消费 pendingConflictPath。用 Qt.callLater 确保本轮属性变更
        // 完全生效后再发，避免外部在绑定尚未更新完时消费 pending。
        Qt.callLater(root.projectReady)
    }

    // Issue #757 评论 5818193510 第 5 点：同步冲突侧栏支持。
    // syncBackendRef 由 main.qml 传入（全局 syncBackend），用于监听同步完成信号
    // 并在冲突产生时刷新冲突列表、打开临时侧栏。
    property var syncBackendRef: null
    // Issue #770 评论 5842877986: 当前作品的冲突列表快照（唯一事实源）。
    // refreshConflictList() 唯一负责填充，RightDrawer/SyncConflictPanel 都消费这一份，
    // 不再各自调 list_sync_conflicts 维护第二份缓存。
    property var syncConflicts: []
    // 是否有未解决冲突 — 派生自 syncConflicts，透传给 RightDrawer 控制冲突 tab 显隐。
    readonly property bool hasConflicts: syncConflicts.length > 0
    // Issue #762 评论 5826175490 第 4 点：冲突路径，由外部（main.qml）设置后
    // 打开右侧抽屉并选中对应冲突。
    property string conflictPath: ""

    signal backToProjects()
    signal openSettings()
    // Issue #790 评论 5875963057: 顶栏收口后的同步/搜索入口
    signal requestSync()
    signal requestSearch()
    // Issue #770 评论 5842877986: 作品切到位后发出，main.qml 据此消费
    // pendingConflictPath（projectId 匹配才消费），不靠猜 Loader 是否已存在。
    signal projectReady()

    // Issue #762 评论 5826175490 第 4 点：当外部设置 conflictPath 时，
    // 刷新冲突列表并打开右侧抽屉到冲突 tab，把 conflictPath 透传给 RightDrawer。
    onConflictPathChanged: {
        if (root.conflictPath) {
            root.refreshConflictList();
            if (root.hasConflicts) {
                // Issue #825：工具 rail 按工具 key 表达选中态，没有 tab 下标。
                root.requestToolPaneOpen("conflict", false);
            }
        }
    }

    function flushActiveEditorBeforeSync() {
        root.preSyncFlushFailureReason = ""
        if (!editorController.chapterId || !editorController.projectId || !editorController.volumeId) return true;
        var saved = editorController.flushActiveEditorBeforeSync();
        root.preSyncFlushFailureReason = editorController.syncFlushFailureReason || "";
        return saved;
    }

    // Issue #829：Core 的当前写作速度以「最近一分钟写了多少字」为计算口径，
    // 查询窗口仍为 60 秒；底栏展示时再折算为字/时。
    readonly property int statusSpeedWindowSeconds: 60

    // ── Issue #829：写作页底部状态栏的数据 ──
    // 全部取自 Core 的写作统计，不在端侧自己算，也不用章节字数顶替：
    //   左段 = Core 当前写作速度的 charsPerMinute（底栏折算为字/时）；
    //   中段 = 今日纯输入字数 summary.totalHumanTypedChars。
    // 统计按 Core 的 writing event 落盘，所以只在打开 / 切章 / 定时器到点时重拉，
    // 不做逐键刷新——每敲一个字去查一次只会拿到同一个值。
    property int latestCharsPerMinute: 0
    property int todayTypedChars: 0
    // 右段：HH:mm。定时器只做格式化，不碰任何业务状态。
    property string clockText: ""
    Timer {
        interval: 30000
        repeat: true
        running: true
        onTriggered: {
            root.updateClockText()
            root.refreshWritingStatusData()
        }
    }

    function updateClockText() {
        var now = new Date()
        var hh = ("0" + now.getHours()).slice(-2)
        var mm = ("0" + now.getMinutes()).slice(-2)
        root.clockText = hh + ":" + mm
    }

    // 保存状态是不是异常态：Core 的 save_status 是自由文本，
    // 只有明确是失败时才用 error 色，其余（已保存 / 未保存 / 保存中）走中性色，
    // 免得「未保存」这种正常中间态被画成红色。
    readonly property bool saveStatusIsError: /失败|error|failed/i.test(editorController.saveStatus || "")

    // 一次性刷新状态栏的左段（速度）和中段（今日进度）。
    // Issue #935：同样改为异步请求——计算在 stats worker 串行队列里完成，
    // 结果经 editorBackendRef 的 writing_status_ready / writing_status_failed
    // 信号发回，requestId 过滤迟到结果；UI 线程不再同步扫描事件文件。
    // 左段用 Core 当前写作速度（窗口 60 秒），不用速度曲线的最后一个桶——
    // 曲线桶只生成到最后一个输入事件，停笔后会一直挂着停笔前的非零值。
    // 中段用 Core 的「今日」汇总，不自己拼「今天是哪一天」：日历日语义
    // 只留在 Core 一处，端侧再拼一份本地日期会在凌晨提前清零。
    // 查询失败保持上一次的值，不清零——Core 拿不到不是"今天写了 0 字"。
    property int statusRequestSerial: 0
    property int pendingStatusRequestId: 0

    function refreshWritingStatusData() {
        var be = root.editorBackendRef
        if (!be) return
        root.statusRequestSerial += 1
        root.pendingStatusRequestId = root.statusRequestSerial
        // 只入队，不在 UI 线程等 result；worker 先处理完此前已入队的编辑事实，
        // 再生成「实时速度 + 今日汇总」快照。
        be.request_writing_status(root.statusRequestSerial, root.statusSpeedWindowSeconds)
    }

    Connections {
        target: root.editorBackendRef
        function onWriting_status_ready(requestId, status) {
            if (requestId !== root.pendingStatusRequestId) return
            if (status && status.currentSpeed && status.currentSpeed.charsPerMinute !== undefined) {
                root.latestCharsPerMinute = Math.round(status.currentSpeed.charsPerMinute)
            }
            if (status && status.today && status.today.totalHumanTypedChars !== undefined) {
                root.todayTypedChars = status.today.totalHumanTypedChars
            }
        }
        function onWriting_status_failed(requestId, message) {
            if (requestId !== root.pendingStatusRequestId) return
            // 保留上一次的值，不清零；只记一笔便于诊断。
            console.warn("writing status refresh failed:", message)
        }
    }

    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)
    signal renameItemRequested(var itemData)
    signal deleteItemRequested(var itemData)

    function requestEditorFocus() {
        Qt.callLater(function() {
            editorViewport.requestEditorFocus();
        });
    }

    // Issue #833 复核：WritingTreeController / treeModel / populateTreeModel
    // 已随 sidebarRect 内容一起搬进 WritingChapterNavigation.qml，这里不再保留。

    EditorController {
        id: editorController
        targetEditorItem: editorViewport.editorItem
        editorViewportRef: editorViewport
        editorBackendRef: root.editorBackendRef
        dt: root.dt
        // Issue #709 评论 issue-body-709: 传入 themeController 使
        // logRenderColorProbe 能读取 runtime state。
        themeControllerRef: root.themeController
        // Issue #933: 设置后端显式传入，替代 EditorController 内裸查全局。
        settingsBackendRef: root.settingsBackendRef
        onEmptySaveBlocked: function(msg) {
            emptySaveDialogText.text = msg;
            emptySaveDialog.open();
        }
    }

    Dialog {
        id: emptySaveDialog
        modal: true
        width: 360
        height: 180
        anchors.centerIn: parent
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.largeSpacing
            Kirigami.Heading {
                level: 4
                text: qsTr("保存被阻止")
            }
            Label {
                id: emptySaveDialogText
                Layout.fillWidth: true
                text: qsTr("空内容保存被阻止，请输入内容后重试")
                color: Kirigami.Theme.disabledTextColor
                wrapMode: Text.Wrap
            }
            Button {
                text: qsTr("确定")
                highlighted: true
                Layout.alignment: Qt.AlignRight
                onClicked: emptySaveDialog.close()
            }
        }
    }

    // Issue #833 复核：onTreeChanged -> populateTreeModel() 已删，
    // treeModel 现在由 WritingChapterNavigation 内部维护。
    Component.onCompleted: {
        // Issue #920 复核：先恢复平台本地保存的侧栏宽度，再按它布局。
        root.restorePersistedPaneWidths()
        // Issue #829：启动时先起状态栏的时钟。
        root.updateClockText()
        // Issue #829：状态栏左段（写作速度）也来自 Core 统计，起步就拉一次。
        root.refreshWritingStatusData()

        var sel = (root.appState && root.appState.selected)
                ? root.appState.selected : null

        if (sel
                && sel.projectId === root.workspaceProjectId
                && sel.chapterId) {
            root.openChapter(
                sel.projectId,
                sel.volumeId || "",
                sel.chapterId,
                ""
            )
        } else {
            editorController.clearActiveChapter()
        }

        root.requestEditorFocus()
        // Issue #762 评论 5826175490 第 3 点：打开作品时立即刷新冲突，不等同步结束
        root.refreshConflictList()
        // Issue #829：打开作品后拉一次今日统计，供底部状态栏显示。
        root.refreshWritingStatusData()
    }

    function openChapter(pId, vId, cId, cTitle) {
        if (!pId || !vId || !cId) return;
        // Prevent re-opening the same chapter (anti-loop guard)
        if (editorController.projectId === pId &&
            editorController.volumeId === vId &&
            editorController.chapterId === cId &&
            !editorController.isLoadingChapter) {
            return;
        }

        editorViewport.snapNextCursorUpdate();
        // loadChapterContentWithIds returns null on failure, result object on success.
        // State is only updated after content is successfully loaded.
        var result = editorController.loadChapterContentWithIds(pId, vId, cId);
        if (result) {
            var d = result.data || {};
            editorController.projectId = d.projectId || pId;
            editorController.volumeId = d.volumeId || vId;
            editorController.chapterId = d.chapterId || cId;
            editorController.chapterTitle = d.title || cTitle || "";
            editorViewport.snapNextCursorUpdate();
            // Issue #715: 切章后明确把新章节视口设为顶部。
            // 旧章节的 contentY 不应继承到新章节，否则动画层会画到错误 Y 坐标。
            // contentY -> scroll_y 绑定会同步给 Rust，确保两层使用同一滚动坐标。
            editorViewport.resetScroll();
            if (root.projectBackendRef)
                root.projectBackendRef.select_chapter(pId, vId, cId)
            root.requestEditorFocus();
            // Issue #829：切章后重拉今日统计（统计按 writing event 落盘，
            // 切章是天然的刷新点，不必逐键去查）。
            root.refreshWritingStatusData();
        }
    }

    function reloadActiveChapter() {
        if (editorController.projectId && editorController.volumeId && editorController.chapterId) {
            editorViewport.snapNextCursorUpdate();
            editorController.loadChapterContentWithIds(
                editorController.projectId,
                editorController.volumeId,
                editorController.chapterId
            );
        }
    }

    // Issue #754 评论 5815901258: 外部内容变更（如同步）后的编辑器收口。
    // 以权威 appState.selected 为准：选中章节存在则 reload/打开，不存在则清空编辑器，
    // 避免同步删除当前章节/分卷/作品后编辑器仍残留已删除正文。
    function reconcileActiveChapter(selected) {
        if (!selected || !selected.chapterId) {
            editorController.clearActiveChapter();
            return;
        }

        if (editorController.projectId !== selected.projectId
                || editorController.volumeId !== selected.volumeId
                || editorController.chapterId !== selected.chapterId) {
            openChapter(
                selected.projectId || "",
                selected.volumeId || "",
                selected.chapterId || "",
                ""
            );
            return;
        }

        reloadActiveChapter();
    }

    color: Kirigami.Theme.backgroundColor

    // Issue #977：顶栏已上移到 main.qml 的 DesktopTopBar。本页只负责顶栏以下的
    // 写作主体：「章节导航 | 正文 | 工具 pane | 最右工具 rail」。
    // Issue #922：内容区改用 Qt 原生 SplitView 管理三栏，两道分隔线由 Qt 自己生成
    // 并驱动宽度；最右 WritingToolRail 仍固定宽度，留在外层 RowLayout
    // （不放进 SplitView，避免造第三条可拖动线）。
    RowLayout {
        anchors.fill: parent
        spacing: 0

            // SplitView 始终承载章节树、正文与右工具 pane；次要 pane 按有效宽度让位。
            SplitView {
                id: paneSplitView
                Layout.fillWidth: true
                Layout.fillHeight: true
                orientation: Qt.Horizontal

                // Issue #922：原生分隔线。纯视觉 Rectangle，不手工放 MouseArea /
                // DragHandler（Qt 文档指出会与原生分割器输入冲突）。
                handle: Rectangle {
                    id: handleDelegate
                    implicitWidth: root.splitHandleWidth   // 视觉仍 6vp
                    // Issue #922 复核：原生分隔线命中区只有 6vp，高 DPI 上易点空。
                    // 用 containmentMask 把命中范围扩到 14vp（左右各多 4vp），
                    // 不重叠加鼠标拦截，也不动视觉宽度。
                    containmentMask: Item {
                        x: (handleDelegate.width - width) / 2
                        width: 14                          // 左右各多 4vp 命中
                        height: paneSplitView.height
                    }
                    color: SplitHandle.pressed
                           ? Kirigami.Theme.highlightColor
                           : (SplitHandle.hovered
                              ? Kirigami.Theme.focusColor
                              : "transparent")
                    Rectangle {
                        // 可视线：1vp 竖线，居中。命中区由整个 handle 承担。
                        anchors.centerIn: parent
                        width: 1
                        height: parent.height
                        color: Kirigami.Theme.disabledTextColor
                        opacity: 0.3
                    }
                }

                // 拖动结束（用户松手）才持久化一次，不在每一帧写 JSON。
                onResizingChanged: {
                    if (!paneSplitView.resizing) {
                        root.persistPaneWidths()
                    }
                }

                // Left sidebar: volume/chapter tree
                // 章节导航作为 SplitView 子项，宽度由 preferredWidth 表达。
                WritingChapterNavigation {
                    id: sidebarRect
                    visible: root.chapterNavigationShown
                    // Issue #922：只有一份宽度状态就是 SplitView.preferredWidth，
                    // 用户期望宽度保存于此；最小宽度钳制在这里表达。
                    SplitView.minimumWidth: root.listPaneMinWidth
                    SplitView.preferredWidth: 320

                    dt: root.dt
                    tree: root.tree
                    workspaceProjectId: root.workspaceProjectId
                    workspaceProjectTitle: root.workspaceProjectTitle
                    currentChapterId: editorController.chapterId

                    onOpenChapter: function(pId, vId, cId, cTitle) {
                        root.openChapter(pId, vId, cId, cTitle)
                    }
                    onCreateVolumeRequested: function(pId) {
                        root.createVolumeRequested(pId)
                    }
                    onCreateChapterRequested: function(pId, vId) {
                        root.createChapterRequested(pId, vId)
                    }
                    onRenameItemRequested: function(itemData) {
                        root.renameItemRequested(itemData)
                    }
                    onDeleteItemRequested: function(itemData) {
                        root.deleteItemRequested(itemData)
                    }
                }

                // Middle Area: Toolbar + Editor
                ColumnLayout {
                    // 正文列吃满剩余宽度，左右拖动都会改变它；工具轨和正文优先保留。
                    SplitView.fillWidth: true
                    SplitView.minimumWidth: root.editorMinWidth
                    spacing: 0

                // Editor Container Area
                Rectangle {
                    id: editorAreaRect
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    color: Kirigami.Theme.backgroundColor

                    // Issue #829：正文区顶部的折叠钮（手稿「宽屏全打开」/「左右缩回」
                    // 都在正文区顶部居中画了一个三角）。它是浮层，不占 ColumnLayout 的
                    // 行高，所以正文纸面高度不受影响；左树收起 / 展开时按钮原地切换
                    // ⌄ / ⌃。真正的「收起后重新拉开」仍由贴边把手 leftPaneHandle 负责。
                    PaneFoldButton {
                        anchors.horizontalCenter: parent.horizontalCenter
                        anchors.top: parent.top
                        anchors.topMargin: Kirigami.Units.smallSpacing
                        z: 30
                        dt: root.dt
                        // Issue #833 复核3：任何模式下都给用户入口切换章节导航。
                        visible: true
                        // 统一读 chapterNavigationShown：展开时显示收起箭头 ⌃，
                        // 收起时显示展开箭头 ⌄。
                        glyph: root.chapterNavigationShown ? "\u2303" : "\u2304"
                        onTriggered: {
                            if (root.chapterNavigationShown) {
                                root.closeChapterNavigation();
                                root.requestEditorFocus();
                            } else {
                                root.requestChapterNavigationOpen();
                            }
                        }
                    }

                    // Centered paper container
                    Item {
                        anchors.fill: parent
                        anchors.leftMargin: Kirigami.Units.smallSpacing
                        anchors.rightMargin: Kirigami.Units.smallSpacing
                        anchors.topMargin: Kirigami.Units.smallSpacing
                        anchors.bottomMargin: Kirigami.Units.smallSpacing

                        // Paper background 跟随正文列的可用宽度。
                        // 编辑区组件（SujianEditorItem）跟随 paperBg 宽度；
                        // 编辑器交互由 EditorController + SujianEditorItem 直接管理。
                        Rectangle {
                            id: paperBg
                            width: parent.width
                            height: parent.height
                            anchors.horizontalCenter: parent.horizontalCenter
                            color: dt.editorBackground
                            radius: Kirigami.Units.cornerRadius
                        }

                        // Issue #833：删除 paperBg 左右两个拖拽改宽 MouseArea。
                        // 正文宽度属于 Editor 列布局，paperBg 直接跟随可用宽度。

                        EditorViewport {
                            id: editorViewport
                            anchors.fill: paperBg
                            dt: root.dt
                            settingsBackendRef: root.settingsBackendRef
                            editorControllerRef: editorController
                            onExplicitClearRequested: editorController.markPotentialExplicitClear()
                        }

                    }

                    // Empty state
                    ColumnLayout {
                        anchors.centerIn: parent
                        spacing: Kirigami.Units.largeSpacing
                        visible: !editorController.chapterId

                        Kirigami.Icon {
                            source: "document-open"
                            color: Kirigami.Theme.disabledTextColor
                            opacity: 0.4
                            Layout.alignment: Qt.AlignHCenter
                            implicitWidth: Kirigami.Units.iconSizes.large
                            implicitHeight: Kirigami.Units.iconSizes.large
                        }
                        Label {
                            text: qsTr("请选择或新建章节")
                            color: Kirigami.Theme.disabledTextColor
                            Layout.alignment: Qt.AlignHCenter
                        }
                    }

                }

                // Issue #829：底部状态栏。挂在中间 Editor 这一列的末尾，
                // 所以它只横贯「章节树右缘 ~ 工具面板左缘」，和手稿一致——
                // 左树 / 右工具面板 / 最右 rail 都不参与这条带子。
                // 硬约束 6：只放字数 / 进度 / 时间 / 保存状态，不放任何导航。
                WritingStatusBar {
                    Layout.fillWidth: true
                    Layout.preferredHeight: implicitHeight
                    dt: root.dt
                    charsPerMinute: root.latestCharsPerMinute
                    progressCurrent: root.todayTypedChars
                    clockText: root.clockText
                    saveStatus: editorController.saveStatus
                    saveStatusIsError: root.saveStatusIsError
                }
            }

            // Issue #825：工具 pane（ToolPane 角色）—— 只显示 rail 选中的那个工具内容。
            RightDrawer {
                id: rightDrawerRect
                // Issue #922：工具 pane 宽度由 SplitView.preferredWidth 表达用户期望宽度，
                // SplitView 拖动时自动更新它；最小宽度钳制在这里表达。
                SplitView.minimumWidth: root.toolPaneMinWidth
                SplitView.preferredWidth: 240
                // 只有用户请求展开且当前空间容纳得下时才参与 SplitView。
                visible: root.drawerOpen
                dt: root.dt
                editorBackendRef: root.editorBackendRef
                editorControllerRef: editorController
                isOpen: root.drawerOpen
                selectedTool: root.drawerTool
                // Issue #835：透传星图后端与 appState 给 RightDrawer（任务 4 已加属性）。
                starmapBackendRef: root.starmapBackendRef
                // Issue #835 评论 6019713847: 透传真实 StarMapController 给 RightDrawer，
                // StarMapPage 据此调 listStarmaps() 读真实根星图。
                starMapControllerRef: root.starMapControllerRef
                appState: root.appState
                // Issue #757 评论 5818193510 第 5 点：冲突侧栏绑定。
                syncBackendRef: root.syncBackendRef
                workspaceProjectId: root.workspaceProjectId
                hasConflicts: root.hasConflicts
                // Issue #770 评论 5842877986: 透传完整冲突快照给 RightDrawer，
                // 不再让 RightDrawer/SyncConflictPanel 各自查一份。
                syncConflicts: root.syncConflicts
                // Issue #762 评论 5826175490 第 4 点：透传请求的冲突路径给 RightDrawer。
                // requestedConflictPath 是单向输入，SyncConflictPanel 绝不在内部赋值。
                requestedConflictPath: root.conflictPath
                onCloseRequested: root.closeToolPane()
                onConflictToolRequested: {
                    // 冲突刚产生或解决后刷新 — 选中 rail 的「冲突」工具。
                    // allowCollapseLeft=false：冲突自动弹出不该顺手收起用户的章节栏，
                    // 放不下就直接回滚（冲突入口仍在全局同步面板里）。
                    root.requestToolPaneOpen("conflict", false);
                    root.refreshConflictList();
                }
            }
            } // end SplitView paneSplitView

            // Issue #825：最右竖向工具 rail（Core 的 ToolRail 角色）。
            // 只负责「选哪个工具 + 展开/收起 ToolPane」，工具内容在右边的 ToolPane 里。
            // 顶部工具条带 ToolbarTrailing 的同步/搜索/设置不搬到这里。
            // Issue #922：rail 固定 56vp，留在外层 RowLayout，不进 SplitView，
            // 不给最右加第三条可拖动线。
            WritingToolRail {
                id: toolRailRect
                visible: root.toolRailVisible
                Layout.preferredWidth: root.toolRailWidth
                Layout.minimumWidth: root.toolRailWidth
                Layout.maximumWidth: root.toolRailWidth
                // Issue #835：内容区 RowLayout cross-axis 高度填满。
                Layout.fillHeight: true
                dt: root.dt
                hasConflicts: root.hasConflicts
                selectedTool: root.drawerTool
                toolPaneOpen: root.drawerOpen
                railWidth: root.toolRailWidth
                onToolRequested: function(toolKey) { root.requestToolPaneOpen(toolKey, true); }
                onToolPaneToggled: {
                    root.toggleToolPane();
                    if (root.drawerOpen) root.requestEditorFocus();
                }
            }
        }

    Connections {
        target: root.settingsBackendRef
        function onSettings_changed() {
            editorController.applyCurrentSettings();
        }
    }

    // Issue #721: 删除 onIsDarkChanged -> applyCurrentSettings() 连接。
    // 主题切换不需要进入 applyCurrentSettings()——正文/选区/光标颜色已通过
    // text_color/selection_color/selected_text_color/cursor_color 绑定自动下发，
    // Rust 侧各自走 color_only_changed() -> request_scene_rebuild()，不会清空动画。
    // 上面 settingsBackendRef.onSettings_changed 仍保留，它只保护保存，不控制编辑器动画。

    Connections {
        target: editorController
        function onChapterIdChanged() {
            if (editorController.chapterId) {
                root.requestEditorFocus();
                // 保存导航状态（包含当前章节信息）
                if (root.workspaceBackendRef) {
                    root.workspaceBackendRef.save_last_navigation_state(
                        "writing",
                        root.workspaceProjectId || "",
                        editorController.volumeId || "",
                        editorController.chapterId || "",
                        ""
                    );
                }
            }
        }
    }

    // Issue #757 评论 5818193510 第 5 点：刷新当前作品的冲突列表。
    // Issue #762 评论 5826175490 第 3 点：不再只由 sync_action_completed 触发——打开作品、
    // 切换作品、sync_conflicts_changed 时都调，已有冲突不需要用户先手动同步一次才看得到。
    // 不在 QML 维护第二份可编辑正文，只通过 SyncBackend QML 方法拿冲突列表。
    function refreshConflictList() {
        var sb = root.syncBackendRef;
        if (!sb || !root.workspaceProjectId) {
            root.syncConflicts = [];
            return;
        }
        var raw = sb.list_sync_conflicts("project", root.workspaceProjectId);
        var resp;
        try { resp = JSON.parse(raw); } catch (e) { resp = null; }
        if (resp && resp.success && resp.data && resp.data.conflicts) {
            // Issue #770 评论 5842877986: 把完整数组写进 syncConflicts（唯一快照），
            // 不再只设 hasConflicts 布尔。RightDrawer/SyncConflictPanel 都消费这一份。
            root.syncConflicts = resp.data.conflicts;
        } else {
            root.syncConflicts = [];
        }
        // Issue #757 评论 5819894306 第 3 点：最后一个冲突解决后自动退出冲突工具。
        // hasConflicts 为 false 且工具 pane 停在「冲突」时，收起 pane，
        // 避免落到没有内容的工具状态。仍由 WritingWorkspace 统一持有工具状态，
        // 不让 RightDrawer 自己猜外部 drawer 状态。
        if (!root.hasConflicts && root.drawerTool === "conflict") {
            root.drawerTool = "starmap";
        }
    }

    function checkConflictsAfterSync() {
        // Issue #762 评论 5826175490 第 2/3 点：不再依赖 sync_operation_state 的最终 status。
        // 冲突是持久状态，和"当前有没有正在跑一轮同步"是两回事——只要本地确实有
        // unresolved conflict 就刷新并选中冲突工具，status 说什么不影响判断。
        root.refreshConflictList();
        if (root.hasConflicts) {
            root.requestToolPaneOpen("conflict", false);
        }
    }

    // Issue #762 评论 5826175490 第 4 点：外部（SyncPage 全局冲突入口）请求选中某条冲突。
    // 用显式方法而不是只靠 conflictPath 属性变化，保证已打开同一作品、重复请求同一路径时
    // 也会重新刷新并切到冲突工具。打开目标作品由 main.qml 负责，不要求同步先结束。
    function openConflictPath(path) {
        root.conflictPath = path || "";
        root.refreshConflictList();
        if (root.conflictPath && root.hasConflicts) {
            root.requestToolPaneOpen("conflict", false);
        }
    }

    Connections {
        target: root.syncBackendRef
        function onSync_action_completed() {
            root.checkConflictsAfterSync();
        }
    }

    // Issue #762 评论 5826175490 第 3 点：监听 sync_conflicts_changed 信号。
    // SyncBackend 在同步开始前 / 每个 target 结束 / 整轮同步结束 / resolve 成功后都会发，
    // 同步过程中新产生的冲突立即出现在冲突 tab，不必等 30 秒的全量同步跑完。
    Connections {
        target: root.syncBackendRef
        function onSync_conflicts_changed() {
            root.refreshConflictList();
        }
    }


    // 宽屏折叠把手：贴边窄悬浮把手，收起后只剩把手本身，
    // 空 pane 由 SplitView 忽略不可见子项自然让出宽度，不留空占位。
    // 把手只改 Qt UI 布局，不进入 Core 编辑事务——编辑会话不受影响。
    Rectangle {
        id: leftPaneHandle
        visible: root.toolRailVisible
        z: 50
        width: Kirigami.Units.gridUnit
        height: Kirigami.Units.gridUnit * 3.5
        radius: width / 2
        color: leftPaneHandleHover.containsMouse || leftPaneHandleHover.pressed
                   ? Kirigami.Theme.alternateBackgroundColor
                   : Kirigami.Theme.backgroundColor
        border.color: leftPaneHandleHover.containsMouse ? Kirigami.Theme.focusColor
                                                        : Kirigami.Theme.disabledTextColor
        border.width: 1
        x: !root.chapterNavigationShown
           ? Kirigami.Units.smallSpacing
           : Math.max(0, Math.min(root.width - width - Kirigami.Units.smallSpacing, sidebarRect.width - width / 2))
        y: Math.round((root.height - height) / 2)

        Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
        Behavior on border.color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

        Kirigami.Icon {
            anchors.centerIn: parent
            source: root.chapterNavigationShown ? "arrow-left" : "arrow-right"
            color: Kirigami.Theme.textColor
            implicitWidth: Kirigami.Units.iconSizes.smallMedium
            implicitHeight: Kirigami.Units.iconSizes.smallMedium
        }

        MouseArea {
            id: leftPaneHandleHover
            anchors.fill: parent
            anchors.margins: -Kirigami.Units.smallSpacing
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
                if (root.chapterNavigationShown) {
                    root.closeChapterNavigation();
                    root.requestEditorFocus();
                } else {
                    root.requestChapterNavigationOpen();
                }
            }
        }
    }

    Rectangle {
        id: rightPaneHandle
        visible: root.toolRailVisible
        z: 50
        width: Kirigami.Units.gridUnit
        height: Kirigami.Units.gridUnit * 3.5
        radius: width / 2
        color: rightPaneHandleHover.containsMouse || rightPaneHandleHover.pressed
                   ? Kirigami.Theme.alternateBackgroundColor
                   : Kirigami.Theme.backgroundColor
        border.color: rightPaneHandleHover.containsMouse ? Kirigami.Theme.focusColor
                                                         : Kirigami.Theme.disabledTextColor
        border.width: 1
        Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
        Behavior on border.color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

        // Issue #825 复核第5项：右把手贴在“正文 | 工具区”分隔边缘上（RightDrawer 展开时
        // 用它的左边缘），只有收起时才退回 ToolRail 左边缘。
        // 工具 pane 收起后 rail 仍然常驻（Core 的 ToolRail 恒有宽度），
        // 所以收起态贴的是 rail 左缘，不是窗口最右缘。
        x: root.drawerOpen
           ? Math.max(0, Math.min(root.width - width - Kirigami.Units.smallSpacing, rightDrawerRect.x - width / 2))
           : Math.max(0, Math.min(root.width - width - Kirigami.Units.smallSpacing, toolRailRect.x - width / 2))
        y: Math.round((root.height - height) / 2)

        Kirigami.Icon {
            anchors.centerIn: parent
            source: root.drawerOpen ? "arrow-right" : "arrow-left"
            color: Kirigami.Theme.textColor
            implicitWidth: Kirigami.Units.iconSizes.smallMedium
            implicitHeight: Kirigami.Units.iconSizes.smallMedium
        }

        MouseArea {
            id: rightPaneHandleHover
            anchors.fill: parent
            anchors.margins: -Kirigami.Units.smallSpacing
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
                root.toggleToolPane()
                if (!root.drawerOpen) root.requestEditorFocus()
            }
        }
    }

}
