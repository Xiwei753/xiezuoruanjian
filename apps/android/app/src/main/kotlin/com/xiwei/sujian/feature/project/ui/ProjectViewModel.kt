package com.xiwei.sujian.feature.project.ui

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.feature.project.data.ProjectRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class VolumeChapterUiState(
    val volumes: List<VolumeUiModel> = emptyList(),
    val expandedVolumeIds: Set<String> = emptySet(),
    val selectedChapterId: String? = null,
    val projectStats: ProjectStatsUiModel? = null,
    val isLoading: Boolean = false,
)

/** #617 评论七：一次 refresh 读取的完整快照 — 卷列表与统计原子提交，避免分两次写回。 */
private data class ProjectSnapshot(
    val volumes: List<VolumeUiModel>,
    val projectStats: ProjectStatsUiModel?,
)

// #617 评论一：ProjectViewModel 不依赖 Application — Navigation 3 的 NavEntry 级
// ViewModelStoreOwner 的 CreationExtras 只保证提供 SavedStateHandle（由
// SaveableStateHolderNavEntryDecorator + ViewModelStoreNavEntryDecorator 提供），
// 不保证 APPLICATION_KEY；此前继承 AndroidViewModel 会在进入章节树时崩溃。
class ProjectViewModel(
    private val savedStateHandle: SavedStateHandle,
) : ViewModel() {
    private var currentProjectId: String? = savedStateHandle["currentProjectId"]

    private val _uiState =
        MutableStateFlow(
            VolumeChapterUiState(
                // #617 评论七：展开状态按 projectId 分 key 保存，进程重建后也只恢复本作品的展开。
                expandedVolumeIds =
                    savedStateHandle[expandedVolumeKey(currentProjectId ?: "")] ?: emptySet(),
            ),
        )
    val uiState: StateFlow<VolumeChapterUiState> = _uiState.asStateFlow()

    private var projectRepository: ProjectRepository? = null
    private var isInitialized: Boolean = false

    // #617 评论七：作品读取收成一条可取消的刷新链 — 作品切换/变更刷新都会取消在途
    // refresh job 并递增纪元；迟到的旧纪元/旧作品结果在写回前被丢弃，防止慢设备上
    // 旧作品的卷/统计覆盖新作品的章节树。纪元在作品切换时递增，同作品变更刷新
    // 靠 refreshJob.cancel() + 写回前校验（generation/pid）双保险。
    private var projectGeneration = 0L
    private var refreshJob: Job? = null

    /** 当前 _uiState 数据所属的作品；用于区分“作品切换”与“同作品变更刷新”。 */
    private var loadedProjectId: String? = null

    private fun expandedVolumeKey(projectId: String) = "expandedVolumeIds:$projectId"

    fun initialize(
        projectId: String,
        projectRepo: ProjectRepository,
    ) {
        val projectChanged = currentProjectId != projectId
        val repoChanged = projectRepository !== projectRepo
        val needsReload =
            !isInitialized || projectChanged || repoChanged ||
                _uiState.value.volumes.isEmpty()

        currentProjectId = projectId
        savedStateHandle["currentProjectId"] = projectId
        projectRepository = projectRepo

        if (needsReload) {
            isInitialized = true
            projectGeneration++
            refreshProject()
        }
    }

    /** 取消在途刷新并重新读取当前作品的卷/章节/统计 — 数据刷新的唯一入口。 */
    private fun refreshProject() {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        val generation = projectGeneration

        refreshJob?.cancel()
        refreshJob =
            viewModelScope.launch {
                if (loadedProjectId != pid) {
                    enterProject(pid)
                } else {
                    _uiState.value = _uiState.value.copy(isLoading = true)
                }

                val snapshot = loadProjectSnapshot(repo, pid)

                if (generation != projectGeneration || pid != currentProjectId) return@launch
                loadedProjectId = pid
                // 写回时从“当前” expandedVolumeIds 派生 isExpanded：加载在途期间用户
                // 切换展开时，旧快照里物化的展开标志会让刚点的展开被视觉回滚。
                val expanded = _uiState.value.expandedVolumeIds
                _uiState.value =
                    _uiState.value.copy(
                        volumes =
                            snapshot.volumes.map { v ->
                                v.copy(isExpanded = expanded.contains(v.id))
                            },
                        projectStats = snapshot.projectStats,
                        isLoading = false,
                    )
            }
    }

    /** 作品切换：立即清掉只属于旧作品的 UI 状态（卷/选中章节/统计），展开按 projectId 恢复。 */
    private fun enterProject(pid: String) {
        loadedProjectId = pid
        _uiState.value =
            VolumeChapterUiState(
                expandedVolumeIds = savedStateHandle[expandedVolumeKey(pid)] ?: emptySet(),
                isLoading = true,
            )
    }

    /** 在 IO 线程读取当前作品快照（卷 + 章节 + 统计），失败时降级为空快照。
     *  注意：不在这里物化 isExpanded — 展开标志由写回方从当前 expandedVolumeIds 派生，
     *  避免加载在途期间的展开切换被旧快照覆盖。 */
    private suspend fun loadProjectSnapshot(
        repo: ProjectRepository,
        projectId: String,
    ): ProjectSnapshot =
        withContext(Dispatchers.IO) {
            val volumes =
                try {
                    repo.getVolumes(projectId).map { vol ->
                        VolumeUiModel(
                            id = vol.id,
                            title = vol.title,
                            chapters = loadChapters(repo, projectId, vol.id),
                            isExpanded = false,
                        )
                    }
                } catch (_: Exception) {
                    emptyList()
                }
            ProjectSnapshot(volumes, loadStats(repo, projectId))
        }

    private fun loadChapters(
        repo: ProjectRepository,
        projectId: String,
        volumeId: String,
    ): List<ChapterUiModel> =
        try {
            repo.getChapters(projectId, volumeId).map { ch ->
                ChapterUiModel(
                    id = ch.id,
                    title = ch.title,
                    wordCount = ch.wordCount,
                )
            }
        } catch (_: Exception) {
            emptyList()
        }

    private fun loadStats(
        repo: ProjectRepository,
        projectId: String,
    ): ProjectStatsUiModel? =
        try {
            repo.getProjectStats(projectId)?.let {
                ProjectStatsUiModel(
                    totalWordCount = it.totalWordCount,
                    volumeCount = it.volumeCount,
                    chapterCount = it.chapterCount,
                )
            }
        } catch (_: Exception) {
            null
        }

    fun toggleVolumeExpand(volumeId: String) {
        val pid = currentProjectId ?: return
        val current = _uiState.value.expandedVolumeIds
        val newExpanded =
            if (current.contains(volumeId)) {
                current - volumeId
            } else {
                current + volumeId
            }
        // #617 评论七：展开状态按 projectId 分 key 保存，不跨作品混用。
        savedStateHandle[expandedVolumeKey(pid)] = newExpanded
        _uiState.value =
            _uiState.value.copy(
                expandedVolumeIds = newExpanded,
                volumes =
                    _uiState.value.volumes.map { v ->
                        v.copy(isExpanded = newExpanded.contains(v.id))
                    },
            )
    }

    fun selectChapter(chapterId: String) {
        _uiState.value = _uiState.value.copy(selectedChapterId = chapterId)
    }

    fun createVolume(title: String) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.createVolume(pid, title)
                } catch (_: Exception) {
                }
            }
            // 变更成功（或失败）后统一走刷新链；期间若已切换作品，不刷新别人。
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun createChapter(
        volumeId: String,
        title: String,
    ) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.createChapter(pid, volumeId, title)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) {
                val expanded = _uiState.value.expandedVolumeIds + volumeId
                savedStateHandle[expandedVolumeKey(pid)] = expanded
                _uiState.value = _uiState.value.copy(expandedVolumeIds = expanded)
                refreshProject()
            }
        }
    }

    fun renameVolume(
        volumeId: String,
        newTitle: String,
    ) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.renameVolume(pid, volumeId, newTitle)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun deleteVolume(volumeId: String) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.deleteVolume(pid, volumeId)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun renameChapter(
        volumeId: String,
        chapterId: String,
        newTitle: String,
    ) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.renameChapter(pid, volumeId, chapterId, newTitle)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun deleteChapter(
        volumeId: String,
        chapterId: String,
    ) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.deleteChapter(pid, volumeId, chapterId)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun moveVolumeUp(volumeId: String) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        val currentVolumes = _uiState.value.volumes
        val currentIndex = currentVolumes.indexOfFirst { it.id == volumeId }
        if (currentIndex <= 0) return
        val reordered = currentVolumes.toMutableList()
        reordered.add(currentIndex - 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.reorderVolumes(pid, orderedIds)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun moveVolumeDown(volumeId: String) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        val currentVolumes = _uiState.value.volumes
        val currentIndex = currentVolumes.indexOfFirst { it.id == volumeId }
        if (currentIndex < 0 || currentIndex >= currentVolumes.size - 1) return
        val reordered = currentVolumes.toMutableList()
        reordered.add(currentIndex + 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.reorderVolumes(pid, orderedIds)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun moveChapterUp(
        volumeId: String,
        chapterId: String,
    ) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        val volume = _uiState.value.volumes.find { it.id == volumeId } ?: return
        val currentIndex = volume.chapters.indexOfFirst { it.id == chapterId }
        if (currentIndex <= 0) return
        val reordered = volume.chapters.toMutableList()
        reordered.add(currentIndex - 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.reorderChapters(pid, volumeId, orderedIds)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }

    fun moveChapterDown(
        volumeId: String,
        chapterId: String,
    ) {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        val volume = _uiState.value.volumes.find { it.id == volumeId } ?: return
        val currentIndex = volume.chapters.indexOfFirst { it.id == chapterId }
        if (currentIndex < 0 || currentIndex >= volume.chapters.size - 1) return
        val reordered = volume.chapters.toMutableList()
        reordered.add(currentIndex + 1, reordered.removeAt(currentIndex))
        val orderedIds = reordered.map { it.id }
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repo.reorderChapters(pid, volumeId, orderedIds)
                } catch (_: Exception) {
                }
            }
            if (currentProjectId == pid) refreshProject()
        }
    }
}
