package com.xiwei.sujian.ui.compose.workspace

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.model.AvoidRegion
import com.xiwei.sujian.model.AvoidRegionKind
import com.xiwei.sujian.platform.api.WindowSizeClass
import com.xiwei.sujian.ui.compose.LocalAndroidCapabilities
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
import com.xiwei.sujian.ui.compose.navigation.WorkspaceBackState
import com.xiwei.sujian.ui.compose.navigation.predictiveBackStateFraction
import com.xiwei.sujian.ui.compose.workbench.component.SujianWorkbench
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.panel.AiAssistantPanel
import com.xiwei.sujian.ui.compose.workbench.panel.ChapterNavigatorPanel
import com.xiwei.sujian.ui.compose.workbench.panel.ProjectNavigatorPanel
import com.xiwei.sujian.ui.compose.workbench.panel.SearchPanel
import com.xiwei.sujian.ui.compose.workbench.panel.StarMapPanel
import com.xiwei.sujian.ui.compose.workbench.panel.StatisticsPanel
import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchLayoutRepository
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchViewModel
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * 写作工作区 — 「作品」一级入口的唯一内容。
 *
 * 作品、卷、章节和正文是同一个工作区内的不同数据和窗格状态：
 * - 当前选择（project/volume/chapter）由 [SujianAppState] 单状态持有，不创建全局 route。
 * - 手机：作品列表 → 章节树 → 正文 依次为工作区内部窗格，返回只切工作区内部窗格。
 * - 平板/大屏：正文为主窗格，作品/章节导航与辅助面板由 [SujianWorkbench] 组合。
 */
@Composable
fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    workspaceBackState: WorkspaceBackState,
    modifier: Modifier = Modifier
) {
    val deps = com.xiwei.sujian.runtime.LocalSujianAppDependencies.current
    val workspaceRepository = deps.workspaceRepository

    val currentProjectId = appState.currentProjectId
    val currentVolumeId = appState.currentVolumeId
    val currentChapterId = appState.currentChapterId
    val currentChapterTitle = appState.currentChapterTitle
    val layoutPlan = appState.currentLayoutPlan

    val capabilities = LocalAndroidCapabilities.current
    val isTabletWindow = capabilities.windowSizeClass != WindowSizeClass.Compact

    if (isTabletWindow) {
        // 平板/大屏：未选作品时全窗格作品列表，选中后多窗格工作台。
        if (currentProjectId == null) {
            ProjectListScreen(
                appState = appState,
                onSelectProject = { projectId, projectTitle ->
                    appState.selectProject(projectId, projectTitle)
                },
                modifier = modifier
            )
        } else {
            WorkbenchWorkspaceContent(
                appState = appState,
                currentProjectId = currentProjectId,
                currentVolumeId = currentVolumeId,
                currentChapterId = currentChapterId,
                currentChapterTitle = currentChapterTitle,
                workspaceRepository = workspaceRepository,
                modifier = modifier,
            )
        }
    } else {
        // 手机模式：无论是否选择作品/章节，都只从这一个固定调用位置组合一次
        // CompactWorkspaceContent。选择作品/章节只改变工作区状态与 navigator
        // 内部历史，不重建 composition 调用链，返回历史与预测返回始终有效。
        CompactWorkspaceContent(
            appState = appState,
            currentProjectId = currentProjectId,
            currentVolumeId = currentVolumeId,
            currentChapterId = currentChapterId,
            currentChapterTitle = currentChapterTitle,
            layoutPlan = layoutPlan,
            workspaceRepository = workspaceRepository,
            workspaceBackState = workspaceBackState,
            modifier = modifier,
        )
    }
}

