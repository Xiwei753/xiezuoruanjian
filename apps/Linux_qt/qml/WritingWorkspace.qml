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
// 关于 LayoutPlan 和布局策略
//
// LayoutPlan 是布局策略对象，控制：
//   - 侧栏宽度（sidebarWidth）
//   - 内容区最大宽度（contentMaxWidthVp）
//   - 外壳模式（shellMode）
//   - 内容区内边距（contentPaddingVp）
//
// LayoutPlan 不直接控制以下内容：
//   - 自研 SujianEditorItem 的 QSG 渲染
//   - 光标和选区的 IME 交互逻辑
//   - QTextLayout 的排版细节
//   - Rust Coordinator → Scene Graph 动画渲染参数
//
// 编辑器交互（包括IME处理）由 EditorController 和 SujianEditorItem
// 直接管理，不走 Qt QSG 渲染管线，不受 LayoutPlan 约束
//
// 组成（Issue #825 按 Core 七角色组织）：
//   WritingWorkbenchToolbar：上方一条贯通工具条带
//     （ToolbarLeading 返回/撤销/重做 | ToolbarCenter 字号/行距/段落/排版 |
//       ToolbarTrailing 同步/搜索/设置）
//   下方内容区 SplitView：
//     ChapterNavigation (章节树) | Editor (SujianEditorItem) |
//     ToolPane (工具内容) | ToolRail (最右竖向工具栏)
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Sujian 1.0

