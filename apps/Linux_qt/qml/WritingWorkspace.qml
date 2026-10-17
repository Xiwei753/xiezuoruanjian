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
// 关于布局（Issue #920）
//
// 布局完全由 Qt 平台层自己决定：本组件的 RowLayout + 本地断点 + 本地尺寸常量。
// Core 不再接收窗口尺寸，也不返回布局模式 / 栏宽 / bounds / 角色矩形。
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
// 组成（按七个产品语义角色组织）：
//   WritingWorkbenchToolbar：上方一条贯通工具条带
//     （ToolbarLeading 返回/撤销/重做 | ToolbarCenter 字号/行距/段落/排版 |
//       ToolbarTrailing 同步/搜索/设置）
//   下方内容区 RowLayout：
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
    property var appState: ({})
    // Issue #835：星图后端透传给 RightDrawer（任务 4 已为 RightDrawer 加 starmapBackendRef 属性）。
    // 由 main.qml 注入，WritingWorkspace 只做透传，不在本地伪造星图业务状态。
    property var starmapBackendRef: null
    // Issue #835 评论 6019713847: StarMapPage 需要真实 StarMapController 读根星图，
    // 由 main.qml 注入 globalStarMapController，透传给 RightDrawer.starMapControllerRef。
    property var starMapControllerRef: null
    property var tree: []
    // 左右 pane 的展开状态只是端侧 UI 状态（不进 Core、不进同步）。
    // 工具 pane 用「选中的工具 key」表达："" = 收起，非空 = 展开并显示该工具。
    // Issue #829：手稿里的常驻入口是 starmap / ai；冲突只作为动态附加工具。
    // 不再保留 drawerOpen + drawerTab 两份状态，避免第二套开关。
    // Issue #842 任务3：drawerTool 只表示当前工具选择，不再同时表达折叠状态。
    // rightPaneCollapsed 独立控制右栏折叠；drawerOpen 由 rightPaneCollapsed 派生。
    property string drawerTool: "starmap"
    property bool rightPaneCollapsed: true
    readonly property bool drawerOpen: !root.rightPaneCollapsed
    property bool leftPaneCollapsed: false
    // 拖拽起始宽度记录（内部属性）
    property real _dragStartChapterNavWidth: 0
    property real _dragStartToolPaneWidth: 0
    // Issue #833 复核：SinglePane 下章节导航作为覆盖在正文上的浮层打开，
    // 不参与 RowLayout 宽度分配；多栏工作台下章节导航由 leftPaneCollapsed 管理，
    // 这个属性只在 SinglePane 下生效。
    property bool singlePaneNavOpen: false

    // ── Issue #920：Qt 平台本地布局断点与尺寸 ──
    // 布局完全由 Qt 自己决定（Qt Quick Layouts + 本地断点），不再让 Core 收窗口
    // 尺寸 / 返回布局模式、栏宽、bounds。数值沿用旧 Core LayoutMetrics，断点沿用
    // 旧窗口宽度分类，保证视觉等价：
    //   width < 600 或 height < 480：单栏；
    //   >= 600 且 >= 480：多栏工作台（当前 pane 组合放不下时退回单栏）。
    readonly property int workbenchMinWidth: 600
    readonly property int workbenchMinHeight: 480
    readonly property int listPaneWidth: 320
    readonly property int listPaneMinWidth: 200
    readonly property int toolPaneMinWidth: 200
    readonly property int toolRailWidth: 56
    readonly property int editorMinWidth: 240
    readonly property int toolbarMinHeight: 64
    readonly property int toolbarLeadingMinWidth: 200
    readonly property int toolbarTrailingMinWidth: 200

    // 用户可拖拽的侧栏 / 工具面板宽度（Qt 本地 UI 状态，多栏工作台内生效）。
    property real chapterNavWidth: 320
    property real toolPaneWidth: 240

    // 一级断点：窗口够宽够高才进入多栏工作台。
    readonly property bool wideWorkbench: root.width >= root.workbenchMinWidth
                                            && root.height >= root.workbenchMinHeight

    // 当前可见 pane 的多栏最小需求宽度。放不下就退回单栏，不把栏压成细线。
    function workbenchFitsFor(chapterVisible, toolVisible) {
        if (!root.wideWorkbench)
            return false
        var needed = (chapterVisible ? root.listPaneMinWidth : 0)
                   + (toolVisible ? root.toolPaneMinWidth : 0)
                   + root.toolRailWidth
                   + root.editorMinWidth
        return root.width >= needed
    }

    // 多栏工作台是否成立（等价于旧 Core 的 workbenchPlan.mode === "Workbench"）。
    readonly property bool multiPane: root.workbenchFitsFor(!root.leftPaneCollapsed, root.drawerOpen)

    // Issue #833 复核3：SinglePane 和 Workbench 是两套壳结构，状态互不污染。
    // singlePaneMode：窗口不够大，或当前 pane 组合放不下。
    readonly property bool singlePaneMode: !root.multiPane
    readonly property bool chapterNavigationShown:
        root.singlePaneMode ? root.singlePaneNavOpen : !root.leftPaneCollapsed
    // 从 SinglePane 切回多栏时清掉浮层状态，避免 overlay 继续盖在工作台上。
    onSinglePaneModeChanged: {
        if (!root.singlePaneMode) {
            root.singlePaneNavOpen = false;
        }
    }

    // 最右工具 rail 与两个折叠把手只在多栏工作台出现；单栏时工具 pane
    // 由正文区里那个箭头按钮收起/展开。
    readonly property bool toolRailVisible: root.multiPane

    // 工具条带三个分组的尺寸。多栏时 leading/trailing 跟随左右栏分割线，
    // 让顶栏分段与内容栏边界对齐；单栏时交给组件按内容自适应（-1）。
    // effective* 与 RowLayout 的压缩口径一致：面板先拿 preferred，空间不足时
    // 压到 min，正文始终保住 editorMinWidth。
    function effectiveChapterNavWidth() {
        return Math.max(root.listPaneMinWidth,
                        Math.min(root.chapterNavWidth, root._chapterNavMaxWidth))
    }
    function effectiveToolPaneWidth() {
        return Math.max(root.toolPaneMinWidth,
                        Math.min(root.toolPaneWidth, root._toolPaneMaxWidth))
    }
    readonly property real toolbarHeight: root.multiPane ? root.toolbarMinHeight : -1
    readonly property real toolbarCenterWidth: -1
    readonly property real toolbarLeadingWidth: root.multiPane
        ? Math.max(root.toolbarLeadingMinWidth,
                   root.leftPaneCollapsed ? 0 : root.effectiveChapterNavWidth())
        : -1
    readonly property real toolbarTrailingWidth: root.multiPane
        ? Math.max(root.toolbarTrailingMinWidth,
                   root.toolRailWidth + (root.drawerOpen ? root.effectiveToolPaneWidth() : 0))
        : -1

    // 单栏时正文纸面的最大宽度（多栏时正文跟随 Editor 列宽度，不额外限宽）。
    // 与旧 Core Workbench 的 contentMaxWidthVp 口径一致：min(840, width - 2*32)。
    readonly property real contentMaxWidthVp: (root.wideWorkbench && root.singlePaneMode)
        ? Math.min(840, Math.max(0, root.width - 64))
        : 0

    // 拖拽时允许的侧栏 / 工具面板宽度上限：始终给正文留 editorMinWidth。
    readonly property real _chapterNavMaxWidth: Math.max(root.listPaneMinWidth,
        root.width - root.toolRailWidth - root.editorMinWidth
            - (root.drawerOpen ? root.toolPaneMinWidth : 0))
    readonly property real _toolPaneMaxWidth: Math.max(root.toolPaneMinWidth,
        root.width - root.toolRailWidth - root.editorMinWidth
            - (root.leftPaneCollapsed ? 0 : root.listPaneMinWidth))

    // Issue #920 复核：恢复用户上次拖拽保存的侧栏宽度。
    // 只恢复左侧章节导航栏（对应旧 LocalSettings.desktop_sidebar_width）；
    // 右侧工具 pane 宽度是另一种 UI 状态，不复用旧 desktop_editor_width。
    function restorePersistedPaneWidths() {
        if (!settingsBackend)
            return
        var stored = settingsBackend.setting_desktop_sidebar_width
        if (stored !== undefined && stored > 0) {
            // 只做下界处理：chapterNavWidth 保存用户的期望宽度，不因当前窗口暂时
            // 变窄而被永久改写；实际显示宽度由 effectiveChapterNavWidth() 在布局
            // 阶段按 _chapterNavMaxWidth 动态钳制，窗口放大后仍能回到期望宽度。
            root.chapterNavWidth = Math.max(root.listPaneMinWidth, stored)
        }
    }

    // 拖拽结束后把章节导航栏宽度写回平台本地配置（不回写 Core 业务设置）。
    function persistChapterNavWidth() {
        if (!settingsBackend)
            return
        settingsBackend.setting_desktop_sidebar_width = root.chapterNavWidth
        settingsBackend.debounced_save_local_settings()
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
        if (root.singlePaneMode) {
            root.singlePaneNavOpen = true;
            return;
        }
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
        // Issue #833 复核3：按 singlePaneMode 分支，SinglePane 只收浮层，
        // 多栏只收 leftPaneCollapsed，两套状态互不污染。
        if (root.singlePaneMode) {
            root.singlePaneNavOpen = false;
        } else {
            root.leftPaneCollapsed = true;
        }
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
            editorViewport.requestEditorFocus();
        });
    }

    // Issue #833 复核：WritingTreeController / treeModel / populateTreeModel
    // 已随 sidebarRect 内容一起搬进 WritingChapterNavigation.qml，这里不再保留。

    EditorController {
        id: editorController
        targetEditorItem: editorViewport.editorItem
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
        onUndoRequested: editorViewport.undo()
        onRedoRequested: editorViewport.redo()
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

        // Issue #920：内容区用 RowLayout，子项通过 Layout.preferredWidth /
        // minimumWidth / maximumWidth 落尺寸，尺寸来自本组件的 Qt 本地常量。
        // 窗口宽度 -> QML 断点 -> Qt Quick Layouts，一条链到底，不经 Core。
        RowLayout {
            anchors.fill: parent
            spacing: 0

            // Left sidebar: volume/chapter tree
            // Issue #833 复核：章节导航内容抽成 WritingChapterNavigation 组件，
            // 多栏工作台下作为 RowLayout 子项，宽度取 Qt 本地 chapterNavWidth。
            // SinglePane 下章节导航作为覆盖浮层打开（见文件末尾 singlePaneNavOverlay），
            // 不参与 RowLayout 宽度分配。
            WritingChapterNavigation {
                id: sidebarRect
                // Issue #833 复核3：多栏工作台下才参与 RowLayout，
                // SinglePane 下章节导航走 singlePaneNavOverlay 浮层，不占宽度。
                visible: !root.singlePaneMode && !root.leftPaneCollapsed
                // Issue #920 复核：章节导航宽度取 Qt 本地 chapterNavWidth（可拖拽，
                // 保存用户期望宽度），显示时由 effectiveChapterNavWidth() 按当前
                // 窗口可用范围动态压缩，保证正文列先拿到 editorMinWidth。
                // Issue #920 复核：preferred/maximum 用 effectiveChapterNavWidth()，
                // 它在布局阶段按 _chapterNavMaxWidth 动态钳制，chapterNavWidth 仍是
                // 用户的期望宽度；避免窄窗口把期望宽度提前钳死。顶栏口径保持一致。
                Layout.preferredWidth: root.effectiveChapterNavWidth()
                Layout.minimumWidth: root.listPaneMinWidth
                Layout.maximumWidth: root.effectiveChapterNavWidth()
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
                // Issue #920：Editor 列始终占满剩余宽度；多栏时用 minimumWidth
                // 保证正文不小于 editorMinWidth，单栏时不再额外限制。
                Layout.fillWidth: true
                Layout.minimumWidth: root.multiPane ? root.editorMinWidth : 0
                // Issue #835：内容区 RowLayout cross-axis 高度填满。
                Layout.fillHeight: true
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

                        // Paper background — Issue #920：单栏时最多夹到 contentMaxWidthVp，
                        // 多栏时正文直接跟随 Editor 列可用宽度，不额外限宽。
                        // 编辑区组件（SujianEditorItem）跟随 paperBg 宽度；
                        // 编辑器交互由 EditorController + SujianEditorItem 直接管理。
                        Rectangle {
                            id: paperBg
                            // 不再有 480 的平台端最小值：多栏时正文跟随 parent.width，
                            // 单栏时只在 contentMaxWidthVp > 0 时夹一下，避免正文溢出。
                            width: root.multiPane
                                   ? parent.width
                                   : Math.min(parent.width,
                                              root.contentMaxWidthVp > 0
                                                  ? root.contentMaxWidthVp
                                                  : parent.width)
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
                            settingsBackendRef: settingsBackend
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

                    // Right drawer button (when closed).
                    // 宽屏 Workbench 由最右竖向 WritingToolRail + 贴边悬浮把手接管收起/展开，
                    // 这里不再重复画一个，避免同一入口出现两份。窄屏保持原来的右侧箭头按钮。
                    Rectangle {
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        width: Kirigami.Units.gridUnit * 2
                        visible: !root.drawerOpen && !root.toolRailVisible && !root.wideWorkbench
                        color: "transparent"

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: Kirigami.Units.smallSpacing

                            ToolButton {
                                Layout.alignment: Qt.AlignHCenter
                                // Left arrow to indicate it opens from the right
                                icon.name: "go-previous"
                                display: AbstractButton.IconOnly
                                flat: true
                                onClicked: root.requestToolPaneOpen("stats", true)
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

            // Issue #825：工具 pane（ToolPane 角色）—— 只显示 rail 选中的那个工具内容。
            RightDrawer {
                id: rightDrawerRect
                // Issue #920：工具 pane 宽度取 Qt 本地 toolPaneWidth（可拖拽），
                // 空间紧张时在 [toolPaneMinWidth, toolPaneWidth] 内压缩。
                Layout.preferredWidth: root.toolPaneWidth
                Layout.minimumWidth: root.toolPaneMinWidth
                Layout.maximumWidth: root.toolPaneWidth
                // Issue #835：内容区 RowLayout cross-axis 高度填满。
                Layout.fillHeight: true
                // Issue #920：只有多栏工作台且工具 pane 展开时才存在。
                visible: root.multiPane && root.drawerOpen
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
                // Issue #920：rail 宽度取 Qt 本地常量（多栏时 56vp，单栏不显示）。
                railWidth: root.multiPane ? root.toolRailWidth : 56
                onToolRequested: function(toolKey) { root.requestToolPaneOpen(toolKey, true); }
                onToolPaneToggled: {
                    root.toggleToolPane();
                    if (root.drawerOpen) root.requestEditorFocus();
                }
            }
        }

        // Issue #842 评论修复（问题1）：左 resize separator 改为 overlay，
        // 不再作为 RowLayout 子项参与宽度分配。四栏（章节导航 + 正文 +
        // 工具 pane + 工具 rail）已占满内容区，separator 只盖在章节栏右边界上
        // 做命中与拖拽，z 提高确保盖在内容之上。可视线 1vp（居中 Rectangle），
        // 命中 Item 12vp。x 跟随 sidebarRect 右边界（中心对齐边界）。
        // 垂直范围锚到 parent（contentSlot）顶/底，与 RowLayout 同高。
        Item {
            id: leftResizeSeparator
            visible: root.multiPane && !root.leftPaneCollapsed
            width: 12
            x: sidebarRect.x + sidebarRect.width - width / 2
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            z: 50

            Rectangle {
                anchors.centerIn: parent
                width: 1
                height: parent.height
                color: Kirigami.Theme.disabledTextColor
                opacity: 0.3
            }

            DragHandler {
                target: null
                cursorShape: Qt.SplitHCursor
                onActiveChanged: {
                    if (active) {
                        // 从当前实际显示宽度起算，避免死区。用 sidebarRect.width
                        // 而非 chapterNavWidth：窄窗口下期望宽度可能大于可显示宽度，
                        // 用期望值起算会出现「拉一段距离侧栏才动」。
                        root._dragStartChapterNavWidth = sidebarRect.width
                    } else {
                        // Issue #920 复核：拖拽结束写回平台本地配置，下次启动恢复。
                        root.persistChapterNavWidth()
                    }
                }
                onTranslationChanged: {
                    if (active) {
                        // Issue #920：本地 clamp 到 [listPaneMinWidth, _chapterNavMaxWidth]，
                        // 始终给正文留 editorMinWidth。activeTranslation 每次手势从 0 重置。
                        root.chapterNavWidth = Math.min(root._chapterNavMaxWidth,
                            Math.max(root.listPaneMinWidth,
                                     root._dragStartChapterNavWidth + activeTranslation.x))
                    }
                }
            }
        }

        // Issue #842 评论修复（问题1）：右 resize separator 改为 overlay。
        // x 跟随 rightDrawerRect 左边界（Editor 右边界），中心对齐边界。
        Item {
            id: rightResizeSeparator
            visible: root.multiPane && root.drawerOpen
            width: 12
            x: rightDrawerRect.x - width / 2
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            z: 50

            Rectangle {
                anchors.centerIn: parent
                width: 1
                height: parent.height
                color: Kirigami.Theme.disabledTextColor
                opacity: 0.3
            }

            DragHandler {
                target: null
                cursorShape: Qt.SplitHCursor
                onActiveChanged: {
                    if (active) {
                        // 每次手势从当前实际宽度起算，避免死区。
                        root._dragStartToolPaneWidth = root.toolPaneWidth
                    }
                }
                onTranslationChanged: {
                    if (active) {
                        // Issue #920：本地 clamp 到 [toolPaneMinWidth, _toolPaneMaxWidth]，
                        // 始终给正文留 editorMinWidth。activeTranslation 每次手势从 0 重置。
                        root.toolPaneWidth = Math.min(root._toolPaneMaxWidth,
                            Math.max(root.toolPaneMinWidth,
                                     root._dragStartToolPaneWidth - activeTranslation.x))
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
        x: root.leftPaneCollapsed
           ? Kirigami.Units.smallSpacing
           : Math.max(0, Math.min(root.width - width - Kirigami.Units.smallSpacing, sidebarRect.width - width / 2))
        y: Math.round((root.height - height) / 2)

        Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
        Behavior on border.color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

        Kirigami.Icon {
            anchors.centerIn: parent
            source: root.leftPaneCollapsed ? "arrow-right" : "arrow-left"
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
                if (root.leftPaneCollapsed) {
                    root.requestChapterNavigationOpen();
                } else {
                    root.closeChapterNavigation();
                }
                // 收起后把手贴到左边缘，留出边距；不需要动编辑会话。
                if (root.leftPaneCollapsed) root.requestEditorFocus()
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

        // 半透明遮罩，点击关闭抽屉。用主题文本色做暗化，避免写死颜色。
        Rectangle {
            anchors.fill: parent
            color: Kirigami.Theme.textColor
            opacity: 0.4
            MouseArea {
                anchors.fill: parent
                onClicked: root.closeChapterNavigation()
            }
        }

        // 左侧抽屉：复用 WritingChapterNavigation，宽度取可用宽度与 Qt 本地
        // listPaneWidth 的较小值（SinglePane 正文占满，抽屉只是临时覆盖）。
        WritingChapterNavigation {
            id: singlePaneNavPanel
            width: Math.min(parent.width, root.listPaneWidth)
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
