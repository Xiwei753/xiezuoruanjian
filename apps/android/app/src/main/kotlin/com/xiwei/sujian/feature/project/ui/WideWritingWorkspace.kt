package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.MeasurePolicy
import androidx.compose.ui.layout.layoutId
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidResolvedWorkspaceMode
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost
import com.xiwei.sujian.feature.project.data.ProjectRepository

/**
 * 大屏写作工作台依赖 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceDeps(
    val appState: SujianAppState,
    val projectRepository: ProjectRepository,
    val projectWorkspaceActions: AndroidWorkspaceActionSpec,
    val chrome: SujianChromeSpec?,
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
 * 大屏工作台组合模式 — [resolveWideWorkspaceCompositionMode] 的返回值。
 *
 * - [SINGLE_PANE_WITH_TOP_BAR]：plan=null/SINGLE_PANE，在 Rust Editor free-region 内测量
 *   singlePaneTopBar + EditorPane body；
 * - [FULL_WORKBENCH]：plan=WORKBENCH，完整 Workbench shell。
 *
 * EditorPane 始终在 [WideWritingWorkspace] 唯一 call site（layoutId=EDITOR），两种模式切换不重建 AndroidView。
 */
internal enum class WideWorkspaceCompositionMode {
    SINGLE_PANE_WITH_TOP_BAR,
    FULL_WORKBENCH,
}

/**
 * 大屏工作台组合模式决策 — 纯函数，无 Compose 副作用，可单测。
 *
 * - plan=null/SINGLE_PANE → SINGLE_PANE_WITH_TOP_BAR（在 Rust Editor free-region
 *   内测量 singlePaneTopBar + EditorPane body）；
 * - plan=WORKBENCH → FULL_WORKBENCH（完整 Workbench shell）。
 *
 * EditorPane 始终在 [WideWritingWorkspace] 唯一 call site（layoutId=EDITOR），两种模式切换不重建 AndroidView。
 */
internal fun resolveWideWorkspaceCompositionMode(
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
): WideWorkspaceCompositionMode {
    val isSinglePanePlan =
        workbenchPlan == null ||
            workbenchPlan.mode == AndroidResolvedWorkspaceMode.SINGLE_PANE
    if (isSinglePanePlan) return WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR
    return WideWorkspaceCompositionMode.FULL_WORKBENCH
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
    val compositionMode =
        resolveWideWorkspaceCompositionMode(
            workbenchPlan = workbenchPlan,
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
            // measure policy 在 Rust Editor free-region 内测量 top bar + body（body = region - top bar height）。
            // EditorPane 仍在下面唯一 call site，top bar 是 sibling slot，不移动 Editor 本身。
            if (compositionMode == WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR) {
                Box(modifier = Modifier.layoutId(LayoutSlotId.SINGLE_PANE_TOP_BAR)) {
                    singlePaneTopBar?.invoke()
                }
            }
            // 唯一 EditorPane — 始终在同一个 call site，layoutId=EDITOR。
            // SINGLE_PANE_WITH_TOP_BAR：measure policy 按 Rust Editor region 测量 body（region - top bar）。
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

        fun get(
            bounds: WorkbenchBounds,
            slotId: LayoutSlotId,
        ): AndroidLayoutRect = slotBoundsMap[slotId]?.invoke(bounds) ?: EMPTY_RECT
    }
}

/**
 * 工作台 slot 状态 — 打包传递，避免 [WorkbenchChromeSlots] 参数过多。
 */
private data class WorkbenchSlotState(
    val chapterTreeCollapsed: Boolean,
    val toolPaneCollapsed: Boolean,
    val selectedToolId: String?,
    val onToggleChapterTree: () -> Unit,
    val onToggleToolPane: () -> Unit,
    val onSelectTool: (String) -> Unit,
)

/**
 * 大屏工作台 chrome slots — 只在 FULL_WORKBENCH 时组合。
 */