Rectangle {
    id: root
    required property var dt
    property var editorBackendRef: null
    property var projectBackendRef: null
    // Issue #709 评论 issue-body-709: 传入 themeController 给 EditorController，
    // 使 logRenderColorProbe 能读取 ThemeController runtime state。
    property var themeController: null
    property var appState: ({})
    // Issue #835：星图后端透传给 RightDrawer（任务 4 已为 RightDrawer 加 starmapBackendRef 属性）。
    // 由 main.qml 注入，WritingWorkspace 只做透传，不在本地伪造星图业务状态。
    property var starmapBackendRef: null
    // Issue #835 评论 6019713847: StarMapPage 需要真实 StarMapController 读根星图，
    // 由 main.qml 注入 globalStarMapController，透传给 RightDrawer.starMapControllerRef。
    property var starMapControllerRef: null
    property var tree: []
    // Issue #825：左右 pane 的展开状态只是端侧 UI 状态（不进 Core、不进同步），
    // 它们作为 WorkbenchVisibility 输入重新算 Core 的七角色 plan。
    // 工具 pane 用「选中的工具 key」表达："" = 收起，非空 = 展开并显示该工具。
    // Issue #829：手稿里的常驻入口是 starmap / ai；冲突只作为动态附加工具。
    // 不再保留 drawerOpen + drawerTab 两份状态，避免第二套开关。
    // Issue #842 任务3：drawerTool 只表示当前工具选择，不再同时表达折叠状态。
    // rightPaneCollapsed 独立控制右栏折叠；drawerOpen 由 rightPaneCollapsed 派生。
    property string drawerTool: "starmap"
    property bool rightPaneCollapsed: true
    readonly property bool drawerOpen: !root.rightPaneCollapsed
    property bool leftPaneCollapsed: false
    // Issue #842 任务2：用户拖拽请求的 pane 宽度，0 代表用 Core 默认。
    property real requestedChapterNavWidth: 0
    property real requestedToolPaneWidth: 0
    // 拖拽起始宽度记录（内部属性）
    property real _dragStartChapterNavWidth: 0
    property real _dragStartToolPaneWidth: 0
    // Issue #833 复核：SinglePane（非 Workbench 或 Core 退回单栏）下章节导航
    // 作为覆盖在正文上的浮层打开，不参与 RowLayout 宽度分配。
    // Workbench 下章节导航由 leftPaneCollapsed + Core ChapterNavigation bounds 管理，
    // 这个属性只在 SinglePane 下生效。
    property bool singlePaneNavOpen: false
    readonly property bool wideWorkbench: layoutPlan === null || layoutPlan.workspaceLayoutMode === "Workbench"
    // Issue #825：Core 工作台布局计划（appBackend.resolve_workbench_layout 直通）。
    // 七角色 bounds 与最终模式都由 Core 决定，QML 只按 bounds 量/摆。
    property var workbenchPlan: null
    readonly property bool coreWorkbench: !!(workbenchPlan && workbenchPlan.mode === "Workbench")
    // 关于 LayoutPlan 和布局策略
    // layoutPlan 是外部注入的布局策略（由上层根据屏幕尺寸和设置决定）
    // 编辑器交互（包括IME处理）由 EditorController 和 SujianEditorItem
    // 直接管理，不走 Qt QSG 渲染管线，不受 LayoutPlan 约束
    property var layoutPlan: null

    // ── Issue #825：Core 七角色 bounds → QML 宽度 ──
    // 只在 Workbench 模式下用 Core 的尺寸；SinglePane（Core 明确退回单栏）时
    // 回落到旧的 SplitView 设定值，Qt 不自己再判一次宽度断点。
    function roleBounds(role) {
        if (!root.workbenchPlan || !root.workbenchPlan.placements) return null;
        var list = root.workbenchPlan.placements;
        for (var i = 0; i < list.length; i++) {
            if (list[i].role === role) return list[i].bounds;
        }
        return null;
    }

    function roleWidth(role, fallback) {
        var b = root.roleBounds(role);
        if (!b) return fallback;
        return Math.max(0, b.rightDp - b.leftDp);
    }

    readonly property real chapterNavWidth: root.roleWidth("ChapterNavigation", -1)
    readonly property real editorWidth: root.roleWidth("Editor", -1)
    readonly property real toolPaneWidth: root.roleWidth("ToolPane", -1)
    readonly property real toolRailWidth: root.roleWidth("ToolRail", -1)

    // ── Issue #825 复核第3项：工具条带的三个分组 bounds 也吃 Core ──
    // Core 的 plan 里 ToolbarLeading / ToolbarCenter / ToolbarTrailing 在同一条顶部带子上，
    // toolbarHeight 取这条带子的高度。Core 判 SinglePane 时不给这些 bounds（返回 -1），
    // 此时 WritingWorkbenchToolbar 按内容自适应，保证窄窗口仍排得下。
    readonly property real toolbarHeight: {
        var b = root.roleBounds("ToolbarCenter")
        return b ? Math.max(0, b.bottomDp - b.topDp) : -1
    }
    readonly property real toolbarLeadingWidth: root.roleWidth("ToolbarLeading", -1)
    readonly property real toolbarCenterWidth: root.roleWidth("ToolbarCenter", -1)
    readonly property real toolbarTrailingWidth: root.roleWidth("ToolbarTrailing", -1)

    // ── Issue #825 复核5第1项：最终结构只听 resolve_workbench_layout().mode ──
    // WorkspaceLayoutMode.Workbench 只是第一层壳模式；Core 还会按当前 pane visibility
    // 再判一次最终模式（600～695vp 时左右都展开就放不下，Core 明确回 SinglePane）。
    // plan 一旦返回，mode=SinglePane 就只留 Editor：章节栏 / 工具 pane / 工具 rail
    // 全部隐藏，不再退回旧的 240/480/240 三栏宽度自己硬塞。
    readonly property bool planResolved: root.wideWorkbench && root.workbenchPlan !== null
    readonly property bool hideContentPanes: root.planResolved && !root.coreWorkbench

    // Issue #833 复核3：SinglePane 和 Workbench 是两套壳结构，状态互不污染。
    // singlePaneMode：非 Workbench，或 Core 因空间不够退回单栏。
    // chapterNavigationShown：当前模式下章节导航是否展开，统一读这一份，
    // PaneFoldButton / 把手不再直接读 leftPaneCollapsed。
    readonly property bool singlePaneMode: !root.wideWorkbench || root.hideContentPanes
    readonly property bool chapterNavigationShown:
        root.singlePaneMode ? root.singlePaneNavOpen : !root.leftPaneCollapsed
    // 从 SinglePane 切回 Workbench 时清掉浮层状态，避免 overlay 继续盖在 Workbench 上。
    onSinglePaneModeChanged: {
        if (!root.singlePaneMode) {
            root.singlePaneNavOpen = false;
        }
    }

    // 宽屏下 Core 已经把七角色尺寸算好：pre/min/max 三者取同一个值，
    // 让 SplitView 只负责"可见子项参与剩余空间分配"，不再被用户拖拽改宽度。
    // 非宽屏仍保留原来的可拖拽区间（最小 180 / 最大 420）。
    // 注意：可收起角色（ChapterNavigation / ToolPane）收起时 Core 给的宽度就是 0，
    // 0 是合法宽度不是 plan 无效，所以这里只校验 Editor / ToolRail 的宽度，
    // 另外确认四个角色在 placements 里确实存在。
    readonly property bool coreSized: root.coreWorkbench
                                   && root.hasContentRole("ChapterNavigation")
                                   && root.hasContentRole("Editor")
                                   && root.hasContentRole("ToolPane")
                                   && root.hasContentRole("ToolRail")
                                   && root.editorWidth > 0
                                   && root.toolRailWidth > 0
    // Issue #825：最右工具 rail 与两个折叠把手在 Core 判定 Workbench 时始终出现，
    // 不跟工具 pane 是否展开绑定——Core 的 ToolRail 恒有宽度，
    // 工具 pane 收起后 rail 仍是唯一能把它再拉出来的入口。
    // Core 明确退回 SinglePane（窗口放不下七角色）时，Qt 不自己挤一个 rail 出来，
    // 收起/展开工具 pane 回到正文区里那个箭头按钮。
    readonly property bool toolRailVisible: root.coreSized

    function hasContentRole(role) {
        return root.roleBounds(role) !== null
    }

    // Issue #825 复核6第1点：展开侧 pane 是一个「请求」，不是直接写状态。
    // 窄窗口下 Core 按当前可见性重判最终 mode：600～695vp 里左章节栏开着时再展开
    // 右工具 pane，最小需求 200+240+200+56 = 696vp 放不下，Core 退回 SinglePane。
    // 如果端侧还留着「右 pane 已展开」，下一轮重算又继续喂 toolPaneVisible=true，
    // 于是永远 SinglePane，而 rail / 折叠把手已被隐藏，用户没有入口再关回去，
    // 只能拉大窗口解锁。
    //
    // Issue #825 复核7第 2 点：改成「先算候选，再一次性提交」。
    // 之前是「先改 QML 属性 → 看 Core 结果 → 再回滚」，失败分支只回滚了自己那侧，
    // 为了腾位置收掉的另一侧没有恢复——用户只想开右栏，失败后左栏反而被关掉了。
    // 现在一个属性都不改就先把候选组合问一遍，哪一组是 Workbench 就整体提交；
    // 两组都不行就什么都不改，不留"已展开但看不见"的端侧状态，也没有中间态闪烁。
    function requestToolPaneOpen(toolKey, allowCollapseLeft) {
        var targetLeftCollapsed = root.leftPaneCollapsed;
        var plan = resolveWorkbenchCandidate(!root.leftPaneCollapsed, true);
        if (!isWorkbenchPlan(plan) && allowCollapseLeft === true) {
            plan = resolveWorkbenchCandidate(false, true);
            targetLeftCollapsed = true;
        }
        if (!isWorkbenchPlan(plan)) return;
        root.leftPaneCollapsed = targetLeftCollapsed;
        root.drawerTool = toolKey || "starmap";
        root.rightPaneCollapsed = false;
        root.workbenchPlan = plan;
    }

    // 左目录栏的展开请求，与右侧对称：必要时让右 pane 让位，仍放不下就不改任何状态。
    function requestChapterNavigationOpen() {
        if (root.singlePaneMode) {
            root.singlePaneNavOpen = true;
            return;
        }
        var targetRightPaneCollapsed = root.rightPaneCollapsed;
        var plan = resolveWorkbenchCandidate(true, !root.rightPaneCollapsed);
        if (!isWorkbenchPlan(plan)) {
            plan = resolveWorkbenchCandidate(true, false);
            targetRightPaneCollapsed = true;
        }
        if (!isWorkbenchPlan(plan)) return;
        root.rightPaneCollapsed = targetRightPaneCollapsed;
        root.leftPaneCollapsed = false;
        root.workbenchPlan = plan;
    }

    // 关闭方向不会把窗口撑小，永远放得下，直接改状态即可（refreshWorkbenchPlan 随后重算）。
    function closeToolPane() {
        root.rightPaneCollapsed = true;
    }

    function closeChapterNavigation() {
        // Issue #833 复核3：按 singlePaneMode 分支，SinglePane 只收浮层，
        // Workbench 只收 leftPaneCollapsed，两套状态互不污染。
        if (root.singlePaneMode) {
            root.singlePaneNavOpen = false;
        } else {
            root.leftPaneCollapsed = true;
        }
    }

    // 展开/收起工具 pane 的统一入口（rail 的展开收起按钮 + 右侧折叠把手）。
    // Issue #842 评论修复（问题2）：展开走 requestToolPaneOpen() 候选入口，
    // 不再绕过「先算候选、必要时收左栏再展开」逻辑。drawerTool 已独立保存当前工具，
    // 不会重新硬切星图；空值兜底 "starmap"。
    function toggleToolPane() {
        if (root.drawerOpen) {
            root.closeToolPane();
        } else {
            root.requestToolPaneOpen(root.drawerTool || "starmap", true);
        }
    }

    // 状态路径：按当前可见性重算并写回 workbenchPlan（窗口尺寸 / 折叠变化时走这里）。
    function refreshWorkbenchPlan() {
        if (!root.wideWorkbench) {
            root.workbenchPlan = null;
            return null;
        }
        root.workbenchPlan = resolveWorkbenchCandidate(!root.leftPaneCollapsed, root.drawerOpen);
        return root.workbenchPlan;
    }

    // Issue #825 复核7第 2 点：候选计算用的纯 helper。
    // 只按给定的可见性问一次 Core，不碰任何属性、不写 workbenchPlan，
    // 这样「算候选」和「提交状态」彻底分开，失败时另一侧的状态不会被弄丢。
    function resolveWorkbenchCandidate(chapterVisible, toolVisible) {
        if (!root.wideWorkbench) return null;
        if (typeof appBackend === "undefined" || !appBackend) return null;
        return appBackend.resolve_workbench_layout(
            root.width,
            root.height,
            chapterVisible,
            toolVisible,
            root.requestedChapterNavWidth,
            root.requestedToolPaneWidth
        );
    }

    function isWorkbenchPlan(plan) {
        return !!plan && plan.mode === "Workbench";
    }

    onWidthChanged: refreshWorkbenchPlan()
    onHeightChanged: refreshWorkbenchPlan()
    onLeftPaneCollapsedChanged: refreshWorkbenchPlan()
    onRightPaneCollapsedChanged: refreshWorkbenchPlan()
    onWideWorkbenchChanged: refreshWorkbenchPlan()
    // Issue #828：根对象上只能有一个 Component.onCompleted。
    // #825 新增的启动期 refreshWorkbenchPlan() 与下面原有的启动初始化块
    // 曾写成两个并列 handler，Qt 报 Property value set multiple times，
    // WritingWorkspace 类型随之不可用，main.qml 加载失败直接退出。
    // 现在只保留下面那一个，refreshWorkbenchPlan() 合并进去当第一句。

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
        if (!editorController.chapterId || !editorController.projectId || !editorController.volumeId) return true;
        return editorController.flushActiveEditorBeforeSync();
    }

    // Issue #829：手稿「86 字/分」是"最近一分钟写了多少字"。
    // 用 Core 的「当前写作速度」查询（window_seconds=60），窗口长度就是 60 秒。
    readonly property int statusSpeedWindowSeconds: 60

    // ── Issue #829：写作页底部状态栏的数据 ──
    // 全部取自 Core 的写作统计，不在端侧自己算，也不用章节字数顶替：
    //   左段 = Core 当前写作速度的 charsPerMinute（手稿「86 字/分」）；
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
    // 查询失败保持上一次的值，不清零——Core 拿不到不是"今天写了 0 字"。
    function refreshWritingStatusData() {
        var be = root.editorBackendRef
        if (!be) return

        // 左段：Core 当前写作速度。
        // 不用写作速度曲线的最后一个桶——曲线桶只生成到最后一个输入事件，
        // 停笔后它会一直挂着停笔前的非零值。也别在这里判断桶是否过期或强制
        // flush 统计事件，平台 writer queue + raw JSONL 是唯一事实源。
        // 失败保持上一次的值，不清零：Core 拿不到不等于"这一刻没写字"。
        var speed = be.get_current_writing_speed(root.statusSpeedWindowSeconds)
        if (speed && speed.charsPerMinute !== undefined) {
            root.latestCharsPerMinute = Math.round(speed.charsPerMinute)
        }

        // 中段：今日纯输入字数。
        // 这里不自己拼"今天是哪一天"（原 todayDateString 已删）：Core 的每日统计
        // 按事件发生地的本地午夜分桶，端侧再拼一份本地日期，一旦时区口径和 Core
        // 错开，凌晨就会出现"今日进度提前清零"。日历日语义只留在 Core 一处。
        // 返回值是 Core JSON 本体（不是 ResultEnvelope），字段是 camelCase。
        var summary = be.get_today_writing_stats_summary_object()
        if (summary && summary.totalHumanTypedChars !== undefined) {
            root.todayTypedChars = summary.totalHumanTypedChars
        }
    }

    signal createVolumeRequested(string projectId)
    signal createChapterRequested(string projectId, string volumeId)
    signal renameItemRequested(var itemData)
    signal deleteItemRequested(var itemData)

    function requestEditorFocus() {
        Qt.callLater(function() {
            if (sujianEditor && sujianEditor.visible && sujianEditor.editor_enabled) {
                sujianEditor.request_text_input_focus();
            }
        });
    }

    // Issue #833 复核：WritingTreeController / treeModel / populateTreeModel
    // 已随 sidebarRect 内容一起搬进 WritingChapterNavigation.qml，这里不再保留。

    EditorController {
        id: editorController
        targetEditorItem: sujianEditor
        editorBackendRef: root.editorBackendRef
        dt: root.dt
        // Issue #709 评论 issue-body-709: 传入 themeController 使
        // logRenderColorProbe 能读取 runtime state。
        themeControllerRef: root.themeController
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
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp16
            AppText {
                dt: root.dt
                text: qsTr("保存被阻止")
                color: dt.textPrimary
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
            AppText {
                dt: root.dt
                id: emptySaveDialogText
                Layout.fillWidth: true
                text: qsTr("空内容保存被阻止，请输入内容后重试")
                color: dt.textSecondary
                font.pointSize: dt.bodyPt
                font.family: dt.fontFamily
                wrapMode: Text.Wrap
            }
            AppButton {
                            text: qsTr("确定")
                dt: root.dt
                variant: "primary"
                Layout.alignment: Qt.AlignRight
                onClicked: emptySaveDialog.close()
            }
        }
    }

    // Issue #833 复核：onTreeChanged -> populateTreeModel() 已删，
    // treeModel 现在由 WritingChapterNavigation 内部维护。
    Component.onCompleted: {
        // Issue #828：启动期先算一次宽屏工作台布局，否则首帧会用上一次的 plan。
        refreshWorkbenchPlan();

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

        sujianEditor.snap_next_cursor_update();
        // loadChapterContentWithIds returns null on failure, result object on success.
        // State is only updated after content is successfully loaded.
        var result = editorController.loadChapterContentWithIds(pId, vId, cId);
        if (result) {
            var d = result.data || {};
            editorController.projectId = d.projectId || pId;
            editorController.volumeId = d.volumeId || vId;
            editorController.chapterId = d.chapterId || cId;
            editorController.chapterTitle = d.title || cTitle || "";
            sujianEditor.snap_next_cursor_update();
            // Issue #715: 切章后明确把新章节视口设为顶部。
            // 旧章节的 contentY 不应继承到新章节，否则动画层会画到错误 Y 坐标。
            // contentY -> scroll_y 绑定会同步给 Rust，确保两层使用同一滚动坐标。
            if (editorScroll.contentItem) {
                editorScroll.contentItem.contentY = 0;
            }
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
            sujianEditor.snap_next_cursor_update();
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

    color: dt.bg

    // Issue #825 复核第3项：Core 的 plan 明确是「最上面一整条 Toolbar，下面才是
    // ChapterNavigation | Editor | ToolPane | ToolRail」。这里由 WritingWorkbenchToolbar
    // 把这个结构落成真实的 QML 树：SplitView 整体只占内容区，不再自己从窗口顶端开始。
    WritingWorkbenchToolbar {
        id: workbenchToolbar
        dt: root.dt
        anchors.fill: parent

        // Core 的 ToolbarLeading / ToolbarCenter / ToolbarTrailing bounds
        // （-1 = Core 没给，按内容自适应）
        toolbarHeight: root.toolbarHeight
        leadingWidth: root.toolbarLeadingWidth
        centerWidth: root.toolbarCenterWidth
        trailingWidth: root.toolbarTrailingWidth

        appState: root.appState

        // Center 组（WritingFormatGroup）入参
        currentFontSize: settingsBackend ? settingsBackend.setting_font_size : 16
        currentLineSpacing: settingsBackend ? settingsBackend.setting_line_spacing : 1.5
        firstLineIndent: settingsBackend ? settingsBackend.setting_auto_indent_enabled : false
        saveStatus: editorController.saveStatus

        onBackRequested: root.backToProjects()
        // 撤销 / 重做走 SujianEditorItem 的真实实现，和键盘 Ctrl+Z/Ctrl+Y 同一条路径。
        onUndoRequested: sujianEditor.undo()
        onRedoRequested: sujianEditor.redo()
        onFontSizeChanged: function(size) {
            if (settingsBackend) {
                settingsBackend.setting_font_size = size;
                settingsBackend.debounced_save_local_settings();
            }
        }
        onLineSpacingChanged: function(spacing) {
            if (settingsBackend) {
                settingsBackend.setting_line_spacing = spacing;
                settingsBackend.debounced_save_local_settings();
            }
        }
        onFirstLineIndentToggled: {
            if (settingsBackend) {
                settingsBackend.setting_auto_indent_enabled = !settingsBackend.setting_auto_indent_enabled;
                settingsBackend.debounced_save_local_settings();
            }
        }
        onFormatOneClick: editorController.formatText()
        onRequestSync: root.requestSync()
        onRequestSearch: root.requestSearch()
        onOpenSettings: root.openSettings()

        // Issue #833：Workbench 内容区改用 RowLayout，子项只读 workbenchPlan
        // 的 role bounds，通过 Layout.preferredWidth / minimumWidth / maximumWidth
        // 落尺寸。不再用 SplitView 做固定角色排版，也不再回写用户设置项宽度。
        // 窗口尺寸 -> Core layout plan -> QML Layout，一条链到底。
        RowLayout {
            anchors.fill: parent
            spacing: 0

            // Left sidebar: volume/chapter tree
            // Issue #833 复核：章节导航内容抽成 WritingChapterNavigation 组件，
            // Workbench 下作为 RowLayout 子项，宽度只吃 Core 的 ChapterNavigation bounds。
            // SinglePane 下章节导航作为覆盖浮层打开（见文件末尾 singlePaneNavOverlay），
            // 不参与 RowLayout 宽度分配。
            WritingChapterNavigation {
                id: sidebarRect
                // Issue #833 复核3：Workbench 下才参与 RowLayout，
                // SinglePane 下章节导航走 singlePaneNavOverlay 浮层，不占宽度。
                visible: !root.singlePaneMode && !root.leftPaneCollapsed
                // Issue #833：章节导航宽度只取 Core 的 ChapterNavigation bounds，
                // 不再读 setting_desktop_sidebar_width，也不再回写。
                Layout.preferredWidth: root.chapterNavWidth
                Layout.minimumWidth: root.chapterNavWidth
                Layout.maximumWidth: root.chapterNavWidth
                // Issue #835：内容区 RowLayout cross-axis 高度填满，
                // 不再靠子组件内容溢出父项来显示，MouseArea 命中区与视觉一致。
                Layout.fillHeight: true

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
                // Issue #833：Editor 角色宽度只取 Core 的 Editor bounds。
                // SinglePane（hideContentPanes）或非 coreSized 时 fillWidth 占满。
                Layout.fillWidth: root.hideContentPanes || !root.coreSized
                Layout.preferredWidth: root.editorWidth > 0 ? root.editorWidth : -1
                Layout.minimumWidth: root.editorWidth > 0 ? root.editorWidth : 0
                Layout.maximumWidth: root.editorWidth > 0 ? root.editorWidth : -1
                // Issue #835：内容区 RowLayout cross-axis 高度填满。
                Layout.fillHeight: true
                spacing: 0

                // Editor Container Area
                Rectangle {
                    id: editorAreaRect
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    color: dt.bg

                    // Issue #829：正文区顶部的折叠钮（手稿「宽屏全打开」/「左右缩回」
                    // 都在正文区顶部居中画了一个三角）。它是浮层，不占 ColumnLayout 的
                    // 行高，所以正文纸面高度不受影响；左树收起 / 展开时按钮原地切换
                    // ⌄ / ⌃。真正的「收起后重新拉开」仍由贴边把手 leftPaneHandle 负责。
                    PaneFoldButton {
                        anchors.horizontalCenter: parent.horizontalCenter
                        anchors.top: parent.top
                        anchors.topMargin: dt.sp4
                        z: 30
                        dt: root.dt
                        // Issue #833 复核3：任何模式下都给用户入口切换章节导航，
                        // 不再随 hideContentPanes 隐藏（wideWorkbench 但 Core 退回
                        // SinglePane 时 hideContentPanes=true，按钮原本直接消失）。
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
                        anchors.leftMargin: dt.sp8
                        anchors.rightMargin: dt.sp8
                        anchors.topMargin: dt.sp8
                        anchors.bottomMargin: dt.sp8

                        // Paper background - adapts to available space up to contentMaxWidthVp from LayoutPlan
                        // 关于 LayoutPlan：contentMaxWidthVp 控制 paperBg 最大宽度，
                        // 编辑区组件（SujianEditorItem）跟随 paperBg 宽度
                        // 编辑器交互由 EditorController + SujianEditorItem 直接管理
                        Rectangle {
                            id: paperBg
                            // Issue #833 复核：不再有 480 的平台端最小值。
                            // Core 的 editor_min_width_dp 明确是 240，Workbench 在空间紧张时
                            // 允许 Editor 只有 240 起步；SinglePane 的 contentMaxWidthVp
                            // 明确是 0 表示不要额外限宽。Math.max(480) 会导致 Core 给 Editor
                            // 240～479 时 QML 仍画 480，正文纸面溢出 Editor role。
                            // coreSized（Workbench 且 Core 给出 Editor bounds）时直接跟随
                            // parent.width；否则只夹 contentMaxWidthVp（>0 才生效）。
                            width: root.coreSized
                                   ? parent.width
                                   : Math.min(parent.width,
                                              (root.layoutPlan && root.layoutPlan.contentMaxWidthVp > 0)
                                                  ? root.layoutPlan.contentMaxWidthVp
                                                  : parent.width)
                            height: parent.height
                            anchors.horizontalCenter: parent.horizontalCenter
                            color: dt.editorBackground
                            radius: dt.radiusMd
                            border.color: dt.border
                            border.width: 1
                        }

                        // Issue #833：删除 paperBg 左右两个拖拽改宽 MouseArea。
                        // 这两个把旧 setting_desktop_editor_width 写回去，正好会和
                        // Core role width 打架。Editor role 宽度已由 Core 给出，
                        // paperBg 在 Editor role 内直接跟随可用宽度。

                        ScrollView {
                            id: editorScroll
                            // Issue #695 评论 5693346400: editorIsScrolling 把
                            // desktopWheelHandler.active 算进去，这样直接修改 contentY
                            // 时现有的滚动期间动画暂停逻辑仍然有效。
                            readonly property bool editorIsScrolling: ScrollBar.vertical.active || desktopWheelHandler.active || (contentItem && ((contentItem.moving !== undefined && contentItem.moving) || (contentItem.flicking !== undefined && contentItem.flicking)))
                            property bool editorAnimationSuppressed: false
                            anchors.fill: paperBg
                            anchors.margins: dt.sp20
                            clip: true
                            contentWidth: availableWidth
                            contentHeight: Math.max(sujianEditor.content_height, editorCanvas.emptyContentMinimumHeight)

                            function clampScroll() {
                                if (contentItem) {
                                    var maxScroll = Math.max(0, contentHeight - height);
                                    // Only clamp if contentY exceeds the valid range.
                                    // Do NOT force contentY to 0 when contentHeight is still updating.
                                    if (maxScroll > 0 && contentItem.contentY > maxScroll) {
                                        contentItem.contentY = maxScroll;
                                    }
                                }
                            }

                            // Issue #724 评论 5751268664 缺口2: 光标自动跟随滚动。
                            // 语义同 QPlainTextEdit::ensureCursorVisible()（centerOnScroll=false）：
                            // 只滚刚好够让 caret 回到可视区，不每打一字就强制居中。
                            // cursor_rect_y 是目标 caret 的 viewport 坐标（Rust 已减过 scroll_y），
                            // 直接用它做最小滚动量。contentY 改后仍通过 scroll_y 绑定回 Rust，
                            // Scene Graph 和 IME 继续使用同一滚动位置。
                            // Issue #727 评论 5757225958 问题2: 删除对已删除 Rust 方法
                            // set_auto_follow_anchor_with_target 的调用，也删除
                            // visual_cursor_rect_y / visual_cursor_rect_height 这套只为
                            // anchor 服务的旧接口。ensureCursorVisible() 只根据 viewport
                            // cursor_rect_y 算 targetY，最后只做 flick.contentY = targetY。

                            function ensureCursorVisible() {
                                const flick = contentItem
                                if (!flick)
                                    return

                                const margin = Math.max(dt.sp12, sujianEditor.cursor_rect_height * 0.5)
                                const top = sujianEditor.cursor_rect_y
                                const bottom = top + sujianEditor.cursor_rect_height
                                const visibleBottom = availableHeight - margin

                                let nextY = flick.contentY
                                if (top < margin) {
                                    nextY += top - margin
                                } else if (bottom > visibleBottom) {
                                    nextY += bottom - visibleBottom
                                } else {
                                    return
                                }

                                const maxY = Math.max(0, contentHeight - height)
                                // Issue #727 评论 5757225958 问题2: 只算出完整目标 targetY，
                                // 若与当前 contentY 差距 < 0.5 直接返回（无需滚动）。
                                // 否则直接赋值 flick.contentY，不再调用已删除的
                                // set_auto_follow_anchor_with_target。
                                const targetY = Math.max(0, Math.min(maxY, nextY))
                                if (Math.abs(targetY - flick.contentY) < 0.5)
                                    return
                                flick.contentY = targetY
                            }

                            function scheduleEnsureCursorVisible() {
                                Qt.callLater(ensureCursorVisible)
                            }

                            onContentHeightChanged: clampScroll()
                            onHeightChanged: clampScroll()
                            Component.onCompleted: {
                                // Issue #695: Qt 6.9+ 建议桌面 Flickable 设 acceptedButtons: Qt.NoButton，
                                // 鼠标按住拖动不当触屏甩动；触屏 flick 不受 mouse-button 限制。
                                // 让 Qt 原生处理 wheel/scrollbar，ScrollView/Flickable 继续持有 contentY。
                                if (contentItem) {
                                    contentItem.acceptedButtons = Qt.NoButton;
                                }
                            }
                            onEditorIsScrollingChanged: {
                                // Issue #724 评论 5752140048 问题 2: 滚动停止时恢复 false，
                                // 触发 Rust set_is_scrolling(false) → resume_all()。
                                // anchor 生命周期和 editorAnimationSuppressed 是两件事，
                                // 不要为了删 anchor timer 顺手把滚动动画抑制状态恢复也删掉。
                                editorAnimationSuppressed = editorIsScrolling
                            }

                            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                            ScrollBar.vertical: ScrollBar {
                                policy: ScrollBar.AsNeeded
                                parent: editorScroll
                                anchors.top: editorScroll.top
                                anchors.bottom: editorScroll.bottom
                                anchors.right: editorScroll.right
                            }

                            Item {
                                id: editorCanvas
                                readonly property real emptyContentMinimumHeight: Math.max((settingsBackend ? settingsBackend.setting_font_size : 16) * 2.4 + dt.sp16 * 2, editorScroll.availableHeight)
                                width: editorScroll.availableWidth
                                height: editorScroll.availableHeight
                                implicitHeight: Math.max(sujianEditor.content_height, emptyContentMinimumHeight)

                                // NOTE: SujianEditorItem is a "viewport renderer" - it must be
                                // a FIXED overlay on paperBg, NOT inside the Flickable contentItem.
                                // The Flickable only holds a transparent spacer for scrollbar / contentHeight.
                                // scroll_y is passed to the Rust renderer for viewport clipping.
                            }
                        }

                        // SujianEditorItem: viewport renderer - fixed overlay on paperBg,
                        // NOT inside Flickable. scroll_y passes contentY to Rust renderer
                        // for viewport clipping. Flickable only holds a transparent spacer
                        // for scrollbar / contentHeight.
                        //
                        // 关于渲染：LayoutPlan 只控制布局策略（如最大宽度和padding），
                        // 不控制 SujianEditorItem 的渲染细节，渲染由 QML layout 自动处理
                        //
                        // 具体分工：
                        //   - LayoutPlan 策略（contentMaxWidthVp, shellMode, contentPaddingVp）
                        //     通过 paperBg 宽度和 SujianEditorItem 的 Q_PROPERTY 传递
                        //   - 编辑器参数（font_pixel_size, line_spacing, text_indent,
                        //     cursor_color, scroll_y 等）由 settingsBackend 和 EditorController
                        //     直接管理，不走 LayoutPlan
                        //   - 编辑区尺寸跟随 paperBg 宽度和 QML layout 自动调整
                        //     不需要 SujianEditorItem 的 geometry_changed 依赖 LayoutPlan
                        //   - updatePaintNode / QSG 渲染完全由 Rust 侧管理
                        // Issue #677 评论 5653315696 约束:
                        //   - 写作区只拿 DesignTokens 已经算好的最终颜色（editorText、
                        //     textPrimary、primary、selectedText 等），不在写作区里
                        //     解释主题 JSON。主题 JSON 的 snake_case key 解析只在
                        //     DesignTokens.qml 里完成。
                        //   - text_color 最终来自 dt.editorText → dt.textPrimary →
                        //     on_surface（Core DTO snake_case 字段）。
                        //   - QML 的 loading/scrolling 状态（is_loading、is_scrolling）
                        //     只用于动画抑制等，不能决定 Scene Graph 根节点和正文层是否
                        //     存在。visible: true 固定不受 loading/scrolling 影响；
                        //     update_paint_node 通过 ensure_editor_root 在第一帧创建根
                        //     节点，不再因 root 为空整帧跳过。
                        SujianEditorItem {
                            id: sujianEditor
                            // Issue #693 评论 5689819383: SujianEditorItem 是 ScrollView
                            // 外的固定 overlay，editorScroll.clip 裁不到这个 sibling。
                            // 自定义 Scene Graph 默认不裁剪，会画到 item 边界外盖住顶栏。
                            // 打开 clip 把自身绘制和子节点限制在 bounding rect 内。
                            // 参考 https://doc.qt.io/qt-6/qquickitem.html#clip-prop
                            clip: true
                            // Issue #833：SujianEditorItem 和 editorScroll 是兄弟项，
                            // 直接 anchors.fill: editorScroll。不再复制 editorScroll.x/y/
                            // availableWidth/availableHeight，避免 ScrollView 重排时出现
                            // 一帧 x/y/availableWidth 已变、overlay 还没跟上的裁剪错位。
                            anchors.fill: editorScroll
                            visible: true
                            focus: true
                            editor_enabled: editorController.chapterId !== ""
                            font_pixel_size: settingsBackend ? settingsBackend.setting_font_size : 16
                            font_family: "serif"
                            line_spacing: settingsBackend ? settingsBackend.setting_line_spacing : 1.5
                            text_indent: (settingsBackend && settingsBackend.setting_auto_indent_enabled)
                                ? Math.max(0, (settingsBackend.setting_font_size || 16) * settingsBackend.setting_auto_indent_width)
                                : 0
                            padding: dt.sp16
                            // Issue #710 评论 5731145076: text_color 只绑定 dt.editorText，
                            // 不再有 fallback 到 dt.textPrimaryHex 的第二套逻辑（已删除）。
                            // editorText = textPrimary = onSurface（DesignTokens 单一事实源），
                            // 主题变化只触发正文节点颜色重建，不重新创建另一份编辑器主题状态。
                            // Issue #736 评论 5777408243 问题2: 直接绑定 DesignTokens 的 hex 字符串属性，
                            // 不再经 QML color → toString() 转换，避免 QString → QML color → toString() → QString 绕一圈。
                            // 写作区和普通 QML 控件共用同一份最终 color token。
                            text_color: dt.editorTextHex
                            selection_color: dt.primaryHex
                            selected_text_color: dt.selectedTextHex
                            cursor_color: dt.primaryHex
                            smooth_cursor_enabled: settingsBackend ? settingsBackend.setting_smooth_cursor_enabled : true
                            // Issue #785 / Issue #826 评论 38：纯光标移动（点击/方向键）
                            // 的 duration 始终独立，用 smooth cursor duration；
                            // 协同模式的正文编辑 caret 不走这个时长，跟着 typing
                            // duration 跑（与前沿同一时钟），两者不再是两条独立时间线。
                            cursor_animation_duration_ms: settingsBackend ? settingsBackend.setting_smooth_cursor_duration_ms : 80
                            typing_animation_enabled: settingsBackend ? settingsBackend.setting_typing_animation_enabled : true
                            typing_animation_duration_ms: settingsBackend ? settingsBackend.setting_typing_animation_duration_ms : 100
                            // Issue #853：正文过渡从最近成功绘制的 VisualFrame
                            // 更新到最新 canonical layout；光标由 CursorController
                            // 独立管理。点击移动光标不会结束正在播放的正文过渡。
                            coordinated_animation_enabled: settingsBackend ? settingsBackend.setting_coordinated_text_cursor_animation_enabled : true
                            scroll_y: editorScroll.contentItem ? editorScroll.contentItem.contentY : 0
                            viewport_height: sujianEditor.height
                            is_scrolling: editorScroll.editorAnimationSuppressed
                            is_loading: editorController.isLoadingChapter
                            is_applying_format: editorController.isApplyingFormat
                            // Issue #721: 不再把保存 guard (settingsSaveGuardActive) 传成
                            // 编辑器视觉抑制状态。主题切换不再触发 applyCurrentSettings()，
                            // 正文/选区/光标颜色已通过 text_color/selection_color/
                            // selected_text_color/cursor_color 绑定自动下发，Rust 侧各自
                            // 走 color_only_changed() -> request_scene_rebuild()，不会清空动画。

                            // Issue #693 评论 5689819383: 光标自动跟随滚动。
                            // 只在真正的编辑/selection 变化时调度，不监听 scroll_y 或
                            // 每次 cursor_rect_changed，避免用户手动滚离光标被立刻拽回。
                            // Rust 侧先发信号再 update_cursor_visual_position()，故用
                            // Qt.callLater() 等本轮 caret target 算完再读 cursor_rect_y。
                            // 同一轮即使排了两次 callLater，第二次看到 caret 已可见会直接返回。
                            onCursor_position_changed: editorScroll.scheduleEnsureCursorVisible()
                            onText_changed: editorScroll.scheduleEnsureCursorVisible()

                            onWidthChanged: {
                                Qt.callLater(sujianEditor.flush_content_height)
                            }

                            Component.onCompleted: {
                                sujianEditor.verify_animation_signal_meta_object()
                            }

                            onExplicit_clear_requested: editorController.markPotentialExplicitClear()

                            onContext_menu_requested: function(cx, cy) {
                                // 将局部坐标映射为全局坐标后弹出菜单
                                var globalPos = sujianEditor.mapToGlobal(cx, cy)
                                editorContextMenu.popup(globalPos.x, globalPos.y)
                            }

                            TapHandler {
                                acceptedButtons: Qt.RightButton
                                onTapped: function(eventPoint) {
                                    sujianEditor.click_at(eventPoint.position.x, eventPoint.position.y, false)
                                    editorContextMenu.popup()
                                }
                            }

                            // Issue #819 评论 5967250411 问题 4：左键长按选词改用
                            // Rust property 驱动的 Timer，不再用 TapHandler 接管 pointer event。
                            //
                            // 设计：
                            // - qquickitem_impl mouse_event 是左键 pointer event 的唯一 owner。
                            //   左键 Press 时 Rust 设 long_press_timer_active = true + 记录 x/y；
                            //   Release / Move 超阈值时设 false。
                            // - QML Timer.running 绑定 sujianEditor.long_press_timer_active，
                            //   到点时调 sujianEditor.activate_pointer_long_press(x, y)。
                            // - Timer 不接管 pointer grab，不处理 MouseMove/Release。
                            // - release/cancel 的 selection gesture 结束只由 qquickitem_impl
                            //   mouse_event 做一次，不再从 QML 结束选择手势。
                            // - 左键长按只负责选择，不弹菜单（菜单只由右键 TapHandler 触发）。
                            //
                            // 旧 leftButtonLongPressHandler (TapHandler) 已删除：它和
                            // qquickitem_impl mouse_event 双 owner，且 TapHandler onPressedChanged
                            // / onCanceled 结束选择手势与 mouse_event release
                            // 重复结束手势。Issue #815 评论 6042062633 修改 1 恢复的鼠标左键
                            // 长按选词语义保留（Timer 到点调 activate_pointer_long_press）。
                            Timer {
                                id: leftButtonLongPressTimer
                                // Timer.running 绑定 Rust property，由 mouse_event 控制启停。
                                running: sujianEditor.long_press_timer_active
                                interval: 800
                                repeat: false
                                // Timer 到点时调 activate_pointer_long_press，
                                // 不接管 pointer grab，不处理 MouseMove/Release。
                                onTriggered: {
                                    sujianEditor.activate_pointer_long_press(
                                        sujianEditor.long_press_pending_x,
                                        sujianEditor.long_press_pending_y)
                                }
                            }

                            // Cursor is now rendered in SujianEditorItem Scene Graph (child[3])
                        }

                        // 编辑器上下文菜单
                        EditorContextMenu {
                            id: editorContextMenu
                            editorItem: sujianEditor
                            dt: root.dt
                        }

                        // Issue #690 评论 5675007226 步骤 4: 光标闪烁改用低频 Timer，
                        // 不再用 FrameAnimation 每帧回调。正文位置动画已由 RenderPlan 同帧计算，
                        // 不再需要 QML 每帧推进。空闲闪烁只需要低频定时触发 blink，
                        // 并且每次切换显式 request_frame_update()。
                        Timer {
                            id: cursorBlinkTimer
                            interval: 265
                            repeat: true
                            running: sujianEditor.editor_enabled
                                     && sujianEditor.focus
                            onTriggered: {
                                sujianEditor.tick_cursor_animation()
                            }
                        }

                        // Animation overlay removed — text animation is now handled in
                        // SujianEditorItem Scene Graph (child[1]) via ActiveVisualTransactionQueue

                        // 文字动画唯一主路径：Rust Coordinator → Scene Graph (child[1])
                        // 不再使用 QML overlay 路线

                        // Issue #695 评论 5693346400: 桌面滚轮事件直译器
                        // 用 WheelHandler 拦截 wheel 事件并直接修改 contentY，阻止
                        // Qt 6.10 QQuickFlickable::wheelEvent() 自带的 wheel acceleration。
                        // 放在 editorScroll/sujianEditor 之后声明（z 更高），wheel 事件
                        // 先到本组件；不拦截鼠标点击/拖拽（Item 默认不处理鼠标事件）。
                        DesktopWheelScrollHandler {
                            id: desktopWheelHandler
                            targetFlickable: editorScroll.contentItem
                            // 每格滚动距离 = wheelScrollLines × 当前行高
                            // 行高 = 字体大小 × 行距倍数
                            lineSpacingPx: (settingsBackend ? settingsBackend.setting_font_size : 16) * (settingsBackend ? settingsBackend.setting_line_spacing : 1.5)
                            anchors.fill: editorScroll
                        }

                    }

                    // Empty state
                    ColumnLayout {
                        anchors.centerIn: parent
                        spacing: dt.sp12
                        visible: !editorController.chapterId

                        Rectangle {
                            width: 32; height: 32
                            radius: 16
                            color: dt.textSecondary
                            opacity: 0.1
                            Layout.alignment: Qt.AlignHCenter
                        }
                        AppText {
                            dt: root.dt
                            text: qsTr("请选择或新建章节")
                            color: dt.textSecondary
                            font.pointSize: dt.fontLgPt
                            Layout.alignment: Qt.AlignHCenter
                        }
                    }

                    // Right drawer button (when closed).
                    // 宽屏 Workbench 由最右竖向 WritingToolRail + 贴边悬浮把手接管收起/展开，
                    // 这里不再重复画一个，避免同一入口出现两份。窄屏保持原来的右侧箭头按钮。
                    Rectangle {
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        width: 36
                        visible: !root.drawerOpen && !root.toolRailVisible && !root.hideContentPanes
                        color: "transparent"

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: dt.sp8

                            Rectangle {
                                width: 28; height: 28
                                radius: dt.radiusPill
                                color: drawerBtnHover.containsMouse ? dt.surfaceVariant : "transparent"

                                AppText {
                                    dt: root.dt
                                    anchors.centerIn: parent
                                    text: "\u25C0" // Left arrow to indicate it opens from the right
                                    color: dt.textMuted
                                    font.pointSize: dt.fontXsPt
                                }

                                MouseArea {
                                    id: drawerBtnHover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.requestToolPaneOpen("stats", true)
                                }
                            }
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

            // Issue #825：工具 pane（Core 的 ToolPane 角色）—— 只显示 rail 选中的那个工具内容。
            RightDrawer {
                id: rightDrawerRect
                // Issue #833：工具 pane 宽度只取 Core 的 ToolPane bounds。
                Layout.preferredWidth: root.toolPaneWidth
                Layout.minimumWidth: root.toolPaneWidth
                Layout.maximumWidth: root.toolPaneWidth
                // Issue #835：内容区 RowLayout cross-axis 高度填满。
                Layout.fillHeight: true
                // Issue #825 复核5第1项：Core 最终判 SinglePane 时工具 pane 不存在。
                visible: root.drawerOpen && !root.hideContentPanes
                dt: root.dt
                editorBackendRef: root.editorBackendRef
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

            // Issue #825：最右竖向工具 rail（Core 的 ToolRail 角色）。
            // 只负责「选哪个工具 + 展开/收起 ToolPane」，工具内容在右边的 ToolPane 里。
            // 顶部工具条带 ToolbarTrailing 的同步/搜索/设置不搬到这里。
            WritingToolRail {
                id: toolRailRect
                visible: root.toolRailVisible
                // Issue #833：工具 rail 宽度只取 Core 的 ToolRail bounds。
                Layout.preferredWidth: root.toolRailWidth
                Layout.minimumWidth: root.toolRailWidth
                Layout.maximumWidth: root.toolRailWidth
                // Issue #835：内容区 RowLayout cross-axis 高度填满。
                Layout.fillHeight: true
                dt: root.dt
                hasConflicts: root.hasConflicts
                selectedTool: root.drawerTool
                toolPaneOpen: root.drawerOpen
                // Issue #825 复核4 第2项：rail 内部的按钮宽度也吃 Core 的 ToolRail bounds，
                // 不能容器用 Core 宽度、组件内部还按默认 56 画。
                railWidth: root.coreSized ? root.toolRailWidth : 56
                onToolRequested: function(toolKey) { root.requestToolPaneOpen(toolKey, true); }
                onToolPaneToggled: {
                    root.toggleToolPane();
                    if (root.drawerOpen) root.requestEditorFocus();
                }
            }
        }

        // Issue #842 评论修复（问题1）：左 resize separator 改为 overlay，
        // 不再作为 RowLayout 子项参与宽度分配。Core 的四角色（ChapterNavigation +
        // Editor + ToolPane + ToolRail）已占满 placement region，separator 只盖在
        // 章节栏右边界上做命中与拖拽，z 提高确保盖在内容之上。可视线 1vp（居中
        // Rectangle），命中 Item 12vp。x 跟随 sidebarRect 右边界（中心对齐边界）。
        // 垂直范围锚到 parent（contentSlot）顶/底，与 RowLayout 同高。
        Item {
            id: leftResizeSeparator
            visible: !root.singlePaneMode && !root.leftPaneCollapsed && root.coreSized
            width: 12
            x: sidebarRect.x + sidebarRect.width - width / 2
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            z: 50

            Rectangle {
                anchors.centerIn: parent
                width: 1
                height: parent.height
                color: root.dt.border
            }

            DragHandler {
                target: null
                cursorShape: Qt.SplitHCursor
                onActiveChanged: {
                    if (active) {
                        // 每次手势从 Core 当前实际宽度起算，避免旧 requested 与实际宽度
                        // 不一致造成死区，或 requested<=0 触发 Core"恢复默认宽度"语义。
                        root._dragStartChapterNavWidth = root.chapterNavWidth
                    }
                }
                onTranslationChanged: {
                    if (active) {
                        // Math.max(1,...) 只防误触 Core 的"<=0 表示默认宽度"哨兵；
                        // 实际最小值仍由 Core list_pane_min_width_dp clamp。
                        // activeTranslation 每次手势从 0 重置（translation 已 deprecated）。
                        root.requestedChapterNavWidth =
                            Math.max(1, root._dragStartChapterNavWidth + activeTranslation.x)
                        root.refreshWorkbenchPlan()
                    }
                }
            }
        }

        // Issue #842 评论修复（问题1）：右 resize separator 改为 overlay。
        // x 跟随 rightDrawerRect 左边界（Editor 右边界），中心对齐边界。
        Item {
            id: rightResizeSeparator
            visible: root.drawerOpen && !root.hideContentPanes && root.coreSized
            width: 12
            x: rightDrawerRect.x - width / 2
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            z: 50

            Rectangle {
                anchors.centerIn: parent
                width: 1
                height: parent.height
                color: root.dt.border
            }

            DragHandler {
                target: null
                cursorShape: Qt.SplitHCursor
                onActiveChanged: {
                    if (active) {
                        // 每次手势从 Core 当前实际宽度起算，避免旧 requested 与实际宽度
                        // 不一致造成死区，或 requested<=0 触发 Core"恢复默认宽度"语义。
                        root._dragStartToolPaneWidth = root.toolPaneWidth
                    }
                }
                onTranslationChanged: {
                    if (active) {
                        // Math.max(1,...) 只防误触 Core 的"<=0 表示默认宽度"哨兵；
                        // 实际最小值仍由 Core tool_pane_min_width_dp clamp。
                        // activeTranslation 每次手势从 0 重置（translation 已 deprecated）。
                        root.requestedToolPaneWidth =
                            Math.max(1, root._dragStartToolPaneWidth - activeTranslation.x)
                        root.refreshWorkbenchPlan()
                    }
                }
            }
        }
    }

    Connections {
        target: settingsBackend
        function onSettings_changed() {
            editorController.applyCurrentSettings();
        }
    }

    // Issue #721: 删除 onIsDarkChanged -> applyCurrentSettings() 连接。
    // 主题切换不需要进入 applyCurrentSettings()——正文/选区/光标颜色已通过
    // text_color/selection_color/selected_text_color/cursor_color 绑定自动下发，
    // Rust 侧各自走 color_only_changed() -> request_scene_rebuild()，不会清空动画。
    // 上面 settingsBackend.onSettings_changed 仍保留，它只保护保存，不控制编辑器动画。

    Connections {
        target: editorController
        function onChapterIdChanged() {
            if (editorController.chapterId) {
                root.requestEditorFocus();
                // 保存导航状态（包含当前章节信息）
                if (workspaceBackend) {
                    workspaceBackend.save_last_navigation_state(
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
        var raw = sb.list_sync_conflicts(root.workspaceProjectId);
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
        width: 18
        height: 64
        radius: dt.radiusPill
        color: leftPaneHandleHover.containsMouse || leftPaneHandleHover.pressed ? dt.surfaceVariant : dt.surface
        border.color: leftPaneHandleHover.containsMouse ? dt.borderFocus : dt.border
        border.width: 1
        x: root.leftPaneCollapsed
           ? dt.sp4
           : Math.max(0, Math.min(root.width - width - dt.sp4, sidebarRect.width - width / 2))
        y: Math.round((root.height - height) / 2)

        Behavior on color { ColorAnimation { duration: dt.animFast } }
        Behavior on border.color { ColorAnimation { duration: dt.animFast } }

        AppText {
            dt: root.dt
            anchors.centerIn: parent
            text: root.leftPaneCollapsed ? "›" : "‹"
            color: dt.textSecondary
            font.pointSize: dt.fontMdPt
        }

        MouseArea {
            id: leftPaneHandleHover
            anchors.fill: parent
            anchors.margins: -dt.sp6
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
                if (root.leftPaneCollapsed) {
                    root.requestChapterNavigationOpen();
                } else {
                    root.closeChapterNavigation();
                }
                // 收起后把手贴到左边缘，留出 sp4 边距；不需要动编辑会话。
                if (root.leftPaneCollapsed) root.requestEditorFocus()
            }
        }
    }

    Rectangle {
        id: rightPaneHandle
        visible: root.toolRailVisible
        z: 50
        width: 18
        height: 64
        radius: dt.radiusPill
        color: rightPaneHandleHover.containsMouse || rightPaneHandleHover.pressed ? dt.surfaceVariant : dt.surface
        border.color: rightPaneHandleHover.containsMouse ? dt.borderFocus : dt.border
        border.width: 1
        Behavior on color { ColorAnimation { duration: dt.animFast } }
        Behavior on border.color { ColorAnimation { duration: dt.animFast } }

        // Issue #825 复核第5项：右把手贴在“正文 | 工具区”分隔边缘上（RightDrawer 展开时
        // 用它的左边缘），只有收起时才退回 ToolRail 左边缘。
        // 工具 pane 收起后 rail 仍然常驻（Core 的 ToolRail 恒有宽度），
        // 所以收起态贴的是 rail 左缘，不是窗口最右缘。
        x: root.drawerOpen
           ? Math.max(0, Math.min(root.width - width - dt.sp4, rightDrawerRect.x - width / 2))
           : Math.max(0, Math.min(root.width - width - dt.sp4, toolRailRect.x - width / 2))
        y: Math.round((root.height - height) / 2)

        AppText {
            dt: root.dt
            anchors.centerIn: parent
            text: root.drawerOpen ? "›" : "‹"
            color: dt.textSecondary
            font.pointSize: dt.fontMdPt
        }

        MouseArea {
            id: rightPaneHandleHover
            anchors.fill: parent
            anchors.margins: -dt.sp6
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
                root.toggleToolPane()
                if (!root.drawerOpen) root.requestEditorFocus()
            }
        }
    }

    // Issue #833 复核：SinglePane 章节导航浮层。
    // SinglePane（非 Workbench 或 Core 退回单栏）下章节导航不参与 RowLayout 宽度分配，
    // 作为覆盖在正文上的抽屉打开。点遮罩或选章后自动关闭。
    // Workbench 下 singlePaneNavOpen 恒为 false，这整块不可见。
    Item {
        id: singlePaneNavOverlay
        anchors.fill: parent
        // Issue #833 复核3：只在 SinglePane 模式且浮层打开时可见，
        // Workbench 下恒不可见，避免两套壳互相覆盖。
        visible: root.singlePaneMode && root.singlePaneNavOpen
        z: 60

        // 半透明遮罩，点击关闭抽屉。
        // 颜色取 DesignTokens 的 scrim，不再写死 "black"（ui_tokens 门禁）。
        Rectangle {
            anchors.fill: parent
            color: dt.scrim
            opacity: 0.4
            MouseArea {
                anchors.fill: parent
                onClicked: root.closeChapterNavigation()
            }
        }

        // 左侧抽屉：复用 WritingChapterNavigation，宽度取可用宽度与 Core listPaneWidthVp 的较小值。
        // Issue #833 第四轮复核：宽度来自 Core LayoutMetrics.list_pane_width_dp（经 DTO 透传），
        // QML 不再自己写第二个 320——SinglePane 正文占满，抽屉只是临时覆盖。
        WritingChapterNavigation {
            id: singlePaneNavPanel
            width: Math.min(
                parent.width,
                root.layoutPlan && root.layoutPlan.listPaneWidthVp > 0
                    ? root.layoutPlan.listPaneWidthVp
                    : parent.width
            )
            height: parent.height
            x: 0
            y: 0
            dt: root.dt
            tree: root.tree
            workspaceProjectId: root.workspaceProjectId
            workspaceProjectTitle: root.workspaceProjectTitle
            currentChapterId: editorController.chapterId

            onOpenChapter: function(pId, vId, cId, cTitle) {
                root.openChapter(pId, vId, cId, cTitle)
                root.closeChapterNavigation()
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
    }
}
