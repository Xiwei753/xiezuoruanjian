package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fitInside
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasurePolicy
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.layout.WindowInsetsRulers
import androidx.compose.ui.layout.layoutId
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidResolvedWorkspaceMode
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost
import com.xiwei.sujian.feature.editor.ui.theme.editorSurfaceBackgroundColor
import com.xiwei.sujian.feature.project.data.ProjectRepository

/**
 * 大屏写作工作台依赖 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceDeps(
    val appState: SujianAppState,
    val projectRepository: ProjectRepository,
    val projectWorkspaceActions: AndroidWorkspaceActionSpec,
    val chrome: SujianChromeSpec,
)

/**
 * 大屏写作工作台当前文档状态 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceDocumentState(
    val currentProjectId: String,
    val currentVolumeId: String?,
    val currentChapterId: String?,
    val currentChapterTitle: String,
    val presentationVisible: Boolean = true,
)

/**
 * 大屏写作工作台回调 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceCallbacks(
    val onBack: () -> Unit,
    val onSync: () -> Unit,
    val onSearch: () -> Unit,
    val onSettings: () -> Unit,
    val onChapterSwitch: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    val onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
)

/**
 * 工作台布局输入 — 打包传递，避免函数参数超出门禁阈值（#628 评论 5301021120 02:59:39Z 版）。
 *
 * plan 由导航套件层统一解析（外层顶栏归属消费同一份 Rust 最终 mode）；
 * pane 收起状态仍是 Android 局部 UI 状态，只作为 planner 输入（不抬到 Core）。
 */
internal data class WideWorkspaceLayoutState(
    val workbenchPlan: AndroidWorkbenchLayoutPlan?,
    val chapterTreeCollapsed: Boolean,
    val toolPaneCollapsed: Boolean,
    val onToggleChapterTree: () -> Unit,
    val onToggleToolPane: () -> Unit,
)

/**
 * 大屏写作工作台（#625 第二段 / #628 评论 5301021120 02:59:39Z 版）—
 * 按 Rust [AndroidWorkbenchLayoutPlan] 放 slot，不跨铰链/遮挡。
 *
 * 顶部工具栏三组独立容器由本组件承担（#628 验收点 6：Workbench Writing 由工作台
 * 自己拥有顶部工具栏，外层 app shell 不再额外画通用顶栏）。
 *
 * #628 评论 5301021120 02:59:39Z 版：`valid: Boolean` 已删除，改由 plan 的最终 [mode]
 * 表达产品模式，Android 只做映射：
 * - [AndroidResolvedWorkspaceMode.WORKBENCH]：按七角色 bounds 的自定义 [Layout] 放置；
 * - [AndroidResolvedWorkspaceMode.SINGLE_PANE]：仍然按 Rust 返回的 Editor bounds
 *   measure/place，**不能 `fillMaxSize()`** —— 遇到横向/竖向 separating hinge 时
 *   fallback 后不再重新跨过遮挡。
 *
 * 外层顶栏归属（SujianNavigationSuite）消费同一份 [AndroidWorkbenchLayoutPlan.mode]：
 * Workbench Writing 隐藏外层顶栏、由工作台三组 toolbar 自己画；SinglePane Writing
 * 恢复外层透明顶栏。不允许出现 layoutSpec=Workbench 但 plan 已 SinglePane、外层还按
 * Workbench 隐藏顶栏的分裂状态——因此 plan 由导航套件层统一解析一次后下发到本组件。
 *
 * #625 评论项4：用户主动收起 — 不按设备尺寸/方向自动多档收 pane。
 * 收起状态用 rememberSaveable 持有（在导航套件层），跨配置变化保留。收起只改变布局占用，
 * 不销毁/重建中央 [SujianEditorHost]（EditorPane 始终在组合中）。
 *
 * #625 评论项5：rail/pane 收成工具壳。当前无真实工具内容（星图/AI 归 #373/#506），
 * 工具列表为空，pane 显示空态，不放伪功能按钮。
 *
 * @param layoutState 导航套件层统一解析的 Rust workbench plan + pane 收起状态 + 收起回调
 *   （plan = null 表示桥失败，按单栏 Editor 处理）。收起左栏/右栏后由导航套件层重新 resolve，
 *   Rust 重新给 Editor 更大 bounds，中央 SujianEditorHost 仍是同一个实例，只改变测量/放置。
 */

