package com.xiwei.sujian.ui.compose.workbench.panel

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.ui.compose.workspace.VolumeChapterTree

@Composable
fun ChapterNavigatorPanel(
    projectId: String,
    workspaceRepository: WorkspaceRepository,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    onBackToProjects: () -> Unit,
    modifier: Modifier = Modifier,
) {
    VolumeChapterTree(
        projectId = projectId,
        workspaceRepository = workspaceRepository,
        onSelectChapter = onSelectChapter,
        onBackToProjects = onBackToProjects,
        modifier = modifier,
    )
}
