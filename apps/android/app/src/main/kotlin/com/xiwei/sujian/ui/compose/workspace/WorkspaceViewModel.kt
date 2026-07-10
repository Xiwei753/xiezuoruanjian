package com.xiwei.sujian.ui.compose.workspace

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.model.ChapterMeta
import com.xiwei.sujian.model.ProjectStats
import com.xiwei.sujian.model.Volume
import kotlinx.coroutines.Dispatchers
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
    val isLoading: Boolean = false
)

class WorkspaceViewModel(application: Application) : AndroidViewModel(application) {

    private val _uiState = MutableStateFlow(VolumeChapterUiState())
    val uiState: StateFlow<VolumeChapterUiState> = _uiState.asStateFlow()

    private var currentProjectId: String? = null
    private var workspaceRepository: WorkspaceRepository? = null

    fun initialize(projectId: String, workspaceRepo: WorkspaceRepository) {
        if (currentProjectId == projectId) return
        currentProjectId = projectId
        workspaceRepository = workspaceRepo
        loadVolumes()
        loadProjectStats(projectId)
    }

    fun loadVolumes() {
        val pid = currentProjectId ?: return
        val repo = workspaceRepository ?: return
        _uiState.value = _uiState.value.copy(isLoading = true)
        viewModelScope.launch {
            val volumes = withContext(Dispatchers.IO) {
                try {
                    repo.getVolumes(pid)
                } catch (_: Exception) {
                    emptyList()
                }
            }
            val uiModels = volumes.map { vol ->
                val chapters = withContext(Dispatchers.IO) {
                    try {
                        repo.getChapters(pid, vol.id)
                    } catch (_: Exception) {
                        emptyList()
                    }
                }
                VolumeUiModel(
                    id = vol.id,
                    title = vol.title,
                    chapters = chapters.map { ch ->
                        ChapterUiModel(
                            id = ch.id,
                            title = ch.title,
                            wordCount = ch.wordCount
                        )
                    },
                    isExpanded = _uiState.value.expandedVolumeIds.contains(vol.id)
                )
            }
            _uiState.value = _uiState.value.copy(volumes = uiModels, isLoading = false)
        }
    }

    fun toggleVolumeExpand(volumeId: String) {
        val current = _uiState.value.expandedVolumeIds
        val newExpanded = if (current.contains(volumeId)) {
            current - volumeId
        } else {
            current + volumeId
        }
        _uiState.value = _uiState.value.copy(
            expandedVolumeIds = newExpanded,
            volumes = _uiState.value.volumes.map { v ->
                v.copy(isExpanded = newExpanded.contains(v.id))
            }
        )
    }

    fun selectChapter(chapterId: String) {
        _uiState.value = _uiState.value.copy(selectedChapterId = chapterId)
    }

    fun createVolume(title: String) {
        val pid = currentProjectId ?: return
        val repo = workspaceRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { repo.createVolume(pid, title) } catch (_: Exception) { }
            }
            loadVolumes()
        }
    }

    fun createChapter(volumeId: String, title: String) {
        val pid = currentProjectId ?: return
        val repo = workspaceRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { repo.createChapter(pid, volumeId, title) } catch (_: Exception) { }
            }
            loadVolumes()
        }
    }

    fun renameVolume(volumeId: String, newTitle: String) {
        val pid = currentProjectId ?: return
        val repo = workspaceRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { repo.renameVolume(pid, volumeId, newTitle) } catch (_: Exception) { }
            }
            loadVolumes()
        }
    }

    fun deleteVolume(volumeId: String) {
        val pid = currentProjectId ?: return
        val repo = workspaceRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { repo.deleteVolume(pid, volumeId) } catch (_: Exception) { }
            }
            loadVolumes()
        }
    }

    fun renameChapter(volumeId: String, chapterId: String, newTitle: String) {
        val pid = currentProjectId ?: return
        val repo = workspaceRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { repo.renameChapter(pid, volumeId, chapterId, newTitle) } catch (_: Exception) { }
            }
            loadVolumes()
        }
    }

    fun deleteChapter(volumeId: String, chapterId: String) {
        val pid = currentProjectId ?: return
        val repo = workspaceRepository ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { repo.deleteChapter(pid, volumeId, chapterId) } catch (_: Exception) { }
            }
            loadVolumes()
        }
    }

    private fun loadProjectStats(projectId: String) {
        val repo = workspaceRepository ?: return
        viewModelScope.launch {
            val stats = withContext(Dispatchers.IO) {
                try {
                    repo.getProjectStats(projectId)
                } catch (_: Exception) {
                    null
                }
            }
            stats?.let {
                _uiState.value = _uiState.value.copy(
                    projectStats = ProjectStatsUiModel(
                        totalWordCount = it.totalWordCount,
                        volumeCount = it.volumeCount,
                        chapterCount = it.chapterCount
                    )
                )
            }
        }
    }
}
