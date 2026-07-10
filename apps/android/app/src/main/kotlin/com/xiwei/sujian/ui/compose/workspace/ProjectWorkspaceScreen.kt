package com.xiwei.sujian.ui.compose.workspace

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.model.AvoidRegion
import com.xiwei.sujian.model.WidthClass
import com.xiwei.sujian.model.WorkspacePaneMode
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
import com.xiwei.sujian.ui.compose.SujianAppState

@Composable
fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val workspaceRepository = remember { WorkspaceRepository(context) }

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

    val paneMode = layoutPlan?.workspacePaneMode ?: WorkspacePaneMode.SinglePane
    val listPaneWidth = layoutPlan?.listPaneWidth
    val editorContentMaxWidthDp = layoutPlan?.editorContentMaxWidthDp ?: 0f
    val pagePaddingDp = layoutPlan?.pagePaddingDp ?: 0f
    val avoidRegions = layoutPlan?.avoidRegions ?: emptyList()
    val visiblePaneRoles = layoutPlan?.visiblePaneRoles

    val listWidth = if (listPaneWidth != null && listPaneWidth.preferredDp > 0f) {
        listPaneWidth.preferredDp.dp
    } else {
        280.dp
    }

    when (paneMode) {
        WorkspacePaneMode.ThreePane -> {
            ThreePaneLayout(
                appState = appState,
                currentProjectId = currentProjectId,
                currentVolumeId = currentVolumeId,
                currentChapterId = currentChapterId,
                currentChapterTitle = currentChapterTitle,
                workspaceRepository = workspaceRepository,
                listWidth = listWidth,
                editorContentMaxWidthDp = editorContentMaxWidthDp,
                pagePaddingDp = pagePaddingDp,
                avoidRegions = avoidRegions,
                modifier = modifier
            )
        }
        WorkspacePaneMode.ListDetail -> {
            ListDetailLayout(
                appState = appState,
                currentProjectId = currentProjectId,
                currentVolumeId = currentVolumeId,
                currentChapterId = currentChapterId,
                currentChapterTitle = currentChapterTitle,
                workspaceRepository = workspaceRepository,
                listWidth = listWidth,
                editorContentMaxWidthDp = editorContentMaxWidthDp,
                pagePaddingDp = pagePaddingDp,
                avoidRegions = avoidRegions,
                modifier = modifier
            )
        }
        else -> {
            SinglePaneLayout(
                appState = appState,
                currentProjectId = currentProjectId,
                currentVolumeId = currentVolumeId,
                currentChapterId = currentChapterId,
                currentChapterTitle = currentChapterTitle,
                workspaceRepository = workspaceRepository,
                modifier = modifier
            )
        }
    }
}

@Composable
private fun ThreePaneLayout(
    appState: SujianAppState,
    currentProjectId: String,
    currentVolumeId: String?,
    currentChapterId: String?,
    currentChapterTitle: String,
    workspaceRepository: WorkspaceRepository,
    listWidth: androidx.compose.ui.unit.Dp,
    editorContentMaxWidthDp: Float,
    pagePaddingDp: Float,
    avoidRegions: List<AvoidRegion>,
    modifier: Modifier = Modifier
) {
    val projectListWidth = (listWidth.value * 0.7f).dp

    Row(modifier = modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .width(projectListWidth)
                .fillMaxHeight()
        ) {
            ProjectListScreen(
                appState = appState,
                onSelectProject = { projectId, projectTitle ->
                    appState.selectProject(projectId, projectTitle)
                    appState.clearChapterSelection()
                },
                modifier = Modifier.fillMaxSize()
            )
        }
        Box(
            modifier = Modifier
                .width(listWidth)
                .fillMaxHeight()
        ) {
            VolumeChapterTree(
                projectId = currentProjectId,
                workspaceRepository = workspaceRepository,
                onSelectChapter = { volumeId, chapterId, chapterTitle ->
                    appState.selectChapter(volumeId, chapterId, chapterTitle)
                },
                onBackToProjects = {},
                modifier = Modifier.fillMaxSize()
            )
        }
        Box(modifier = Modifier.weight(1f).fillMaxHeight()) {
            if (currentVolumeId != null && currentChapterId != null) {
                SujianEditorHost(
                    projectId = currentProjectId,
                    volumeId = currentVolumeId,
                    chapterId = currentChapterId,
                    chapterTitle = currentChapterTitle,
                    modifier = Modifier
                        .fillMaxSize()
                        .then(
                            if (editorContentMaxWidthDp > 0f) Modifier.width(editorContentMaxWidthDp.dp)
                            else Modifier
                        )
                        .then(
                            if (pagePaddingDp > 0f) Modifier.padding(horizontal = pagePaddingDp.dp)
                            else Modifier
                        )
                )
            } else {
                Box(modifier = Modifier.fillMaxSize()) {
                    Text("选择章节开始写作", modifier = Modifier.padding(16.dp))
                }
            }
        }
    }
}

