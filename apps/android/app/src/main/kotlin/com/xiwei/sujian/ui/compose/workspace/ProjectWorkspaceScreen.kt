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
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.data.WorkspaceRepository
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

    when (paneMode) {
        WorkspacePaneMode.ThreePane -> {
            ThreePaneLayout(
                appState = appState,
                currentProjectId = currentProjectId,
                currentVolumeId = currentVolumeId,
                currentChapterId = currentChapterId,
                currentChapterTitle = currentChapterTitle,
                workspaceRepository = workspaceRepository,
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
    modifier: Modifier = Modifier
) {
    Row(modifier = modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .width(200.dp)
                .fillMaxHeight()
        ) {
            ProjectListScreen(
                appState = appState,
                onSelectProject = { _, _ -> },
                modifier = Modifier.fillMaxSize()
            )
        }
        Box(
            modifier = Modifier
                .width(280.dp)
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
                    modifier = Modifier.fillMaxSize()
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
    modifier: Modifier = Modifier
) {
    Row(modifier = modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .width(280.dp)
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
                    modifier = Modifier.fillMaxSize()
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
    val showEditor = currentVolumeId != null && currentChapterId != null

    AnimatedContent(
        targetState = showEditor,
        transitionSpec = {
            if (targetState) {
                slideInHorizontally { it } togetherWith slideOutHorizontally { -it }
            } else {
                slideInHorizontally { -it } togetherWith slideOutHorizontally { it }
            }
        },
        modifier = modifier.fillMaxSize(),
        label = "workspace_single_pane"
    ) { editing ->
        if (editing && currentVolumeId != null && currentChapterId != null) {
            SujianEditorHost(
                projectId = currentProjectId,
                volumeId = currentVolumeId,
                chapterId = currentChapterId,
                chapterTitle = currentChapterTitle,
                modifier = Modifier.fillMaxSize()
            )
        } else {
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
    }
}
