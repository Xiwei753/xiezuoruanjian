package com.xiwei.sujian.ui.compose.workspace

import android.os.Parcel
import android.os.Parcelable
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.PaneAdaptedValue
import androidx.compose.material3.adaptive.layout.ThreePaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.NavigableListDetailPaneScaffold
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.model.AvoidRegion
import com.xiwei.sujian.model.AvoidRegionKind
import com.xiwei.sujian.model.WorkspacePaneMode
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.adaptive.rememberCoreLayoutDirective
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val workspaceRepository = remember { WorkspaceRepository(context) }
    val coroutineScope = rememberCoroutineScope()

    val currentProjectId = appState.currentProjectId
    val currentVolumeId = appState.currentVolumeId
    val currentChapterId = appState.currentChapterId
    val currentChapterTitle = appState.currentChapterTitle
    val layoutPlan = appState.currentLayoutPlan

    if (currentProjectId == null) {
        ProjectListScreen(
            appState = appState,
            onSelectProject = { projectId, projectTitle ->
                appState.selectProject(projectId, projectTitle)
            },
            modifier = modifier
        )
        return
    }

    val visiblePaneRoles = layoutPlan?.visiblePaneRoles
    val editorContentMaxWidthDp = layoutPlan?.editorContentMaxWidthDp ?: 0f
    val pagePaddingDp = layoutPlan?.pagePaddingDp ?: 0f
    val avoidRegions = layoutPlan?.avoidRegions ?: emptyList()
    val paneMode = layoutPlan?.workspacePaneMode ?: WorkspacePaneMode.SinglePane
    val showProjectListInExtra = visiblePaneRoles?.showProjectList == true && paneMode == WorkspacePaneMode.ThreePane
    val listPaneWidthConstraint = layoutPlan?.listPaneWidth

    val coreDirective = rememberCoreLayoutDirective(layoutPlan)

    val windowInsetsPadding = computeWindowInsetPadding(avoidRegions)

    val navigator = rememberListDetailPaneScaffoldNavigator<WorkspaceDetailConfig>(
        scaffoldDirective = coreDirective
    )

    val isDetailExpanded = navigator.scaffoldValue[ThreePaneScaffoldRole.Secondary] == PaneAdaptedValue.Expanded

    BackHandler(enabled = navigator.canNavigateBack()) {
        coroutineScope.launch {
            navigator.navigateBack()
        }
        if (appState.currentChapterId != null) {
            appState.clearChapterSelection()
        }
    }

    BackHandler(enabled = !navigator.canNavigateBack() && currentProjectId != null) {
        appState.clearProjectSelection()
    }

    LaunchedEffect(currentChapterId) {
        if (currentChapterId != null && currentVolumeId != null) {
            if (!isDetailExpanded) {
                navigator.navigateTo(
                    ThreePaneScaffoldRole.Secondary,
                    WorkspaceDetailConfig(
                        volumeId = currentVolumeId,
                        chapterId = currentChapterId,
                        chapterTitle = currentChapterTitle
                    )
                )
            }
        }
    }

    val listPaneModifier = Modifier
        .fillMaxSize()
        .then(
            if (listPaneWidthConstraint != null && listPaneWidthConstraint.maxDp > 0f)
                Modifier.width(listPaneWidthConstraint.preferredDp.dp.coerceIn(
                    if (listPaneWidthConstraint.minDp > 0f) listPaneWidthConstraint.minDp.dp else 0.dp,
                    listPaneWidthConstraint.maxDp.dp
                ))
            else Modifier
        )
        .then(windowInsetsPadding)

    val hingePadding = computeHingePadding(avoidRegions)

    NavigableListDetailPaneScaffold(
        navigator = navigator,
        listPane = {
            AnimatedPane {
                if (visiblePaneRoles?.showChapterTree != false) {
                    VolumeChapterTree(
                        projectId = currentProjectId,
                        workspaceRepository = workspaceRepository,
                        onSelectChapter = { volumeId, chapterId, chapterTitle ->
                            appState.selectChapter(volumeId, chapterId, chapterTitle)
                            coroutineScope.launch {
                                navigator.navigateTo(
                                    ThreePaneScaffoldRole.Secondary,
                                    WorkspaceDetailConfig(
                                        volumeId = volumeId,
                                        chapterId = chapterId,
                                        chapterTitle = chapterTitle
                                    )
                                )
                            }
                        },
                        onBackToProjects = {
                            appState.clearProjectSelection()
                        },
                        modifier = listPaneModifier
                    )
                }
            }
        },
        detailPane = {
            AnimatedPane {
                if (visiblePaneRoles?.showEditor != false) {
                    val currentContent = navigator.currentDestination?.content
                    val detailVolumeId = currentContent?.volumeId ?: currentVolumeId
                    val detailChapterId = currentContent?.chapterId ?: currentChapterId
                    val detailChapterTitle = currentContent?.chapterTitle ?: currentChapterTitle

                    if (detailVolumeId != null && detailChapterId != null) {
                        val detailModifier = Modifier
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
                            .then(hingePadding)

                        SujianEditorHost(
                            projectId = currentProjectId,
                            volumeId = detailVolumeId,
                            chapterId = detailChapterId,
                            chapterTitle = detailChapterTitle,
                            modifier = detailModifier
                        )
                    } else {
                        Box(
                            modifier = Modifier.fillMaxSize().then(windowInsetsPadding),
                            contentAlignment = Alignment.Center
                        ) {
                            Text("选择章节开始写作", modifier = Modifier.padding(16.dp))
                        }
                    }
                }
            }
        },
        extraPane = {
            AnimatedPane {
                if (showProjectListInExtra) {
                    ProjectListScreen(
                        appState = appState,
                        onSelectProject = { projectId, projectTitle ->
                            appState.selectProject(projectId, projectTitle)
                            appState.clearChapterSelection()
                        },
                        modifier = Modifier
                            .fillMaxSize()
                            .then(windowInsetsPadding)
                    )
                }
            }
        },
        modifier = modifier.fillMaxSize()
    )
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
