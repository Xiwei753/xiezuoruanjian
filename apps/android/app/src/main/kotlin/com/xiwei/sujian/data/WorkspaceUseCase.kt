package com.xiwei.sujian.data

import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.RecentEdit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class WorkspaceUseCase(private val repository: WorkspaceRepository) {
    suspend fun getProjects(): List<Project> = withContext(Dispatchers.IO) {
        repository.getProjects()
    }

    suspend fun getRecentEdits(limit: Int): List<RecentEdit> = withContext(Dispatchers.IO) {
        repository.getRecentEdits().take(limit)
    }

    suspend fun createProject(title: String) = withContext(Dispatchers.IO) {
        repository.createProject(title)
    }

    suspend fun renameProject(projectId: String, newTitle: String) = withContext(Dispatchers.IO) {
        repository.renameProject(projectId, newTitle)
    }

    suspend fun deleteProject(projectId: String) = withContext(Dispatchers.IO) {
        repository.deleteProject(projectId)
    }

    suspend fun reorderProjects(orderedProjectIds: List<String>) = withContext(Dispatchers.IO) {
        repository.reorderProjects(orderedProjectIds)
    }
}
