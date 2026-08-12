package com.xiwei.sujian.feature.project.ui

data class ProjectWorkspaceUiState(
    val projectId: String? = null,
    val projectTitle: String = "",
    val volumes: List<VolumeUiModel> = emptyList(),
    val selectedVolumeId: String? = null,
    val selectedChapterId: String? = null,
    val expandedVolumeIds: Set<String> = emptySet(),
    val projectStats: ProjectStatsUiModel? = null,
    val paneDestination: WorkspacePaneDestination = WorkspacePaneDestination.List,
    val isLoading: Boolean = false,
    val errorMessageKey: String? = null,
)

// #617 评论八：UI 模型只装卷/章节数据，不携带任何交互状态 —
// 展开/收起只存在 VolumeChapterUiState.expandedVolumeIds 一份真相，
// 选中章节只存在 selectedChapterId 一份真相；刷新链写回永远不覆盖交互状态。
data class VolumeUiModel(
    val id: String,
    val title: String,
    val chapters: List<ChapterUiModel>,
)

data class ChapterUiModel(
    val id: String,
    val title: String,
    val wordCount: Int = 0,
)

data class ProjectStatsUiModel(
    val totalWordCount: Int = 0,
    val volumeCount: Int = 0,
    val chapterCount: Int = 0,
)

enum class WorkspacePaneDestination {
    List,
    Detail,
}
