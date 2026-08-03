package com.xiwei.sujian.ui.compose.workspace

import android.os.Parcel
import android.os.Parcelable
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import com.xiwei.sujian.designsystem.layout.SujianListDetailScope
import com.xiwei.sujian.designsystem.layout.SujianListDetailScaffoldWithNavigator
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.model.AvoidRegion
import com.xiwei.sujian.model.AvoidRegionKind
import com.xiwei.sujian.model.WorkspacePaneMode
import com.xiwei.sujian.platform.api.WindowSizeClass
import com.xiwei.sujian.ui.compose.LocalAndroidCapabilities
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.adaptive.rememberCoreLayoutDirective
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
import com.xiwei.sujian.ui.compose.workbench.component.SujianWorkbench
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
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

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    projectIdOverride: String? = null,
    volumeIdOverride: String? = null,
    chapterIdOverride: String? = null,
    onNavigateToProject: ((projectId: String) -> Unit)? = null,
    onNavigateToChapter: ((projectId: String, volumeId: String, chapterId: String) -> Unit)? = null,
    modifier: Modifier = Modifier
) {
    val coroutineScope = rememberCoroutineScope()
    val deps = com.xiwei.sujian.runtime.LocalSujianAppDependencies.current
    val workspaceRepository = deps.workspaceRepository

    val isRouteDriven = onNavigateToProject != null || onNavigateToChapter != null

    val currentProjectId = resolveEffectiveId(projectIdOverride, appState.currentProjectId, isRouteDriven)
    val currentVolumeId = resolveEffectiveId(volumeIdOverride, appState.currentVolumeId, isRouteDriven)
    val currentChapterId = resolveEffectiveId(chapterIdOverride, appState.currentChapterId, isRouteDriven)
    val currentChapterTitle = appState.currentChapterTitle
    val layoutPlan = appState.currentLayoutPlan

    if (currentProjectId == null) {
        ProjectListScreen(
            appState = appState,
            onSelectProject = { projectId, projectTitle ->
                if (onNavigateToProject != null) {
                    onNavigateToProject(projectId)
                } else {
                    appState.selectProject(projectId, projectTitle)
                }
            },
            modifier = modifier
        )
        return
    }

    val capabilities = LocalAndroidCapabilities.current
    val isTabletWindow = capabilities.windowSizeClass != WindowSizeClass.Compact

    if (isTabletWindow) {
        WorkbenchWorkspaceContent(
            appState = appState,
            currentProjectId = currentProjectId,
            currentVolumeId = currentVolumeId,
            currentChapterId = currentChapterId,
            currentChapterTitle = currentChapterTitle,
            workspaceRepository = workspaceRepository,
            onNavigateToProject = onNavigateToProject,
            onNavigateToChapter = onNavigateToChapter,
            modifier = modifier,
        )
    } else {
        CompactWorkspaceContent(
            appState = appState,
            currentProjectId = currentProjectId,
            currentVolumeId = currentVolumeId,
            currentChapterId = currentChapterId,
            currentChapterTitle = currentChapterTitle,
            layoutPlan = layoutPlan,
            workspaceRepository = workspaceRepository,
            onNavigateToProject = onNavigateToProject,
            onNavigateToChapter = onNavigateToChapter,
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
    onNavigateToProject: ((projectId: String) -> Unit)?,
    onNavigateToChapter: ((projectId: String, volumeId: String, chapterId: String) -> Unit)?,
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
                        if (onNavigateToChapter != null) {
                            onNavigateToChapter(currentProjectId, volumeId, chapterId)
                        } else {
                            appState.selectChapter(volumeId, chapterId, chapterTitle)
                        }
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

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun CompactWorkspaceContent(
    appState: SujianAppState,
    currentProjectId: String,
    currentVolumeId: String?,
    currentChapterId: String?,
    currentChapterTitle: String,
    layoutPlan: com.xiwei.sujian.model.LayoutPlan?,
    workspaceRepository: com.xiwei.sujian.data.WorkspaceRepository,
    onNavigateToProject: ((projectId: String) -> Unit)?,
    onNavigateToChapter: ((projectId: String, volumeId: String, chapterId: String) -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current

    val visiblePaneRoles = layoutPlan?.visiblePaneRoles
    val editorContentMaxWidthDp = layoutPlan?.editorContentMaxWidthDp ?: 0f
    val pagePaddingDp = layoutPlan?.pagePaddingDp ?: 0f
    val avoidRegions = layoutPlan?.avoidRegions ?: emptyList()

    val windowInsetsPadding = computeWindowInsetPadding(avoidRegions)

    if (onNavigateToChapter == null) {
        BackHandler(enabled = currentChapterId != null) {
            appState.clearChapterSelection()
        }

        BackHandler(enabled = currentChapterId == null) {
            appState.clearProjectSelection()
        }
    }

    if (currentChapterId != null && currentVolumeId != null && visiblePaneRoles?.showEditor != false) {
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
            modifier = modifier.then(editorModifier),
        )
    } else {
        if (visiblePaneRoles?.showChapterTree != false) {
            VolumeChapterTree(
                projectId = currentProjectId,
                workspaceRepository = workspaceRepository,
                onSelectChapter = { volumeId, chapterId, chapterTitle ->
                    appState.selectChapter(volumeId, chapterId, chapterTitle)
                    onNavigateToChapter?.invoke(currentProjectId, volumeId, chapterId)
                },
                onBackToProjects = {
                    appState.clearProjectSelection()
                },
                modifier = modifier.then(windowInsetsPadding)
            )
        }
    }
}

@Suppress("DEPRECATION")
private class WorkspaceDetailConfig(
    val volumeId: String,
    val chapterId: String,
    val chapterTitle: String
) : Parcelable {
    constructor(parcel: Parcel) : this(
        parcel.readString() ?: "",
        parcel.readString() ?: "",
        parcel.readString() ?: ""
    )

    override fun writeToParcel(parcel: Parcel, flags: Int) {
        parcel.writeString(volumeId)
        parcel.writeString(chapterId)
        parcel.writeString(chapterTitle)
    }

    override fun describeContents(): Int = 0

    companion object CREATOR : Parcelable.Creator<WorkspaceDetailConfig> {
        override fun createFromParcel(parcel: Parcel): WorkspaceDetailConfig =
            WorkspaceDetailConfig(parcel)
        override fun newArray(size: Int): Array<WorkspaceDetailConfig?> =
            arrayOfNulls(size)
    }
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

@Composable
private fun computeHingePadding(avoidRegions: List<AvoidRegion>): Modifier {
    val horizontalHinges = avoidRegions.filter { it.kind == AvoidRegionKind.HorizontalHinge }
    if (horizontalHinges.isEmpty()) return Modifier
    var topDp = 0f
    var bottomDp = 0f
    for (hinge in horizontalHinges) {
        val height = hinge.bottomDp - hinge.topDp
        if (height > 0f) {
            topDp = maxOf(topDp, hinge.topDp)
            bottomDp = maxOf(bottomDp, hinge.bottomDp - hinge.topDp)
        }
    }
    if (topDp == 0f && bottomDp == 0f) return Modifier
    return Modifier.padding(top = topDp.dp, bottom = bottomDp.dp)
}

internal fun resolveEffectiveId(
    overrideValue: String?,
    appStateValue: String?,
    isRouteDriven: Boolean
): String? = if (isRouteDriven) overrideValue else (overrideValue ?: appStateValue)
