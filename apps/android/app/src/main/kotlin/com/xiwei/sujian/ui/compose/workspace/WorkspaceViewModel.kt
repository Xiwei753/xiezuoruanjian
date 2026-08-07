package com.xiwei.sujian.ui.compose.workspace

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.data.ProjectRepository
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
    val isLoading: Boolean = false,
)

class WorkspaceViewModel(
    application: Application,
    private val savedStateHandle: SavedStateHandle,
) : AndroidViewModel(application) {
    private val _uiState =
        MutableStateFlow(
            VolumeChapterUiState(
                expandedVolumeIds = savedStateHandle.get<Set<String>>("expandedVolumeIds") ?: emptySet(),
            ),
        )
    val uiState: StateFlow<VolumeChapterUiState> = _uiState.asStateFlow()

    private var currentProjectId: String? = savedStateHandle["currentProjectId"]
    private var projectRepository: ProjectRepository? = null
    private var isInitialized: Boolean = false

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
            loadVolumes()
            loadProjectStats(projectId)
        }
    }

    fun loadVolumes() {
        val pid = currentProjectId ?: return
        val repo = projectRepository ?: return
        _uiState.value = _uiState.value.copy(isLoading = true)
        viewModelScope.launch {
            val volumes =
                withContext(Dispatchers.IO) {
                    try {
                        repo.getVolumes(pid)
                    } catch (_: Exception) {
                        emptyList()
                    }
                }
            val uiModels =
                volumes.map { vol ->
                    val chapters =
                        withContext(Dispatchers.IO) {
                            try {
                                repo.getChapters(pid, vol.id)
                            } catch (_: Exception) {
                                emptyList()
                            }
                        }
                    VolumeUiModel(
                        id = vol.id,
                        title = vol.title,
                        chapters =
                            chapters.map { ch ->
                                ChapterUiModel(
                                    id = ch.id,
                                    title = ch.title,
                                    wordCount = ch.wordCount,
                                )
                            },
                        isExpanded = _uiState.value.expandedVolumeIds.contains(vol.id),
                    )
                }
            _uiState.value = _uiState.value.copy(volumes = uiModels, isLoading = false)
        }
    }

    fun toggleVolumeExpand(volumeId: String) {
        val current = _uiState.value.expandedVolumeIds
        val newExpanded =
            if (current.contains(volumeId)) {
                current - volumeId
            } else {
                current + volumeId
            }
        savedStateHandle["expandedVolumeIds"] = newExpanded
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
            loadVolumes()
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
            val expanded = _uiState.value.expandedVolumeIds + volumeId
            savedStateHandle["expandedVolumeIds"] = expanded
            _uiState.value = _uiState.value.copy(expandedVolumeIds = expanded)
            loadVolumes()
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
            loadVolumes()
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
            loadVolumes()
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
            loadVolumes()
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
            loadVolumes()
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
            loadVolumes()
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
            loadVolumes()
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
            loadVolumes()
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
            loadVolumes()
        }
    }

    private fun loadProjectStats(projectId: String) {
        val repo = projectRepository ?: return
        viewModelScope.launch {
            val stats =
                withContext(Dispatchers.IO) {
                    try {
                        repo.getProjectStats(projectId)
                    } catch (_: Exception) {
                        null
                    }
                }
            stats?.let {
                _uiState.value =
                    _uiState.value.copy(
                        projectStats =
                            ProjectStatsUiModel(
                                totalWordCount = it.totalWordCount,
                                volumeCount = it.volumeCount,
                                chapterCount = it.chapterCount,
                            ),
                    )
            }
        }
    }
}
