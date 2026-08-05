package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.RecentEdit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

sealed interface SessionRestoreState {
    data object Loading : SessionRestoreState
    data class Ready(
        val projectId: String?,
        val volumeId: String?,
        val chapterId: String?,
    ) : SessionRestoreState
}

class WorkspaceSessionViewModel(
    private val savedStateHandle: SavedStateHandle,
) : ViewModel() {

    var restoreState by mutableStateOf<SessionRestoreState>(SessionRestoreState.Loading)
        private set

    var projects by mutableStateOf<List<Project>>(emptyList())
        private set

    var recentEdits by mutableStateOf<List<RecentEdit>>(emptyList())
        private set

    var currentProjectId by mutableStateOf<String?>(savedStateHandle["currentProjectId"])
        private set

    var currentProjectTitle by mutableStateOf(savedStateHandle["currentProjectTitle"] ?: "")
        private set

    var currentVolumeId by mutableStateOf<String?>(savedStateHandle["currentVolumeId"])
        private set

    var currentChapterId by mutableStateOf<String?>(savedStateHandle["currentChapterId"])
        private set

    var currentChapterTitle by mutableStateOf(savedStateHandle["currentChapterTitle"] ?: "")
        private set

    private var workspaceUseCase: WorkspaceUseCase? = null

    fun initialize(
        workspaceRepo: WorkspaceRepository,
        workspaceUC: WorkspaceUseCase,
    ) {
        workspaceUseCase = workspaceUC
        refreshProjects()
        refreshRecentEdits()
        restoreState = SessionRestoreState.Ready(
            projectId = currentProjectId,
            volumeId = currentVolumeId,
            chapterId = currentChapterId,
        )
    }

    fun selectProject(projectId: String, projectTitle: String) {
        currentProjectId = projectId
        currentProjectTitle = projectTitle
        savedStateHandle["currentProjectId"] = projectId
        savedStateHandle["currentProjectTitle"] = projectTitle
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("project", projectId)
    }

    fun selectProject(projectId: String) {
        currentProjectId = projectId
        savedStateHandle["currentProjectId"] = projectId
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("project", projectId)
        val cachedProject = projects.find { it.id == projectId }
        if (cachedProject != null) {
            currentProjectTitle = cachedProject.title
            savedStateHandle["currentProjectTitle"] = cachedProject.title
        } else {
            viewModelScope.launch {
                val title = withContext(Dispatchers.IO) {
                    try { workspaceUseCase?.getProjectTitle(projectId) ?: "" }
                    catch (_: Exception) { "" }
                }
                currentProjectTitle = title
                savedStateHandle["currentProjectTitle"] = title
            }
        }
    }

    fun selectChapter(volumeId: String, chapterId: String, chapterTitle: String) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        currentChapterTitle = chapterTitle
        savedStateHandle["currentVolumeId"] = volumeId
        savedStateHandle["currentChapterId"] = chapterId
        savedStateHandle["currentChapterTitle"] = chapterTitle
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("chapter", chapterId)
    }

    fun selectChapter(volumeId: String, chapterId: String) {
        currentVolumeId = volumeId
        currentChapterId = chapterId
        savedStateHandle["currentVolumeId"] = volumeId
        savedStateHandle["currentChapterId"] = chapterId
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceSelection("chapter", chapterId)
        viewModelScope.launch {
            val title = withContext(Dispatchers.IO) {
                try { workspaceUseCase?.getChapterTitle(chapterId) ?: "" }
                catch (_: Exception) { "" }
            }
            currentChapterTitle = title
            savedStateHandle["currentChapterTitle"] = title
        }
    }

    fun clearChapterSelection() {
        currentVolumeId = null
        currentChapterId = null
        currentChapterTitle = ""
        savedStateHandle.remove<String>("currentVolumeId")
        savedStateHandle.remove<String>("currentChapterId")
        savedStateHandle["currentChapterTitle"] = ""
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceClear("chapter")
    }

    fun clearProjectSelection() {
        currentProjectId = null
        currentProjectTitle = ""
        currentVolumeId = null
        currentChapterId = null
        currentChapterTitle = ""
        savedStateHandle.remove<String>("currentProjectId")
        savedStateHandle["currentProjectTitle"] = ""
        savedStateHandle.remove<String>("currentVolumeId")
        savedStateHandle.remove<String>("currentChapterId")
        savedStateHandle["currentChapterTitle"] = ""
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceClear("project")
    }

    fun refreshProjects() {
        viewModelScope.launch {
            projects = withContext(Dispatchers.IO) {
                try { workspaceUseCase?.getProjects() ?: emptyList() }
                catch (_: Exception) { emptyList() }
            }
        }
    }

    fun refreshRecentEdits() {
        viewModelScope.launch {
            recentEdits = withContext(Dispatchers.IO) {
                try { workspaceUseCase?.getRecentEdits(5) ?: emptyList() }
                catch (_: Exception) { emptyList() }
            }
        }
    }

    fun createProject(title: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { workspaceUseCase?.createProject(title) } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun deleteProject(projectId: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { workspaceUseCase?.deleteProject(projectId) } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }

    fun renameProject(projectId: String, newTitle: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { workspaceUseCase?.renameProject(projectId, newTitle) } catch (_: Exception) { }
            }
            refreshProjects()
        }
    }
}
