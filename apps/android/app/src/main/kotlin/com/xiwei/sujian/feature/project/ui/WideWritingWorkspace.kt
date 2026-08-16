package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasurePolicy
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.layout.layout
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
import com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.requestOpenChapter
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost
import com.xiwei.sujian.feature.project.data.ProjectRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

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
)

/**
 * 大屏写作工作台回调 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceCallbacks(
    val onBack: () -> Unit,
    val onSync: () -> Unit,
    val onSearch: () -> Unit,
    val onSettings: () -> Unit,
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
@Composable
internal fun WideWritingWorkspace(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    editorViewModel: EditorViewModel,
    layoutState: WideWorkspaceLayoutState,
    callbacks: WideWorkspaceCallbacks,
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
    // plan == null（桥失败）或 mode == SINGLE_PANE（放不下最小 workbench）→ 单栏 Editor。
    if (workbenchPlan == null || workbenchPlan.mode == AndroidResolvedWorkspaceMode.SINGLE_PANE) {
        // 按 Rust 返回的 Editor bounds measure/place，不能 fillMaxSize()：
        // 遇到横向/竖向 separating hinge 时，fillMaxSize 会让正文重新跨过遮挡。
        val editorBounds = workbenchPlan?.placementFor(AndroidWorkbenchRole.EDITOR)?.bounds
        val density = LocalDensity.current
        EditorPane(
            documentState = documentState,
            onChapterSwitchFailed = callbacks.onChapterSwitchFailed,
            modifier =
                if (editorBounds != null && !editorBounds.isEmpty) {
                    modifier.editorAtBounds(editorBounds, density)
                } else {
                    // 桥失败（null）时 Rust bounds 不可用，只能本地占满——没有任何遮挡信息。
                    modifier.fillMaxSize()
                },
        )
        return
    }
    val bounds = WorkbenchBounds.fromPlan(workbenchPlan)
    val density = LocalDensity.current
    val slotState =
        WorkbenchSlotState(
            chapterTreeCollapsed = chapterTreeCollapsed,
            toolPaneCollapsed = toolPaneCollapsed,
            selectedToolId = selectedToolId,
            onToggleChapterTree = layoutState.onToggleChapterTree,
            onToggleToolPane = layoutState.onToggleToolPane,
            onSelectTool = { id -> selectedToolId = id },
        )
    Layout(
        modifier = modifier.fillMaxSize(),
        content = {
            WorkbenchSlots(
                deps = deps,
                documentState = documentState,
                editorViewModel = editorViewModel,
                callbacks = callbacks,
                state = slotState,
                bounds = bounds,
            )
        },
        measurePolicy = workbenchMeasurePolicy(bounds, density),
    )
}

/** Layout slot 标识 — 用于自定义 Layout 中按 layoutId 取 placeable。 */
private enum class LayoutSlotId {
    TOOLBAR_LEADING,
    TOOLBAR_CENTER,
    TOOLBAR_TRAILING,
    CHAPTER_NAVIGATION,
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
 * 七角色 slot 内容 — 提取以降低 [WideWritingWorkspace] 行数。
 */
@Composable
private fun WorkbenchSlots(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    editorViewModel: EditorViewModel,
    callbacks: WideWorkspaceCallbacks,
    state: WorkbenchSlotState,
    bounds: WorkbenchBounds,
) {
    val coroutineScope = rememberCoroutineScope()
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
    WorkbenchContentSlots(
        deps = deps,
        documentState = documentState,
        state = state,
        bounds = bounds,
        callbacks = callbacks,
        env =
            WorkbenchContentEnv(
                editorViewModel = editorViewModel,
                coroutineScope = coroutineScope,
                tools = tools,
                toolPaneContent = toolPaneContent,
            ),
    )
}

/** Toolbar 三组 slot — 提取以降低 [WorkbenchSlots] 行数。 */
@Composable
private fun WorkbenchToolbarSlots(
    deps: WideWorkspaceDeps,
    callbacks: WideWorkspaceCallbacks,
    state: WorkbenchSlotState,
    onUndo: () -> Unit,
    onRedo: () -> Unit,
    syncState: com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState,
) {
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
    val editorViewModel: EditorViewModel,
    val coroutineScope: CoroutineScope,
    val tools: List<WritingToolItem>,
    val toolPaneContent: (@Composable () -> Unit)?,
)

/**
 * Content 四角色 slot — 提取以降低 [WorkbenchSlots] 行数。
 */
@Composable
private fun WorkbenchContentSlots(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    state: WorkbenchSlotState,
    bounds: WorkbenchBounds,
    callbacks: WideWorkspaceCallbacks,
    env: WorkbenchContentEnv,
) {
    // 章节树：用户主动收起时不画（bounds 由 plan 决定，收起通过 visibility 重算 plan）。
    if (!state.chapterTreeCollapsed && bounds.chapterNav != null && !bounds.chapterNav.isEmpty) {
        ChapterTreeContent(
            projectId = documentState.currentProjectId,
            projectRepository = deps.projectRepository,
            workspaceActions = deps.projectWorkspaceActions,
            onSelectChapter = { volumeId, chapterId, chapterTitle ->
                ChapterSelectContext(
                    coroutineScope = env.coroutineScope,
                    editorViewModel = env.editorViewModel,
                    appState = deps.appState,
                    projectId = documentState.currentProjectId,
                ).handleChapterSelect(
                    volumeId = volumeId,
                    chapterId = chapterId,
                    chapterTitle = chapterTitle,
                )
            },
            onError = deps.appState::reportWorkspaceError,
            modifier = Modifier.layoutId(LayoutSlotId.CHAPTER_NAVIGATION),
        )
    }
    // 正文编辑器（复用 SujianEditorHost，不创建第二个；始终在组合中，bounds 由 plan 决定）。
    EditorPane(
        documentState = documentState,
        onChapterSwitchFailed = callbacks.onChapterSwitchFailed,
        modifier = Modifier.layoutId(LayoutSlotId.EDITOR),
    )
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
 * SinglePane 退化：按 Rust 返回的 Editor bounds measure/place（#628 评论 5301021120 02:59:39Z 版）。
 *
 * 不能 `fillMaxSize()`：遇到横向/竖向 separating hinge 时，全窗口铺满会让正文
 * 重新跨过遮挡。这里与 Workbench 模式用同一套 bounds 语义 — 子级按 bounds 尺寸
 * 测量、按 bounds 偏移放置，Editor 只占 Rust 算好的最大连续安全 free-region。
 */
private fun Modifier.editorAtBounds(
    bounds: AndroidLayoutRect,
    density: Density,
): Modifier =
    this.then(
        Modifier.layout { measurable, constraints ->
            with(density) {
                val pxWidth = bounds.widthDp.dp.roundToPx()
                val pxHeight = bounds.heightDp.dp.roundToPx()
                val placeable =
                    measurable.measure(
                        constraints.copy(
                            minWidth = pxWidth,
                            maxWidth = pxWidth,
                            minHeight = pxHeight,
                            maxHeight = pxHeight,
                        ),
                    )
                layout(constraints.maxWidth, constraints.maxHeight) {
                    placeable.place(
                        bounds.leftDp.dp.roundToPx(),
                        bounds.topDp.dp.roundToPx(),
                    )
                }
            }
        },
    )

/**
 * 构造 workbench measure policy — 提取以降低 [WideWritingWorkspace] 行数。
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
 * 章节选择事务上下文 — 打包传递，避免函数参数超出门禁阈值。
 */
private data class ChapterSelectContext(
    val coroutineScope: CoroutineScope,
    val editorViewModel: EditorViewModel,
    val appState: SujianAppState,
    val projectId: String,
)

/**
 * 章节选择事务 — 提取以降低 [WideWritingWorkspace] 认知复杂度。
 * 复用 ProjectWorkspaceScreen 的章节切换事务入口 — 事务成功后才提交业务选择。
 */
private fun ChapterSelectContext.handleChapterSelect(
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
) {
    coroutineScope.launch {
        val result =
            editorViewModel.requestOpenChapter(
                projectId,
                volumeId,
                chapterId,
                chapterTitle,
            )
        when (result) {
            is ChapterSwitchResult.Success -> {
                appState.selectChapter(volumeId, chapterId, chapterTitle)
            }
            is ChapterSwitchResult.SaveFailed,
            is ChapterSwitchResult.LoadFailed,
            ChapterSwitchResult.Stale,
            -> {
                // 错误提示已由 ViewModel 事件（toast）发出。
            }
        }
    }
}

/**
 * 正文编辑器面板 — 提取以降低 [WideWritingWorkspace] 认知复杂度。
 * 复用 [SujianEditorHost]，不创建第二个编辑器。
 */
@Composable
private fun EditorPane(
    documentState: WideWorkspaceDocumentState,
    modifier: Modifier = Modifier,
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
) {
    Box(
        modifier =
            modifier
                .background(MaterialTheme.colorScheme.background),
    ) {
        if (documentState.currentChapterId != null && documentState.currentVolumeId != null) {
            SujianEditorHost(
                projectId = documentState.currentProjectId,
                volumeId = documentState.currentVolumeId,
                chapterId = documentState.currentChapterId,
                chapterTitle = documentState.currentChapterTitle,
                modifier = Modifier.fillMaxSize(),
                onChapterSwitchFailed = onChapterSwitchFailed,
            )
        } else {
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                Text(stringResource(id = R.string.select_chapter_to_write))
            }
        }
    }
}
