package com.xiwei.sujian.core.interop.project

import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class ProjectUseCase(
    private val repository: ProjectRepository,
    private val recentEditsRepository: RecentEditsRepository,
) {
    suspend fun getProjects(): List<Project> =
        withContext(Dispatchers.IO) {
            repository.getProjects()
        }

    suspend fun getRecentEdits(limit: Int): List<RecentEdit> =
        withContext(Dispatchers.IO) {
            recentEditsRepository.getRecentEdits().take(limit)
        }

    suspend fun createProject(title: String) =
        withContext(Dispatchers.IO) {
            repository.createProject(title)
        }

    suspend fun renameProject(
        projectId: String,
        newTitle: String,
    ) = withContext(Dispatchers.IO) {
        repository.renameProject(projectId, newTitle)
    }

    suspend fun deleteProject(projectId: String) =
        withContext(Dispatchers.IO) {
            repository.deleteProject(projectId)
        }

    suspend fun reorderProjects(orderedProjectIds: List<String>) =
        withContext(Dispatchers.IO) {
            repository.reorderProjects(orderedProjectIds)
        }

    suspend fun getProjectTitle(projectId: String): String =
        withContext(Dispatchers.IO) {
            try {
                repository.getProjects().find { it.id == projectId }?.title ?: ""
            } catch (_: Exception) {
                ""
            }
        }

    suspend fun getChapterTitle(chapterId: String): String =
        withContext(Dispatchers.IO) {
            try {
                val projects = repository.getProjects()
                for (project in projects) {
                    val volumes = repository.getVolumes(project.id)
                    for (volume in volumes) {
                        val chapters = repository.getChapters(project.id, volume.id)
                        val found = chapters.find { it.id == chapterId }
                        if (found != null) return@withContext found.title
                    }
                }
                ""
            } catch (_: Exception) {
                ""
            }
        }
}
