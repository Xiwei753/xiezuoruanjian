package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.*

class WorkspaceRepository(private val context: Context) {
    private val workspaceBridge = BridgeProvider.getWorkspaceBridge(context)
    private val writingBridge = BridgeProvider.getWritingBridge(context)
    private val statsBridge = BridgeProvider.getStatsBridge(context)

    init {
        workspaceBridge.createWorkspaceIfNeeded()
    }

    fun getProjects(): List<Project> {
        return when (val result = workspaceBridge.getProjects()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("获取作品列表失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getRecentEdits(): List<RecentEdit> {
        return when (val result = workspaceBridge.getRecentEdits()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> emptyList() // fail silently for recent edits
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String) {
        workspaceBridge.recordRecentEdit(projectId, volumeId, chapterId)
    }

    fun getChapterContentWithMeta(projectId: String, volumeId: String, chapterId: String): Pair<String, ChapterMeta> {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> Pair(result.data.content, result.data.meta)
            is BridgeResult.Error -> throw RepositoryException("获取章节内容失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): Boolean {
        return when (val result = writingBridge.updateChapterNote(projectId, volumeId, chapterId, note)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("更新章节备注失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getVolumes(projectId: String): List<Volume> {
        return when (val result = workspaceBridge.getVolumes(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("获取卷列表失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        return when (val result = workspaceBridge.getChapters(projectId, volumeId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("获取章节列表失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> result.data.content
            is BridgeResult.Error -> throw RepositoryException("获取章节内容失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<ChapterSaveReceipt> {
        return writingBridge.saveChapterContent(projectId, volumeId, chapterId, content)
    }

    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterSaveReceipt> {
        return writingBridge.clearChapterContent(projectId, volumeId, chapterId)
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
        return writingBridge.recordWritingEvent(
            deviceId, projectId, volumeId, chapterId,
            source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId
        )
    }

    fun flushWritingStats() {
        statsBridge.flushWritingStats()
    }

    fun getProjectStats(projectId: String): ProjectStats {
        return when (val result = statsBridge.getProjectStats(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("获取作品统计失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createProject(title: String): Project {
        return when (val result = workspaceBridge.createProject(title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("创建作品失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createVolume(projectId: String, title: String): Volume {
        return when (val result = workspaceBridge.createVolume(projectId, title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("创建卷失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        return when (val result = workspaceBridge.createChapter(projectId, volumeId, title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException("创建章节失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameProject(projectId: String, newTitle: String) {
        when (val result = workspaceBridge.renameProject(projectId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("重命名作品失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteProject(projectId: String) {
        when (val result = workspaceBridge.deleteProject(projectId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("删除作品失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderProjects(orderedProjectIds: List<String>) {
        when (val result = workspaceBridge.reorderProjects(orderedProjectIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("重排作品失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameVolume(projectId: String, volumeId: String, newTitle: String) {
        when (val result = workspaceBridge.renameVolume(projectId, volumeId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("重命名分卷失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteVolume(projectId: String, volumeId: String) {
        when (val result = workspaceBridge.deleteVolume(projectId, volumeId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("删除分卷失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderVolumes(projectId: String, orderedVolumeIds: List<String>) {
        when (val result = workspaceBridge.reorderVolumes(projectId, orderedVolumeIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("重排分卷失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String) {
        when (val result = workspaceBridge.renameChapter(projectId, volumeId, chapterId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("重命名章节失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteChapter(projectId: String, volumeId: String, chapterId: String) {
        when (val result = workspaceBridge.deleteChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("删除章节失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderChapters(projectId: String, volumeId: String, orderedChapterIds: List<String>) {
        when (val result = workspaceBridge.reorderChapters(projectId, volumeId, orderedChapterIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException("重排章节失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getWorkspaceDir(): String = com.xiwei.writerapp.data.WorkspaceManager.getWorkspaceDir(context).absolutePath

    fun calculateWordCount(text: String): Int {
        return writingBridge.calculateWordCount(text)
    }

    fun processWritingEvent(
        deviceId: String,
        platform: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        oldText: String,
        newText: String,
        sessionId: String
    ): Boolean {
        return writingBridge.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, sessionId)
    }

}