internal enum class WideWorkspaceCompositionMode {
    /**
     * 大屏工作台组合模式（#640 C）— [resolveWideWorkspaceCompositionMode] 的返回值。
     *
     * - [SINGLE_PANE_WITH_TOP_BAR]：plan=null/SINGLE_PANE，在 Rust Editor free-region 内测量
     *   singlePaneTopBar + EditorPane body，top bar 是否 place 由 presentationVisible 决定，
     *   body 尺寸 hidden/visible 完全一致；
     * - [EDITOR_ONLY_PREWARM]：plan=WORKBENCH + hidden，只组合/测量唯一 EditorPane 按 Rust Editor bounds，
     *   不塞 top bar、不进 Workbench shell；
     * - [FULL_WORKBENCH]：plan=WORKBENCH + visible，完整 Workbench shell。
     *
     * EditorPane 始终在 [WideWritingWorkspace] 唯一 call site（layoutId=EDITOR），三种模式切换不重建 AndroidView。
     */
    SINGLE_PANE_WITH_TOP_BAR,
    EDITOR_ONLY_PREWARM,
    FULL_WORKBENCH,
}

/**
 * 大屏工作台组合模式决策（#640 C）— 纯函数，无 Compose 副作用，可单测。
 *
 * - plan=null/SINGLE_PANE → SINGLE_PANE_WITH_TOP_BAR（无论 hidden/visible，在 Rust Editor free-region
 *   内测量 singlePaneTopBar + EditorPane body）；
 * - plan=WORKBENCH + hidden → EDITOR_ONLY_PREWARM（只组合/测量唯一 EditorPane 按 Rust Editor bounds，
 *   不进入完整 Workbench shell，避免隐藏阶段预热 toolbar / 第二份 ChapterTree / sync collector /
 *   ToolRail / ToolPane）；
 * - plan=WORKBENCH + visible → FULL_WORKBENCH（完整 Workbench shell）。
 *
 * EditorPane 始终在 [WideWritingWorkspace] 唯一 call site（layoutId=EDITOR），三种模式切换不重建 AndroidView。
 * 保留 Rust workbench plan 的 Editor bounds 语义：EDITOR_ONLY_PREWARM/SINGLE_PANE_WITH_TOP_BAR 分支仍由
 * 调用方按 [AndroidWorkbenchLayoutPlan.placementFor]([AndroidWorkbenchRole.EDITOR]) 取 bounds place，
 * 不用延时/alpha/GONE 或另一个 editor/state source。
 */
internal fun resolveWideWorkspaceCompositionMode(
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    presentationVisible: Boolean,
): WideWorkspaceCompositionMode {
    val isSinglePanePlan =
        workbenchPlan == null ||
            workbenchPlan.mode == AndroidResolvedWorkspaceMode.SINGLE_PANE
    if (isSinglePanePlan) return WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR
    return if (presentationVisible) {
        WideWorkspaceCompositionMode.FULL_WORKBENCH
    } else {
        WideWorkspaceCompositionMode.EDITOR_ONLY_PREWARM
    }
}

/**
 * #640 评论 5441849412 问题2 / 5442422507：EditorPane 唯一 call site 由源码结构保证。
 *
 * 之前 EDITOR_ONLY/FULL_WORKBENCH 两分支各自创建 EditorPane，是两个 call site，
 * Compose composable 实例由 call site 识别 → AndroidView 被丢弃重建。重构后 EditorPane
 * 始终在 [WideWritingWorkspace] 的 Layout content 唯一 call site（layoutId=EDITOR），
 * 切换 compositionMode 只增删 Workbench chrome slot，不移动 Editor 本身。
 *
 * 不再用纯函数 identity 锁此不变量（纯函数 self-proving，无生产消费者）— 由源码结构
 * （EditorPane 始终在同一 call site）和 [EditorPresentationWideIdentityTest] 锁
 * [resolveWideWorkspaceCompositionMode] 决策（AndroidView 不重建的前提）保证。
 */