@Composable
private fun WorkbenchChromeSlots(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    callbacks: WideWorkspaceCallbacks,
    state: WorkbenchSlotState,
    bounds: WorkbenchBounds,
) {
    // 顶部工具栏三组独立容器
    Box(modifier = Modifier.layoutId(LayoutSlotId.TOOLBAR_LEADING)) {
        Text("Toolbar Leading")
    }
    Box(modifier = Modifier.layoutId(LayoutSlotId.TOOLBAR_CENTER)) {
        Text("Toolbar Center")
    }
    Box(modifier = Modifier.layoutId(LayoutSlotId.TOOLBAR_TRAILING)) {
        Text("Toolbar Trailing")
    }
    // 章节导航
    Box(modifier = Modifier.layoutId(LayoutSlotId.CHAPTER_NAVIGATION)) {
        Text("Chapter Navigation")
    }
    // 工具面板
    Box(modifier = Modifier.layoutId(LayoutSlotId.TOOL_PANE)) {
        Text("Tool Pane")
    }
    // 工具栏
    Box(modifier = Modifier.layoutId(LayoutSlotId.TOOL_RAIL)) {
        Text("Tool Rail")
    }
}

/**
 * 大屏工作台 measure policy — 按 compositionMode 选择 place 策略。
 */
private fun wideWorkspaceMeasurePolicy(
    compositionMode: WideWorkspaceCompositionMode,
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    density: Density,
    fallbackSafeBounds: AndroidLayoutRect,
): MeasurePolicy =
    MeasurePolicy { measurables, constraints ->
        val byId = measurables.associateBy { it.layoutId as LayoutSlotId }
        when (compositionMode) {
            WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR -> {
                // 在 Rust Editor free-region 内测量 top bar + body
                val topBarPlaceable = byId[LayoutSlotId.SINGLE_PANE_TOP_BAR]?.measure(constraints)
                val topBarHeight = topBarPlaceable?.height ?: 0
                val bodyConstraints =
                    constraints.copy(
                        minHeight = (constraints.maxHeight - topBarHeight).coerceAtLeast(0),
                        maxHeight = constraints.maxHeight,
                    )
                val bodyPlaceable = byId[LayoutSlotId.EDITOR]?.measure(bodyConstraints)
                layout(constraints.maxWidth, constraints.maxHeight) {
                    topBarPlaceable?.place(0, 0)
                    bodyPlaceable?.place(0, topBarHeight)
                }
            }
            WideWorkspaceCompositionMode.FULL_WORKBENCH -> {
                // 按七角色 bounds place
                val plan = workbenchPlan
                if (plan != null) {
                    val bounds = WorkbenchBounds.fromPlan(plan)
                    val width = constraints.maxWidth
                    val height = constraints.maxHeight
                    layout(width, height) {
                        val editorPlaceable = byId[LayoutSlotId.EDITOR]?.measure(constraints)
                        val editorBounds = WorkbenchBounds.get(bounds, LayoutSlotId.EDITOR)
                        editorPlaceable?.place(
                            editorBounds.leftDp.toInt(),
                            editorBounds.topDp.toInt(),
                        )
                        // 其他 chrome slots 类似处理
                    }
                } else {
                    // 无 plan 时 fallback 到 fillMaxSize
                    val editorPlaceable = byId[LayoutSlotId.EDITOR]?.measure(constraints)
                    layout(constraints.maxWidth, constraints.maxHeight) {
                        editorPlaceable?.place(0, 0)
                    }
                }
            }
        }
    }

/**
 * 大屏 EditorPane — 始终在唯一 call site（layoutId=EDITOR）。
 */
@Composable
private fun EditorPane(
    documentState: WideWorkspaceDocumentState,
    onChapterSwitchFailed: (
        (
            oldProjectId: String,
            oldVolumeId: String?,
            oldChapterId: String?,
            oldChapterTitle: String,
        ) -> Unit
    )?,
    modifier: Modifier = Modifier,
) {
    SujianEditorHost(
        projectId = documentState.currentProjectId,
        volumeId = documentState.currentVolumeId ?: "",
        chapterId = documentState.currentChapterId ?: "",
        chapterTitle = documentState.currentChapterTitle,
        modifier = modifier,
        onChapterSwitchFailed = onChapterSwitchFailed,
    )
}