@Composable
private fun WorkbenchWorkspaceContent(
    appState: SujianAppState,
    currentProjectId: String,
    currentVolumeId: String?,
    currentChapterId: String?,
    currentChapterTitle: String,
    workspaceRepository: com.xiwei.sujian.data.WorkspaceRepository,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val configuration = LocalConfiguration.current
    val dims = LocalSujianDimensions.current
    val workbenchVm: WorkbenchViewModel = viewModel()

    val widthDp = configuration.screenWidthDp
    val heightDp = configuration.screenHeightDp
    val orientation = if (configuration.orientation == android.content.res.Configuration.ORIENTATION_LANDSCAPE) "landscape" else "portrait"
    val widthBucket = WindowWidthBucket.fromDp(widthDp)

    val deviceId = remember {
        try {
            val prefs = context.getSharedPreferences("sujian_device", android.content.Context.MODE_PRIVATE)
            prefs.getString("device_id", "unknown") ?: "unknown"
        } catch (_: Exception) { "unknown" }
    }

    val storageKey = remember(deviceId, orientation, widthBucket) {
        LayoutStorageKey(
            deviceId = deviceId,
            orientation = orientation,
            windowWidthBucket = widthBucket,
            windowMode = "standard",
        )
    }

    val previousStorageKey = remember { mutableStateOf<LayoutStorageKey?>(null) }
    LaunchedEffect(storageKey) {
        if (previousStorageKey.value == null) {
            val repo = WorkbenchLayoutRepository(context)
            workbenchVm.initialize(repo, storageKey)
        } else if (previousStorageKey.value != storageKey) {
            workbenchVm.onWindowBucketChanged(storageKey)
        }
        previousStorageKey.value = storageKey
    }

    val layoutState = workbenchVm.layoutState

    val avoidRegions = appState.currentLayoutPlan?.avoidRegions ?: emptyList()
    val windowInsetsPadding = computeWindowInsetPadding(avoidRegions)
    val editorContentMaxWidthDp = appState.currentLayoutPlan?.editorContentMaxWidthDp ?: 0f
    val pagePaddingDp = appState.currentLayoutPlan?.pagePaddingDp ?: 0f

    SujianWorkbench(
        layoutState = layoutState,
        onAction = { action ->
            when (action) {
                is WorkbenchAction.MoveFloatingPanel,
                is WorkbenchAction.ResizeFloatingPanel,
                is WorkbenchAction.ResizeDockSplit,
                is WorkbenchAction.ResizeDockZone -> workbenchVm.dispatchDeferredPersist(action)
                else -> workbenchVm.dispatch(action)
            }
        },
        onWindowSizeChanged = { maxWidthDp, maxHeightDp ->
            workbenchVm.onWindowSizeChanged(maxWidthDp, maxHeightDp)
        },
        modifier = modifier.fillMaxSize().then(windowInsetsPadding),
        editorContent = {
            if (currentVolumeId != null && currentChapterId != null) {
                val editorModifier = Modifier
                    .fillMaxSize()
                    .then(
                        if (editorContentMaxWidthDp > 0f) Modifier.width(editorContentMaxWidthDp.dp)
                        else Modifier
                    )
                    .then(
                        if (pagePaddingDp > 0f) Modifier.padding(horizontal = pagePaddingDp.dp)
                        else Modifier
                    )

                SujianEditorHost(
                    projectId = currentProjectId,
                    volumeId = currentVolumeId,
                    chapterId = currentChapterId,
                    chapterTitle = currentChapterTitle,
                    modifier = editorModifier,
                )
            } else {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        stringResource(id = R.string.select_chapter_to_write),
                        modifier = Modifier.padding(dims.space16),
                    )
                }
            }
        },
        panelContent = { panelState ->
            when (panelState.id) {
                WorkbenchPanelId.ProjectNavigator -> ProjectNavigatorPanel(
                    appState = appState,
                    onSelectProject = { projectId, projectTitle ->
                        appState.selectProject(projectId, projectTitle)
                        appState.clearChapterSelection()
                    },
                )
                WorkbenchPanelId.ChapterNavigator -> ChapterNavigatorPanel(
                    projectId = currentProjectId,
                    workspaceRepository = workspaceRepository,
                    onSelectChapter = { volumeId, chapterId, chapterTitle ->
                        appState.selectChapter(volumeId, chapterId, chapterTitle)
                    },
                    onBackToProjects = {
                        appState.clearProjectSelection()
                    },
                )
                WorkbenchPanelId.AiAssistant -> AiAssistantPanel(
                    projectId = currentProjectId,
                    volumeId = currentVolumeId,
                    chapterId = currentChapterId,
                )
                WorkbenchPanelId.Search -> SearchPanel()
                WorkbenchPanelId.Statistics -> StatisticsPanel()
                WorkbenchPanelId.StarMap -> StarMapPanel()
                WorkbenchPanelId.DocumentOutline,
                WorkbenchPanelId.CharacterInfo -> {
                    Box(
                        modifier = Modifier.fillMaxSize(),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = panelState.id.name,
                            style = androidx.compose.material3.MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
        },
    )
}

/**
 * 工作区内部窗格（仅手机单窗格使用）。
 *
 * 作品列表 / 章节树 / 正文是同一个工作区内的不同数据窗格状态，由同一个
 * Material3 Adaptive 列表—详情 navigator 管理：系统返回、顶栏返回与章节树
 * 自带返回都调用同一套窗格转换；预测返回手势按 BackEvent.progress 真实
 * seek 过渡（拖动跟手、取消回原位、提交完成剩余动画）。
 */
private enum class WorkspacePaneKey {
    ProjectList,
    ChapterTree,
    Editor,
}

private val WorkspacePaneKey.role: androidx.compose.material3.adaptive.layout.ThreePaneScaffoldRole
    get() = when (this) {
        WorkspacePaneKey.ProjectList -> ListDetailPaneScaffoldRole.List
        WorkspacePaneKey.ChapterTree -> ListDetailPaneScaffoldRole.Detail
        WorkspacePaneKey.Editor -> ListDetailPaneScaffoldRole.Extra
    }

private val WorkspacePaneKey.paneName: String
    get() = when (this) {
        WorkspacePaneKey.ProjectList -> "project_list"
        WorkspacePaneKey.ChapterTree -> "chapter_tree"
        WorkspacePaneKey.Editor -> "editor"
    }

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun CompactWorkspaceContent(
    appState: SujianAppState,
    currentProjectId: String?,
    currentVolumeId: String?,
    currentChapterId: String?,
    currentChapterTitle: String,
    layoutPlan: com.xiwei.sujian.model.LayoutPlan?,
    workspaceRepository: com.xiwei.sujian.data.WorkspaceRepository,
    workspaceBackState: WorkspaceBackState,
    modifier: Modifier = Modifier,
) {
    val coroutineScope = rememberCoroutineScope()

    // navigator 初始历史按当前工作区选择建立完整链条（普通进入、进程恢复、横竖屏/
    // 窗口形态切换后返回历史始终完整，不把当前窗格当作新根）：
    // - 作品列表：ProjectList
    // - 章节树：ProjectList → ChapterTree
    // - 正文：ProjectList → ChapterTree → Editor
    // 选择作品/章节只在该 navigator 实例内 navigateTo 追加历史；实例在条件分支重建
    // 前由本函数唯一调用位置持有，跨选择不销毁。
    val initialHistory = remember {
        val chain = mutableListOf(
            ThreePaneScaffoldDestinationItem(WorkspacePaneKey.ProjectList.role, WorkspacePaneKey.ProjectList),
        )
        if (appState.currentProjectId != null) {
            chain += ThreePaneScaffoldDestinationItem(WorkspacePaneKey.ChapterTree.role, WorkspacePaneKey.ChapterTree)
            if (appState.currentChapterId != null && appState.currentVolumeId != null) {
                chain += ThreePaneScaffoldDestinationItem(WorkspacePaneKey.Editor.role, WorkspacePaneKey.Editor)
            }
        }
        chain
    }
    val navigator = rememberListDetailPaneScaffoldNavigator<WorkspacePaneKey>(
        initialDestinationHistory = initialHistory,
    )

    // 先播放完窗格退出动画，再回写工作区选择状态；避免“画面不动、松手后突然切换”，
    // 也保证顶栏返回与系统返回走完全相同的窗格转换。
    suspend fun backOnePaneAndWriteBack() {
        // 历史不足两步（如选中作品后、窗格动画尚未完成时立即按返回）不弹栈，
        // 直接回写选择；避免 navigateBack 在无前驱时清空历史导致当前窗格失步。
        if (navigator.canNavigateBack()) {
            navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
        }
        when (navigator.currentDestination?.contentKey) {
            WorkspacePaneKey.ProjectList -> {
                if (appState.currentProjectId != null) {
                    appState.clearProjectSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("project_list")
                }
            }
            WorkspacePaneKey.ChapterTree -> {
                if (appState.currentChapterId != null) {
                    appState.clearChapterSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("chapter_tree")
                }
            }
            // 快速连按导致历史被清空（navigator 已回根）：同步清空选择，避免
            // 作品列表窗格与顶栏标题/返回按钮失步。
            null -> {
                if (appState.currentProjectId != null) {
                    appState.clearProjectSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("project_list")
                }
            }
            // 仍在编辑窗格（未发生弹栈）：无需回写。
            WorkspacePaneKey.Editor -> {}
        }
    }
    val navigateBackWithWriteBack: () -> Unit = {
        coroutineScope.launch {
            backOnePaneAndWriteBack()
        }
    }

    // 顶栏返回按钮：根壳通过该入口调用与系统返回完全相同的窗格转换。
    LaunchedEffect(navigator) {
        workspaceBackState.update(navigateBackWithWriteBack)
    }
    DisposableEffect(workspaceBackState) {
        onDispose {
            workspaceBackState.update(null)
        }
    }

    // 预测返回（系统返回手势）：拖动时按 BackEvent.progress 真实 seek 内部窗格
    // 过渡（当前窗格跟手退出、后窗格同步显露）；取消回到原位；提交后播放剩余
    // 过渡，动画完成后才回写选择状态（无手势结束跳变）。未选中作品时不注册
    // （返回交给全局 NavDisplay，Works 是栈底，由系统收尾）。
    PredictiveBackHandler(enabled = navigator.canNavigateBack()) { progressEvents ->
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack(
            navigator.currentDestination?.contentKey?.paneName ?: "workspace",
            "start",
        )
        try {
            progressEvents.collect { event ->
                if (event.progress != 0f) {
                    navigator.seekBack(
                        BackNavigationBehavior.PopUntilScaffoldValueChange,
                        predictiveBackStateFraction(event.progress),
                    )
                }
            }
            backOnePaneAndWriteBack()
        } catch (e: CancellationException) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("workspace", "cancel")
            withContext(NonCancellable) {
                navigator.seekBack(BackNavigationBehavior.PopUntilScaffoldValueChange, 0f)
            }
            throw e
        }
    }

    val editorContentMaxWidthDp = layoutPlan?.editorContentMaxWidthDp ?: 0f
    val pagePaddingDp = layoutPlan?.pagePaddingDp ?: 0f
    val avoidRegions = layoutPlan?.avoidRegions ?: emptyList()

    val windowInsetsPadding = computeWindowInsetPadding(avoidRegions)

    // 同一 navigator 的列表—详情窗格：三个窗格共享同一份返回历史与过渡状态，
    // 预测返回、普通返回和顶栏返回全部复用同一套转换。
    ListDetailPaneScaffold(
        modifier = modifier,
        directive = navigator.scaffoldDirective,
        scaffoldState = navigator.scaffoldState,
        listPane = {
            AnimatedPane {
                ProjectListScreen(
                    appState = appState,
                    onSelectProject = { projectId, projectTitle ->
                        appState.selectProject(projectId, projectTitle)
                        coroutineScope.launch {
                            navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, WorkspacePaneKey.ChapterTree)
                        }
                    },
                )
            }
        },
        detailPane = {
            AnimatedPane {
                if (currentProjectId != null) {
                    VolumeChapterTree(
                        projectId = currentProjectId,
                        workspaceRepository = workspaceRepository,
                        onSelectChapter = { volumeId, chapterId, chapterTitle ->
                            appState.selectChapter(volumeId, chapterId, chapterTitle)
                            coroutineScope.launch {
                                navigator.navigateTo(ListDetailPaneScaffoldRole.Extra, WorkspacePaneKey.Editor)
                            }
                        },
                        onBackToProjects = navigateBackWithWriteBack,
                        modifier = Modifier.then(windowInsetsPadding),
                    )
                }
            }
        },
        extraPane = {
            AnimatedPane {
                if (currentProjectId != null && currentChapterId != null && currentVolumeId != null) {
                    val editorModifier = Modifier
                        .fillMaxSize()
                        .then(
                            if (editorContentMaxWidthDp > 0f) Modifier.width(editorContentMaxWidthDp.dp)
                            else Modifier
                        )
                        .then(
                            if (pagePaddingDp > 0f) Modifier.padding(horizontal = pagePaddingDp.dp)
                            else Modifier
                        )
                        .then(windowInsetsPadding)

                    SujianEditorHost(
                        projectId = currentProjectId,
                        volumeId = currentVolumeId,
                        chapterId = currentChapterId,
                        chapterTitle = currentChapterTitle,
                        modifier = editorModifier,
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
        },
    )
}

@Composable
private fun computeWindowInsetPadding(avoidRegions: List<AvoidRegion>): Modifier {
    val insets = avoidRegions.filter { it.kind == AvoidRegionKind.WindowInset }
    if (insets.isEmpty()) return Modifier
    var startDp = 0f
    var endDp = 0f
    var topDp = 0f
    var bottomDp = 0f
    for (region in insets) {
        if (region.leftDp > 0f) startDp = maxOf(startDp, region.leftDp)
        if (region.rightDp > 0f) endDp = maxOf(endDp, region.rightDp)
        if (region.topDp > 0f) topDp = maxOf(topDp, region.topDp)
        if (region.bottomDp > 0f) bottomDp = maxOf(bottomDp, region.bottomDp)
    }
    if (startDp == 0f && endDp == 0f && topDp == 0f && bottomDp == 0f) return Modifier
    return Modifier.padding(
        start = startDp.dp,
        end = endDp.dp,
        top = topDp.dp,
        bottom = bottomDp.dp
    )
}
