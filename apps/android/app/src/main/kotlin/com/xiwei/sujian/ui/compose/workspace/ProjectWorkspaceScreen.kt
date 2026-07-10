package com.xiwei.sujian.ui.compose.workspace

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ListDetailPaneScaffold
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.ui.compose.adaptive.rememberAdaptiveWindowState
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
import com.xiwei.sujian.ui.compose.SujianAppState

@Composable
fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val windowState = rememberAdaptiveWindowState()
    val navigator = rememberListDetailPaneScaffoldNavigator<Any>()
    val workspaceRepository = remember { WorkspaceRepository(context) }

    val currentProjectId = appState.currentProjectId
    val currentVolumeId = appState.currentVolumeId
    val currentChapterId = appState.currentChapterId
    val currentChapterTitle = appState.currentChapterTitle

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

    ListDetailPaneScaffold(
        directive = windowState.scaffoldDirective,
        value = navigator.scaffoldValue,
        listPane = {
            VolumeChapterTree(
                projectId = currentProjectId,
                workspaceRepository = workspaceRepository,
                onSelectChapter = { volumeId, chapterId, chapterTitle ->
                    appState.selectChapter(volumeId, chapterId, chapterTitle)
                },
                modifier = Modifier
            )
        },
        detailPane = {
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
                    Text(
                        "选择章节开始写作",
                        modifier = Modifier.padding(androidx.compose.ui.unit.dp(16.dp))
                    )
                }
            }
        },
        modifier = modifier
    )
}
