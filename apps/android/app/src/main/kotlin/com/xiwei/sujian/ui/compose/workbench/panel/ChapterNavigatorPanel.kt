package com.xiwei.sujian.ui.compose.workbench.panel

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.ui.compose.workspace.ChapterTreeContent

@Composable
fun ChapterNavigatorPanel(
    projectId: String,
    workspaceRepository: WorkspaceRepository,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    onBackToProjects: () -> Unit,
    modifier: Modifier = Modifier,
) {
    ChapterTreeContent(
        projectId = projectId,
        workspaceRepository = workspaceRepository,
        onSelectChapter = onSelectChapter,
        showHeader = false,
        onBackToProjects = onBackToProjects,
        modifier = modifier,
    )
}
