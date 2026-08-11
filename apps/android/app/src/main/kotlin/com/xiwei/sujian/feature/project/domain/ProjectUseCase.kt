package com.xiwei.sujian.feature.project.domain

import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * #614：ProjectUseCase 对外契约。app 层 [com.xiwei.sujian.app.SujianAppViewModel] 只依赖此端口，
 * 便于在 Robolectric 单元测试里注入 fake（无需构造真实 Repository/Bridge）。
 */
interface ProjectUseCasePort {
    suspend fun getProjects(): List<Project>

    suspend fun getRecentEdits(limit: Int): List<RecentEdit>

    suspend fun createProject(title: String): Project

    suspend fun renameProject(
        projectId: String,
        newTitle: String,
    )

    suspend fun deleteProject(projectId: String)

    suspend fun reorderProjects(orderedProjectIds: List<String>)

    suspend fun getProjectTitle(projectId: String): String

    suspend fun getChapterTitle(chapterId: String): String
}

class ProjectUseCase(
    private val repository: ProjectRepository,
    private val recentEditsRepository: RecentEditsRepository,
) : ProjectUseCasePort {
    override suspend fun getProjects(): List<Project> =
        withContext(Dispatchers.IO) {
            repository.getProjects()
        }

    override suspend fun getRecentEdits(limit: Int): List<RecentEdit> =
        withContext(Dispatchers.IO) {
            recentEditsRepository.getRecentEdits().take(limit)
        }

    override suspend fun createProject(title: String) =
        withContext(Dispatchers.IO) {
            repository.createProject(title)
        }

    override suspend fun renameProject(
        projectId: String,
        newTitle: String,
    ) = withContext(Dispatchers.IO) {
        repository.renameProject(projectId, newTitle)
    }

    override suspend fun deleteProject(projectId: String) =
        withContext(Dispatchers.IO) {
            repository.deleteProject(projectId)
        }

    override suspend fun reorderProjects(orderedProjectIds: List<String>) =
        withContext(Dispatchers.IO) {
            repository.reorderProjects(orderedProjectIds)
        }

    override suspend fun getProjectTitle(projectId: String): String =
        withContext(Dispatchers.IO) {
            try {
                repository.getProjects().find { it.id == projectId }?.title ?: ""
            } catch (_: Exception) {
                ""
            }
        }

    override suspend fun getChapterTitle(chapterId: String): String =
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
