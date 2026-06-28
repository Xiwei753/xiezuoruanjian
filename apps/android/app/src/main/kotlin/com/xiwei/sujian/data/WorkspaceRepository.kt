package com.xiwei.sujian.data

import android.content.Context
import com.xiwei.sujian.R
import com.xiwei.sujian.model.*

class WorkspaceRepository(private val context: Context) {
    private val workspaceBridge = BridgeProvider.getWorkspaceBridge(context)
    private val writingBridge = BridgeProvider.getWritingBridge(context)
    private val statsBridge = BridgeProvider.getStatsBridge(context)

    private fun BridgeResult.Error.localizedMessage(): String {
        return MessageKeyMapper.resolveMessage(context, envelope.messageKey, envelope.messageArgs, envelope.errorCode)
    }

    init {
        when (val result = workspaceBridge.createWorkspaceIfNeeded()) {
            is BridgeResult.Error -> {
                android.util.Log.e("WorkspaceRepository", "工作区初始化失败: ${result.localizedMessage()}")
                throw RepositoryException(context.getString(R.string.repo_workspace_init_failed, result.localizedMessage()))
            }
            BridgeResult.NotLoaded -> {
                android.util.Log.e("WorkspaceRepository", context.getString(R.string.repo_native_not_loaded_init))
            }
            is BridgeResult.Success -> {
                android.util.Log.d("WorkspaceRepository", "工作区初始化成功")
            }
        }
    }

    fun getProjects(): List<Project> {
        return when (val result = workspaceBridge.getProjects()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_get_projects_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun getRecentEdits(): List<RecentEdit> {
        return when (val result = workspaceBridge.getRecentEdits()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                android.util.Log.w("WorkspaceRepository", context.getString(R.string.repo_get_recent_edits_failed, result.localizedMessage()))
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String) {
        workspaceBridge.recordRecentEdit(projectId, volumeId, chapterId)
    }

    fun flushRecentEdits() {
        workspaceBridge.flushRecentEdits()
    }

    fun getChapterContentWithMeta(projectId: String, volumeId: String, chapterId: String): Pair<String, ChapterMeta> {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> Pair(result.data.content, result.data.meta)
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_get_chapter_content_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): Boolean {
        return when (val result = writingBridge.updateChapterNote(projectId, volumeId, chapterId, note)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_update_chapter_note_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun getVolumes(projectId: String): List<Volume> {
        return when (val result = workspaceBridge.getVolumes(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_get_volumes_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        return when (val result = workspaceBridge.getChapters(projectId, volumeId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_get_chapters_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> result.data.content
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_get_chapter_content_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
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
        durationSeconds: Int,
        sessionId: String
    ): BridgeResult<Boolean> {
        return writingBridge.recordWritingEvent(
            deviceId, projectId, volumeId, chapterId,
            source, insertedChars, deletedChars, pastedChars, aiInsertedChars, durationSeconds, sessionId
        )
    }

    fun flushWritingStats() {
        statsBridge.flushWritingStats()
    }

    fun getProjectStats(projectId: String): ProjectStats {
        return when (val result = statsBridge.getProjectStats(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_get_project_stats_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun createProject(title: String): Project {
        return when (val result = workspaceBridge.createProject(title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_create_project_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun createVolume(projectId: String, title: String): Volume {
        return when (val result = workspaceBridge.createVolume(projectId, title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_create_volume_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        return when (val result = workspaceBridge.createChapter(projectId, volumeId, title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_create_chapter_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun renameProject(projectId: String, newTitle: String) {
        when (val result = workspaceBridge.renameProject(projectId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_rename_project_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun deleteProject(projectId: String) {
        when (val result = workspaceBridge.deleteProject(projectId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_delete_project_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun reorderProjects(orderedProjectIds: List<String>) {
        when (val result = workspaceBridge.reorderProjects(orderedProjectIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_reorder_projects_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun renameVolume(projectId: String, volumeId: String, newTitle: String) {
        when (val result = workspaceBridge.renameVolume(projectId, volumeId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_rename_volume_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun deleteVolume(projectId: String, volumeId: String) {
        when (val result = workspaceBridge.deleteVolume(projectId, volumeId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_delete_volume_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun reorderVolumes(projectId: String, orderedVolumeIds: List<String>) {
        when (val result = workspaceBridge.reorderVolumes(projectId, orderedVolumeIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_reorder_volumes_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String) {
        when (val result = workspaceBridge.renameChapter(projectId, volumeId, chapterId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_rename_chapter_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun deleteChapter(projectId: String, volumeId: String, chapterId: String) {
        when (val result = workspaceBridge.deleteChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_delete_chapter_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun reorderChapters(projectId: String, volumeId: String, orderedChapterIds: List<String>) {
        when (val result = workspaceBridge.reorderChapters(projectId, volumeId, orderedChapterIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(context.getString(R.string.repo_reorder_chapters_failed, result.localizedMessage()))
            BridgeResult.NotLoaded -> throw RepositoryException(context.getString(R.string.repo_native_not_loaded))
        }
    }

    fun getWorkspaceDir(): String = com.xiwei.sujian.data.WorkspaceManager.getWorkspaceDir(context).absolutePath

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
        durationSeconds: UInt,
        sessionId: String
    ): BridgeResult<Boolean> {
        return writingBridge.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, durationSeconds, sessionId)
    }

}
