package com.xiwei.sujian.ui.compose.workspace

data class WorkspaceUiState(
    val projectId: String? = null,
    val projectTitle: String = "",
    val volumes: List<VolumeUiModel> = emptyList(),
    val selectedVolumeId: String? = null,
    val selectedChapterId: String? = null,
    val expandedVolumeIds: Set<String> = emptySet(),
    val projectStats: ProjectStatsUiModel? = null,
    val paneDestination: WorkspacePaneDestination = WorkspacePaneDestination.List,
    val isLoading: Boolean = false,
    val errorMessageKey: String? = null
)

data class VolumeUiModel(
    val id: String,
    val title: String,
    val chapters: List<ChapterUiModel>,
    val isExpanded: Boolean = false
)

data class ChapterUiModel(
    val id: String,
    val title: String,
    val wordCount: Int = 0,
    val isSelected: Boolean = false
)

data class ProjectStatsUiModel(
    val totalWordCount: Int = 0,
    val volumeCount: Int = 0,
    val chapterCount: Int = 0
)

enum class WorkspacePaneDestination {
    List,
    Detail
}
