package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.*

class WorkspaceRepository(context: Context) {
    private val bridge = NativeCoreBridge(context)

    init {
        bridge.createWorkspaceIfNeeded()
    }

    fun getProjects(): List<Project> {
        return when (val result = bridge.getProjects()) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取作品列表失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getRecentEdits(): List<RecentEdit> {
        return when (val result = bridge.getRecentEdits()) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> emptyList() // fail silently for recent edits
            NativeResult.NotLoaded -> emptyList()
        }
    }

    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String) {
        bridge.recordRecentEdit(projectId, volumeId, chapterId)
    }

    fun getChapterContentWithMeta(projectId: String, volumeId: String, chapterId: String): Pair<String, ChapterMeta> {
        return when (val result = bridge.getChapterContentWithMeta(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取章节内容失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): Boolean {
        return when (val result = bridge.updateChapterNote(projectId, volumeId, chapterId, note)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("更新章节备注失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getVolumes(projectId: String): List<Volume> {
        return when (val result = bridge.getVolumes(projectId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取卷列表失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        return when (val result = bridge.getChapters(projectId, volumeId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取章节列表失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        return when (val result = bridge.getChapterContent(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取章节内容失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        return when (val result = bridge.saveChapterContent(projectId, volumeId, chapterId, content)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("保存章节内容失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun recordWritingEvent(
        deviceId: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        source: String,
        insertedChars: Int,
        deletedChars: Int,
        pastedChars: Int,
        aiInsertedChars: Int,
        sessionId: String
    ): Boolean {
        return bridge.recordWritingEvent(
            deviceId, projectId, volumeId, chapterId,
            source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId
        )
    }

    fun flushWritingStats() {
        bridge.flushWritingStats()
    }

    fun getProjectStats(projectId: String): ProjectStats {
        return when (val result = bridge.getProjectStats(projectId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取作品统计失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createProject(title: String): Project {
        return when (val result = bridge.createProject(title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("创建作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createVolume(projectId: String, title: String): Volume {
        return when (val result = bridge.createVolume(projectId, title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("创建卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        return when (val result = bridge.createChapter(projectId, volumeId, title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("创建章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameProject(projectId: String, newTitle: String) {
        when (val result = bridge.renameProject(projectId, newTitle)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重命名作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteProject(projectId: String) {
        when (val result = bridge.deleteProject(projectId)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("删除作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderProjects(orderedProjectIds: List<String>) {
        when (val result = bridge.reorderProjects(orderedProjectIds)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重排作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameVolume(projectId: String, volumeId: String, newTitle: String) {
        when (val result = bridge.renameVolume(projectId, volumeId, newTitle)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重命名分卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteVolume(projectId: String, volumeId: String) {
        when (val result = bridge.deleteVolume(projectId, volumeId)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("删除分卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderVolumes(projectId: String, orderedVolumeIds: List<String>) {
        when (val result = bridge.reorderVolumes(projectId, orderedVolumeIds)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重排分卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String) {
        when (val result = bridge.renameChapter(projectId, volumeId, chapterId, newTitle)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重命名章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteChapter(projectId: String, volumeId: String, chapterId: String) {
        when (val result = bridge.deleteChapter(projectId, volumeId, chapterId)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("删除章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderChapters(projectId: String, volumeId: String, orderedChapterIds: List<String>) {
        when (val result = bridge.reorderChapters(projectId, volumeId, orderedChapterIds)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重排章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

}
