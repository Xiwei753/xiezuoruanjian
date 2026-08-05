package com.xiwei.sujian.ui.compose.workspace

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
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
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
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
    modifier: Modifier = Modifier
) {
    val deps = com.xiwei.sujian.runtime.LocalSujianAppDependencies.current
    val workspaceRepository = deps.workspaceRepository

    val currentProjectId = appState.currentProjectId
    val currentVolumeId = appState.currentVolumeId
    val currentChapterId = appState.currentChapterId
    val currentChapterTitle = appState.currentChapterTitle


    // 平板/大屏：未选作品时全窗格作品列表，选中后多窗格工作台。
    // 手机竖屏由 PhoneWorkspaceHost 独立管理，不经过此路径。
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