@Suppress("LongParameterList") // #640 评论 5444584755：7 参数达 threshold，函数级 suppress（既有先例）
@Composable
internal fun WideWritingWorkspace(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    layoutState: WideWorkspaceLayoutState,
    callbacks: WideWorkspaceCallbacks,
    fallbackSafeBounds: AndroidLayoutRect,
    singlePaneTopBar: (@Composable () -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val workbenchPlan = layoutState.workbenchPlan
    val chapterTreeCollapsed = layoutState.chapterTreeCollapsed
    val toolPaneCollapsed = layoutState.toolPaneCollapsed
    // #625 评论项5：当前选中工具 — rememberSaveable 持有。星图/AI 归 #373/#506。
    // 工具选中不是 planner 输入（visibility 只有章节树/工具面板两个布尔），保持本组件局部状态。
    var selectedToolId by rememberSaveable { mutableStateOf<String?>(null) }

    // #628 评论 5301021120 02:59:39Z 版：plan 由导航套件层统一解析（外层顶栏归属必须消费
    // 同一份 Rust 最终 mode），本组件不再自行 resolve。
    // #640 C：presentationVisible=false 时即使 plan=WORKBENCH 也只组合/测量唯一 EditorPane,
    // 不进入完整 Workbench shell（避免隐藏阶段预热）。决策由纯函数 [resolveWideWorkspaceCompositionMode]
    // 统一表达，生产分支与单测共用同一逻辑。
    val compositionMode =
        resolveWideWorkspaceCompositionMode(
            workbenchPlan = workbenchPlan,
            presentationVisible = documentState.presentationVisible,
        )
    val density = LocalDensity.current

    // #640 评论 5441849412 问题2：EditorPane 始终在唯一 call site（layoutId=EDITOR），
    // 不在 EDITOR_ONLY/FULL_WORKBENCH 两分支各自创建。切换 compositionMode 只增删 Workbench chrome slot,
    // 不移动 Editor 本身 — Compose composable 实例由 call site 识别，call site 不变则 AndroidView 不重建。
    val isFullWorkbench =
        compositionMode == WideWorkspaceCompositionMode.FULL_WORKBENCH && workbenchPlan != null
    val workbenchBounds =
        if (isFullWorkbench) {
            WorkbenchBounds.fromPlan(requireNotNull(workbenchPlan))
        } else {
            null
        }
    val slotState =
        WorkbenchSlotState(
            chapterTreeCollapsed = chapterTreeCollapsed,
            toolPaneCollapsed = toolPaneCollapsed,
            selectedToolId = selectedToolId,
            onToggleChapterTree = layoutState.onToggleChapterTree,
            onToggleToolPane = layoutState.onToggleToolPane,
            onSelectTool = { id -> selectedToolId = id },
        )

    // 始终用同一个 Layout；EditorPane 始终是 layoutId=EDITOR 的 slot，唯一 call site。
    // measure policy 按 compositionMode 选 place 策略，但 EditorPane composable 实例不变。
    Layout(
        modifier = modifier.fillMaxSize(),
        content = {
            // Workbench chrome slots — 只在 FULL_WORKBENCH 时组合（toolbar/chapter tree/tool pane/rail）。
            // 不创建 EditorPane — EditorPane 在下面唯一 call site。
            if (isFullWorkbench && workbenchBounds != null) {
                WorkbenchChromeSlots(
                    deps = deps,
                    documentState = documentState,
                    callbacks = callbacks,
                    state = slotState,
                    bounds = workbenchBounds,
                )
            }
            // #640 评论 5443102488：single-pane top bar slot — 只在 SINGLE_PANE_WITH_TOP_BAR 时组合。
            // measure policy 在 Rust Editor free-region 内测量 top bar + body（body = region - top bar height）；
            // presentationVisible=false 时只不 place top bar，body 尺寸与 visible 完全一致（不触发 onSizeChanged）。
            // EditorPane 仍在下面唯一 call site，top bar 是 sibling slot，不移动 Editor 本身。
            if (compositionMode == WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR) {
                Box(modifier = Modifier.layoutId(LayoutSlotId.SINGLE_PANE_TOP_BAR)) {
                    singlePaneTopBar?.invoke()
                }
            }
            // 唯一 EditorPane — 始终在同一个 call site，layoutId=EDITOR。
            // SINGLE_PANE_WITH_TOP_BAR：measure policy 按 Rust Editor region 测量 body（region - top bar）。
            // EDITOR_ONLY_PREWARM：measure policy 按 Rust Editor bounds place（无 top bar）。
            // FULL_WORKBENCH：measure policy 按七角色 bounds place。
            // 切换 compositionMode 不改变 EditorPane 的 call site，AndroidView 不重建。
            EditorPane(
                documentState = documentState,
                onChapterSwitchFailed = callbacks.onChapterSwitchFailed,
                modifier = Modifier.layoutId(LayoutSlotId.EDITOR),
            )
        },
        measurePolicy =
            wideWorkspaceMeasurePolicy(
                compositionMode = compositionMode,
                workbenchPlan = workbenchPlan,
                density = density,
                presentationVisible = documentState.presentationVisible,
                fallbackSafeBounds = fallbackSafeBounds,
            ),
    )
}

/** Layout slot 标识 — 用于自定义 Layout 中按 layoutId 取 placeable。 */
private enum class LayoutSlotId {
    TOOLBAR_LEADING,
    TOOLBAR_CENTER,
    TOOLBAR_TRAILING,
    CHAPTER_NAVIGATION,
    SINGLE_PANE_TOP_BAR,
    EDITOR,
    TOOL_PANE,
    TOOL_RAIL,
}

private val EMPTY_RECT: AndroidLayoutRect = AndroidLayoutRect(0f, 0f, 0f, 0f)

/**
 * 七角色 bounds 打包 — 避免 [boundsForSlot] 参数过多（#628 lint LongParameterList）。
 */
private data class WorkbenchBounds(
    val toolbarLeading: AndroidLayoutRect?,
    val toolbarCenter: AndroidLayoutRect?,
    val toolbarTrailing: AndroidLayoutRect?,
    val chapterNav: AndroidLayoutRect?,
    val editor: AndroidLayoutRect?,
    val toolPane: AndroidLayoutRect?,
    val toolRail: AndroidLayoutRect?,
) {
    companion object {
        fun fromPlan(plan: AndroidWorkbenchLayoutPlan): WorkbenchBounds {
            val p = plan::placementFor
            return WorkbenchBounds(
                toolbarLeading = p(AndroidWorkbenchRole.TOOLBAR_LEADING)?.bounds,
                toolbarCenter = p(AndroidWorkbenchRole.TOOLBAR_CENTER)?.bounds,
                toolbarTrailing = p(AndroidWorkbenchRole.TOOLBAR_TRAILING)?.bounds,
                chapterNav = p(AndroidWorkbenchRole.CHAPTER_NAVIGATION)?.bounds,
                editor = p(AndroidWorkbenchRole.EDITOR)?.bounds,
                toolPane = p(AndroidWorkbenchRole.TOOL_PANE)?.bounds,
                toolRail = p(AndroidWorkbenchRole.TOOL_RAIL)?.bounds,
            )
        }

        /** 按 slotId 查表取 bounds（Map 替代 when，降低圈复杂度）。 */
        private val slotBoundsMap: Map<LayoutSlotId, (WorkbenchBounds) -> AndroidLayoutRect> =
            mapOf(
                LayoutSlotId.TOOLBAR_LEADING to { b -> b.toolbarLeading ?: EMPTY_RECT },
                LayoutSlotId.TOOLBAR_CENTER to { b -> b.toolbarCenter ?: EMPTY_RECT },
                LayoutSlotId.TOOLBAR_TRAILING to { b -> b.toolbarTrailing ?: EMPTY_RECT },
                LayoutSlotId.CHAPTER_NAVIGATION to { b -> b.chapterNav ?: EMPTY_RECT },
                LayoutSlotId.EDITOR to { b -> b.editor ?: EMPTY_RECT },
                LayoutSlotId.TOOL_PANE to { b -> b.toolPane ?: EMPTY_RECT },
                LayoutSlotId.TOOL_RAIL to { b -> b.toolRail ?: EMPTY_RECT },
            )
    }

    /** 按 slotId 查表取 bounds（Map 替代 when，降低圈复杂度）。 */
    fun forSlot(slotId: LayoutSlotId): AndroidLayoutRect = slotBoundsMap.getValue(slotId)(this)
}

/**
 * 工作台 slot 可变状态 + 回调 — 提取以降低 [WideWritingWorkspace] 行数。
 */
private data class WorkbenchSlotState(
    val chapterTreeCollapsed: Boolean,
    val toolPaneCollapsed: Boolean,
    val selectedToolId: String?,
    val onToggleChapterTree: () -> Unit,
    val onToggleToolPane: () -> Unit,
    val onSelectTool: (String?) -> Unit,
)

/**
 * #640 评论 5441849412 问题2：Workbench chrome slot 内容（不含 EditorPane）— 提取以降低
 * [WideWritingWorkspace] 行数。EditorPane 在 [WideWritingWorkspace] 顶层唯一 call site，
 * 不在此处创建，避免 EDITOR_ONLY/FULL_WORKBENCH 两分支各自创建导致 AndroidView 重建。
 */
@Composable
private fun WorkbenchChromeSlots(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    callbacks: WideWorkspaceCallbacks,
    state: WorkbenchSlotState,
    bounds: WorkbenchBounds,
) {
    val editorWindowHost = LocalEditorWindowHost.current
    val onUndo: () -> Unit = { editorWindowHost?.performUndo() }
    val onRedo: () -> Unit = { editorWindowHost?.performRedo() }
    val appDeps = LocalSujianAppDependencies.current
    val syncState by appDeps.syncStatusRepository.state.collectAsState()
    val tools = remember { emptyList<WritingToolItem>() }
    val toolPaneContent: (@Composable () -> Unit)? = null

    WorkbenchToolbarSlots(
        deps = deps,
        callbacks = callbacks,
        state = state,
        onUndo = onUndo,
        onRedo = onRedo,
        syncState = syncState,
    )
    WorkbenchChromeContentSlots(
        deps = deps,
        documentState = documentState,
        state = state,
        bounds = bounds,
        callbacks = callbacks,
        env =
            WorkbenchContentEnv(
                tools = tools,
                toolPaneContent = toolPaneContent,
            ),
    )
}

/** Toolbar 三组 slot — 提取以降低 [WorkbenchChromeSlots] 行数。 */
@Composable
private fun WorkbenchToolbarSlots(
    deps: WideWorkspaceDeps,
    callbacks: WideWorkspaceCallbacks,
    state: WorkbenchSlotState,
    onUndo: () -> Unit,
    onRedo: () -> Unit,
    syncState: com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState,
) {
    // #640 评论 5443102488：fitInside 已移到各 toolbar composable 内部的 Row/content 上
    // （Surface 保持完整 Rust rect 画背景，内容避开 safe region，符合 Material3 TopAppBar
    // "背景画到系统栏、内容避开系统栏"原则）。slot 根节点只挂 layoutId 给 measure policy 取。
    WritingToolbarLeadingGroup(
        showBack = deps.chrome.showBack,
        chapterTreeCollapsed = state.chapterTreeCollapsed,
        callbacks =
            WritingToolbarLeadingCallbacks(
                onBack = callbacks.onBack,
                onUndo = onUndo,
                onRedo = onRedo,
                onToggleChapterTree = state.onToggleChapterTree,
            ),
        modifier = Modifier.layoutId(LayoutSlotId.TOOLBAR_LEADING),
    )
    WritingToolbarCenterSlot(
        modifier = Modifier.layoutId(LayoutSlotId.TOOLBAR_CENTER),
    )
    WritingToolbarTrailingGroup(
        actions = deps.chrome.actions,
        syncState = syncState,
        callbacks =
            WritingToolbarTrailingCallbacks(
                onSync = callbacks.onSync,
                onSearch = callbacks.onSearch,
                onSettings = callbacks.onSettings,
            ),
        modifier = Modifier.layoutId(LayoutSlotId.TOOLBAR_TRAILING),
    )
}

/**
 * Content slot 所需的编辑环境 — 打包传递，避免函数参数超出门禁阈值。
 */
private data class WorkbenchContentEnv(
    val tools: List<WritingToolItem>,
    val toolPaneContent: (@Composable () -> Unit)?,
)

/**
 * #640 评论 5441849412 问题2：Workbench chrome content slot（章节树/工具面板/工具栏图标列）—
 * 不含 EditorPane。EditorPane 在 [WideWritingWorkspace] 顶层唯一 call site（layoutId=EDITOR）。
 */
@Composable
private fun WorkbenchChromeContentSlots(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    state: WorkbenchSlotState,
    bounds: WorkbenchBounds,
    callbacks: WideWorkspaceCallbacks,
    env: WorkbenchContentEnv,
) {
    // #640 评论 5443789509：slot 根节点占 Rust rect（layoutId 被 measure policy 取）。
    // 稳定系统栏（systemBars + displayCutout）已由 workbench safe frame 在 planner 输入前
    // 裁掉，Rust plan 的 slot rect 已在安全区内；slot 内不再 fitInside(SafeDrawing)，
    // 避免二次避让。不改 ChapterTreeContent/WritingToolPane/WritingToolRail 内部 —
    // ChapterTreeContent 有窄屏调用方（ProjectWorkspaceScreen.kt），改内部会 double-inset。
    // 章节树：用户主动收起时不画（bounds 由 plan 决定，收起通过 visibility 重算 plan）。
    if (!state.chapterTreeCollapsed && bounds.chapterNav != null && !bounds.chapterNav.isEmpty) {
        ChapterTreeContent(
            projectId = documentState.currentProjectId,
            projectRepository = deps.projectRepository,
            workspaceActions = deps.projectWorkspaceActions,
            onSelectChapter = callbacks.onChapterSwitch,
            onError = deps.appState::reportWorkspaceError,
            modifier = Modifier.layoutId(LayoutSlotId.CHAPTER_NAVIGATION),
        )
    }
    // EditorPane 不在此处 — 在 WideWritingWorkspace 顶层唯一 call site（layoutId=EDITOR），
    // 避免 EDITOR_ONLY/FULL_WORKBENCH 切换时 AndroidView 重建。
    // 工具面板（用户主动收起时不画）。
    if (!state.toolPaneCollapsed && bounds.toolPane != null && !bounds.toolPane.isEmpty) {
        WritingToolPane(
            content = env.toolPaneContent,
            modifier = Modifier.layoutId(LayoutSlotId.TOOL_PANE),
        )
    }
    // 工具栏图标列（始终画）。
    WritingToolRail(
        tools = env.tools,
        selectedToolId = state.selectedToolId,
        onSelect = state.onSelectTool,
        onTogglePane = state.onToggleToolPane,
        paneCollapsed = state.toolPaneCollapsed,
        modifier = Modifier.layoutId(LayoutSlotId.TOOL_RAIL),
    )
}

/**
 * #640 评论 5441849412 问题2：构造 wide workspace measure policy — 按 [compositionMode] 分发。
 *
 * - FULL_WORKBENCH + plan 非空：按七角色 bounds measure/place 所有 slot（含 Editor）；
 * - SINGLE_PANE_WITH_TOP_BAR：在 Rust Editor free-region 内测量 singlePaneTopBar + Editor body；
 * - EDITOR_ONLY_PREWARM：只按 Rust Editor bounds measure/place Editor slot，其他 chrome slot 不组合。
 *
 * EditorPane 始终是 layoutId=EDITOR 的 slot，call site 不变；切换 compositionMode 只改变 place 策略,
 * 不重建 AndroidView。
 *
 * #640 评论 5444584755：[fallbackSafeBounds] 是 safe frame 对应的物理安全矩形（dp），
 * plan 有无都共用。SINGLE_PANE_WITH_TOP_BAR / EDITOR_ONLY_PREWARM（以及 FULL_WORKBENCH
 * 但 plan null 的退化分支）在 `workbenchPlan == null` 或 Editor bounds 空时用它作 fallback，
 * 不再回落整个 constraints（物理窗口 (0,0)）。FULL_WORKBENCH + plan 非空走七角色 bounds，不需要它。
 */
private fun wideWorkspaceMeasurePolicy(
    compositionMode: WideWorkspaceCompositionMode,
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    density: Density,
    presentationVisible: Boolean,
    fallbackSafeBounds: AndroidLayoutRect,
): MeasurePolicy =
    MeasurePolicy { measurables, constraints ->
        when (compositionMode) {
            WideWorkspaceCompositionMode.FULL_WORKBENCH -> {
                if (workbenchPlan != null) {
                    measureAndPlaceWorkbench(
                        measurables,
                        constraints,
                        WorkbenchBounds.fromPlan(workbenchPlan),
                        density,
                    )
                } else {
                    measureAndPlaceEditorOnly(measurables, constraints, workbenchPlan, density, fallbackSafeBounds)
                }
            }
            WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR ->
                measureAndPlaceSinglePaneWithTopBar(
                    measurables,
                    constraints,
                    workbenchPlan,
                    density,
                    presentationVisible,
                    fallbackSafeBounds,
                )
            WideWorkspaceCompositionMode.EDITOR_ONLY_PREWARM ->
                measureAndPlaceEditorOnly(measurables, constraints, workbenchPlan, density, fallbackSafeBounds)
        }
    }

/**
 * #640 评论 5441849412 问题2：EDITOR_ONLY measure + place — 只 place Editor slot。
 *
 * 按 Rust [AndroidWorkbenchLayoutPlan] 的 EDITOR 角色 bounds measure/place Editor；
 * plan null 或 Editor bounds 空 → 回落 [fallbackSafeBounds]（safe frame 物理安全矩形）。
 * 不能 `fillMaxSize()` 当 bounds 可用时：遇到横向/竖向 separating hinge 时全窗口铺满会让正文
 * 重新跨过遮挡。与 Workbench 模式用同一套 bounds 语义。
 *
 * #640 评论 5444584755：fallback 不再回落整个 constraints（物理窗口 (0,0)），改用
 * [fallbackSafeBounds] — safe frame 已裁掉稳定系统栏/刘海，避免 TopAppBar 从 (0,0) 开始、
 * 正文只躲 IME 不躲 status bar / display cutout / navigation bar。统一一条 dp→px 路径，
 * 不再有 if/else 两分支或第二套 fillMaxSize() 坐标系。
 */
private fun MeasureScope.measureAndPlaceEditorOnly(
    measurables: List<Measurable>,
    constraints: Constraints,
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    density: Density,
    fallbackSafeBounds: AndroidLayoutRect,
): MeasureResult {
    val editorMeasurable =
        measurables.firstOrNull { (it.layoutId as? LayoutSlotId) == LayoutSlotId.EDITOR }
            ?: return layout(constraints.maxWidth, constraints.maxHeight) {}
    val editorBounds =
        workbenchPlan?.placementFor(AndroidWorkbenchRole.EDITOR)?.bounds?.takeUnless { it.isEmpty }
            ?: fallbackSafeBounds
    return with(density) {
        val pxWidth = editorBounds.widthDp.dp.roundToPx()
        val pxHeight = editorBounds.heightDp.dp.roundToPx()
        val placeable =
            editorMeasurable.measure(
                constraints.copy(
                    minWidth = pxWidth,
                    maxWidth = pxWidth,
                    minHeight = pxHeight,
                    maxHeight = pxHeight,
                ),
            )
        layout(constraints.maxWidth, constraints.maxHeight) {
            placeable.place(
                editorBounds.leftDp.dp.roundToPx(),
                editorBounds.topDp.dp.roundToPx(),
            )
        }
    }
}

/**
 * #640 评论 5443102488：SINGLE_PANE_WITH_TOP_BAR measure + place — 在 Rust Editor free-region 内测量 top bar + body。
 *
 * root region = Rust [AndroidWorkbenchLayoutPlan] 的 EDITOR 角色 bounds（plan 非空且 bounds 非空），
 * 否则回落 [fallbackSafeBounds]（safe frame 物理安全矩形）。在 root region 内：
 * - 测量 singlePaneTopBar（width = root width，height unconstrained）取其实际高度；
 * - body height = root height - top bar height（钳到非负）；
 * - place top bar 在 (root.left, root.top)（仅 presentationVisible=true）；
 * - place EditorPane body 在 (root.left, root.top + topBarHeight)。
 *
 * hidden 与 visible 的 body bounds 完全相同（top bar 始终测量，只 place 随 visible 切换），
 * visible 切换不触发 SujianEditorView onSizeChanged，ready 一次成立后稳定。
 * EditorPane 仍是 layoutId=EDITOR 的 slot，call site 不变。
 *
 * #640 评论 5444584755：fallback 不再回落整个 constraints（物理窗口 (0,0)），改用
 * [fallbackSafeBounds] — safe frame 已裁掉稳定系统栏/刘海，避免 TopAppBar 从 (0,0) 开始、
 * 正文只躲 IME 不躲 status bar / display cutout / navigation bar。统一一条 dp→px 路径，
 * 不再有 if/else 两分支或第二套 constraints 坐标系。
 */
private fun MeasureScope.measureAndPlaceSinglePaneWithTopBar(
    measurables: List<Measurable>,
    constraints: Constraints,
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    density: Density,
    presentationVisible: Boolean,
    fallbackSafeBounds: AndroidLayoutRect,
): MeasureResult {
    val byId = measurables.associateBy { it.layoutId as? LayoutSlotId }
    val topBarMeasurable = byId[LayoutSlotId.SINGLE_PANE_TOP_BAR]
    val editorMeasurable =
        byId[LayoutSlotId.EDITOR] ?: return layout(constraints.maxWidth, constraints.maxHeight) {}

    val rootBounds =
        workbenchPlan
            ?.placementFor(AndroidWorkbenchRole.EDITOR)
            ?.bounds
            ?.takeUnless { it.isEmpty }
            ?: fallbackSafeBounds
    val rootRegion =
        with(density) {
            SinglePaneRootRegion(
                leftPx = rootBounds.leftDp.dp.roundToPx(),
                topPx = rootBounds.topDp.dp.roundToPx(),
                widthPx = rootBounds.widthDp.dp.roundToPx(),
                heightPx = rootBounds.heightDp.dp.roundToPx(),
            )
        }

    val topBarPlaceable =
        topBarMeasurable?.measure(
            constraints.copy(
                minWidth = rootRegion.widthPx,
                maxWidth = rootRegion.widthPx,
                minHeight = 0,
                maxHeight = Constraints.Infinity,
            ),
        )
    val topBarHeight = (topBarPlaceable?.height ?: 0).coerceIn(0, rootRegion.heightPx)
    val bodyHeight = (rootRegion.heightPx - topBarHeight).coerceAtLeast(0)

    val editorPlaceable =
        editorMeasurable.measure(
            constraints.copy(
                minWidth = rootRegion.widthPx,
                maxWidth = rootRegion.widthPx,
                minHeight = bodyHeight,
                maxHeight = bodyHeight,
            ),
        )

    return layout(constraints.maxWidth, constraints.maxHeight) {
        if (presentationVisible) {
            topBarPlaceable?.place(rootRegion.leftPx, rootRegion.topPx)
        }
        editorPlaceable.place(rootRegion.leftPx, rootRegion.topPx + topBarHeight)
    }
}

/** single-pane root region（px）— 提取以降低 [measureAndPlaceSinglePaneWithTopBar] 圈复杂度。 */
private data class SinglePaneRootRegion(
    val leftPx: Int,
    val topPx: Int,
    val widthPx: Int,
    val heightPx: Int,
)

/**
 * 构造 workbench measure policy — 提取以降低 [wideWorkspaceMeasurePolicy] 行数。
 * Android 只做 dp→px 和 place，不判断 hinge 在左还是右。
 */
private fun workbenchMeasurePolicy(
    bounds: WorkbenchBounds,
    density: Density,
): MeasurePolicy =
    MeasurePolicy { measurables, constraints ->
        measureAndPlaceWorkbench(measurables, constraints, bounds, density)
    }

/**
 * measure + place 逻辑 — 纯函数，降低 [WideWritingWorkspace] 圈复杂度。
 */
private fun MeasureScope.measureAndPlaceWorkbench(
    measurables: List<Measurable>,
    constraints: Constraints,
    bounds: WorkbenchBounds,
    density: Density,
): MeasureResult =
    with(density) {
        val placeables =
            measurables.associate { measurable ->
                val slotId = measurable.layoutId as LayoutSlotId
                val rect = bounds.forSlot(slotId)
                val pxWidth = rect.widthDp.dp.roundToPx()
                val pxHeight = rect.heightDp.dp.roundToPx()
                val placeable =
                    measurable.measure(
                        constraints.copy(
                            minWidth = pxWidth,
                            maxWidth = pxWidth,
                            minHeight = pxHeight,
                            maxHeight = pxHeight,
                        ),
                    )
                slotId to (placeable to rect)
            }
        layout(constraints.maxWidth, constraints.maxHeight) {
            placeables.values.forEach { (placeable, rect) ->
                val x = rect.leftDp.dp.roundToPx()
                val y = rect.topDp.dp.roundToPx()
                placeable.place(x, y)
            }
        }
    }

/**
 * 正文编辑器面板 — 提取以降低 [WideWritingWorkspace] 认知复杂度。
 * 复用 [SujianEditorHost]，不创建第二个编辑器。
 *
 * #640 A.4：预热阶段（presentationVisible=false）背景透明，不画 opaque editor surface；
 * 可见阶段用共享 editorSurfaceBackgroundColor，不用 MaterialTheme colorScheme background。
 *
 * #640 评论 5442422507 / 5443789509：外层 Box 占 Rust Editor rect（layoutId 被 measure policy 取，按 Rust rect
 * measure/place）。SujianEditorHost / select-chapter 提示在内层用
 * `fitInside(WindowInsetsRulers.Ime.current)` 把内容收进 IME 上方。
 * 稳定系统栏（systemBars + displayCutout）/ 折叠遮挡已由 workbench safe frame + Rust plan
 * 处理（slot rect 已在安全区内）；IME 由 Editor 自己用 `Ime.current` 动态处理，键盘出现只改变
 * Editor 实际可用高度，不重算 Rust workbench plan。plan 仍是唯一布局决策。
 */
@Composable
private fun EditorPane(
    documentState: WideWorkspaceDocumentState,
    modifier: Modifier = Modifier,
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
) {
    val background =
        if (documentState.presentationVisible) {
            editorSurfaceBackgroundColor()
        } else {
            androidx.compose.ui.graphics.Color.Transparent
        }
    Box(
        modifier =
            modifier
                .background(background),
    ) {
        // #640 评论 5443789509：fitInside(Ime.current) 在 slot 内部把内容收进 IME 上方。
        // 稳定系统栏/刘海/折叠遮挡由 workbench safe frame + Rust plan 处理（slot rect 已在安全区），
        // IME 由 Editor 自己动态处理，键盘出现只改变 Editor 实际可用高度，不重算 Rust workbench plan。
        val imeRulers = WindowInsetsRulers.Ime.current
        if (documentState.currentChapterId != null && documentState.currentVolumeId != null) {
            SujianEditorHost(
                projectId = documentState.currentProjectId,
                volumeId = documentState.currentVolumeId,
                chapterId = documentState.currentChapterId,
                chapterTitle = documentState.currentChapterTitle,
                modifier = Modifier.fillMaxSize().fitInside(imeRulers),
                onChapterSwitchFailed = onChapterSwitchFailed,
                presentationVisible = documentState.presentationVisible,
            )
        } else {
            Box(
                modifier = Modifier.fillMaxSize().fitInside(imeRulers),
                contentAlignment = Alignment.Center,
            ) {
                Text(stringResource(id = R.string.select_chapter_to_write))
            }
        }
    }
}