@Composable
private fun ListDetailLayout(
    appState: SujianAppState,
    currentProjectId: String,
    currentVolumeId: String?,
    currentChapterId: String?,
    currentChapterTitle: String,
    workspaceRepository: WorkspaceRepository,
    listWidth: androidx.compose.ui.unit.Dp,
    editorContentMaxWidthDp: Float,
    pagePaddingDp: Float,
    avoidRegions: List<AvoidRegion>,
    modifier: Modifier = Modifier
) {
    Row(modifier = modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .width(listWidth)
                .fillMaxHeight()
        ) {
            VolumeChapterTree(
                projectId = currentProjectId,
                workspaceRepository = workspaceRepository,
                onSelectChapter = { volumeId, chapterId, chapterTitle ->
                    appState.selectChapter(volumeId, chapterId, chapterTitle)
                },
                onBackToProjects = {
                    appState.clearChapterSelection()
                    appState.currentProjectId = null
                    appState.currentProjectTitle = ""
                },
                modifier = Modifier.fillMaxSize()
            )
        }
        Box(modifier = Modifier.weight(1f).fillMaxHeight()) {
            if (currentVolumeId != null && currentChapterId != null) {
                SujianEditorHost(
                    projectId = currentProjectId,
                    volumeId = currentVolumeId,
                    chapterId = currentChapterId,
                    chapterTitle = currentChapterTitle,
                    modifier = Modifier
                        .fillMaxSize()
                        .then(
                            if (editorContentMaxWidthDp > 0f) Modifier.width(editorContentMaxWidthDp.dp)
                            else Modifier
                        )
                        .then(
                            if (pagePaddingDp > 0f) Modifier.padding(horizontal = pagePaddingDp.dp)
                            else Modifier
                        )
                )
            } else {
                Box(modifier = Modifier.fillMaxSize()) {
                    Text("选择章节开始写作", modifier = Modifier.padding(16.dp))
                }
            }
        }
    }
}

@Composable
private fun SinglePaneLayout(
    appState: SujianAppState,
    currentProjectId: String,
    currentVolumeId: String?,
    currentChapterId: String?,
    currentChapterTitle: String,
    workspaceRepository: WorkspaceRepository,
    modifier: Modifier = Modifier
) {
    var navigationStack by remember { mutableStateOf<List<SinglePanePage>>(listOf(SinglePanePage.ProjectList)) }

    val currentPage = navigationStack.lastOrNull() ?: SinglePanePage.ProjectList

    LaunchedEffect(currentProjectId, currentVolumeId, currentChapterId) {
        if (currentChapterId != null && currentVolumeId != null) {
            if (currentPage != SinglePanePage.Editor) {
                navigationStack = navigationStack + SinglePanePage.Editor
            }
        } else if (currentProjectId != null) {
            if (currentPage == SinglePanePage.ProjectList) {
                navigationStack = listOf(SinglePanePage.ProjectList, SinglePanePage.ChapterTree)
            }
        }
    }

    fun navigateBack(): Boolean {
        if (navigationStack.size > 1) {
            navigationStack = navigationStack.dropLast(1)
            val targetPage = navigationStack.last()
            when (targetPage) {
                SinglePanePage.ProjectList -> {
                    appState.clearChapterSelection()
                    appState.currentProjectId = null
                    appState.currentProjectTitle = ""
                }
                SinglePanePage.ChapterTree -> {
                    appState.clearChapterSelection()
                }
                SinglePanePage.Editor -> {}
            }
            return true
        }
        return false
    }

    AnimatedContent(
        targetState = currentPage,
        transitionSpec = {
            if (targetState.ordinal > initialState.ordinal) {
                slideInHorizontally { it } togetherWith slideOutHorizontally { -it }
            } else {
                slideInHorizontally { -it } togetherWith slideOutHorizontally { it }
            }
        },
        modifier = modifier.fillMaxSize(),
        label = "workspace_single_pane"
    ) { page ->
        when (page) {
            SinglePanePage.ProjectList -> {
                ProjectListScreen(
                    appState = appState,
                    onSelectProject = { projectId, projectTitle ->
                        appState.selectProject(projectId, projectTitle)
                        navigationStack = listOf(SinglePanePage.ProjectList, SinglePanePage.ChapterTree)
                    },
                    modifier = Modifier.fillMaxSize()
                )
            }
            SinglePanePage.ChapterTree -> {
                VolumeChapterTree(
                    projectId = currentProjectId,
                    workspaceRepository = workspaceRepository,
                    onSelectChapter = { volumeId, chapterId, chapterTitle ->
                        appState.selectChapter(volumeId, chapterId, chapterTitle)
                        navigationStack = listOf(SinglePanePage.ProjectList, SinglePanePage.ChapterTree, SinglePanePage.Editor)
                    },
                    onBackToProjects = {
                        navigationStack = listOf(SinglePanePage.ProjectList)
                        appState.clearChapterSelection()
                        appState.currentProjectId = null
                        appState.currentProjectTitle = ""
                    },
                    modifier = Modifier.fillMaxSize()
                )
            }
            SinglePanePage.Editor -> {
                if (currentVolumeId != null && currentChapterId != null) {
                    SujianEditorHost(
                        projectId = currentProjectId,
                        volumeId = currentVolumeId,
                        chapterId = currentChapterId,
                        chapterTitle = currentChapterTitle,
                        modifier = Modifier.fillMaxSize()
                    )
                } else {
                    navigateBack()
                    Box(modifier = Modifier.fillMaxSize())
                }
            }
        }
    }
}

private enum class SinglePanePage {
    ProjectList, ChapterTree, Editor
}
