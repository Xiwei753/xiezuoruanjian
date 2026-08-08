package com.xiwei.sujian.core.interop.project
import android.content.Context
import com.xiwei.sujian.R
import com.xiwei.sujian.app.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.BridgeProvider
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.MessageKeyMapper
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.core.interop.sync.SyncFailureKind
import com.xiwei.sujian.core.model.ChapterMeta
import com.xiwei.sujian.core.model.ChapterSaveReceipt
import com.xiwei.sujian.core.model.Project
import com.xiwei.sujian.core.model.ProjectStats
import com.xiwei.sujian.core.model.RecentEdit
import com.xiwei.sujian.core.model.Volume

class ProjectRepository(private val context: Context, bridge: AppServiceBridge? = null) : ChapterContentSavePort {
    private val appBridge = bridge ?: BridgeProvider.getAppServiceBridge(context)
    private val projectBridge = appBridge.projectBridge
    private val chapterBridge = appBridge.chapterBridge
    private val recentEditsBridge = appBridge.recentEditsBridge
    private val writingBridge = WritingBridge(appBridge)
    private val statsBridge = appBridge.statsBridge

    private fun BridgeResult.Error.localizedMessage(): String {
        return MessageKeyMapper.resolveMessage(context, envelope.messageKey, envelope.messageArgs, envelope.errorCode)
    }

    fun getProjects(): List<Project> {
        return when (val result = projectBridge.listProjects()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_projects_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun getRecentEdits(): List<RecentEdit> {
        return when (val result = recentEditsBridge.getRecentEdits()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                DiagnosticsLogger.w(
                    "ProjectRepository",
                    context.getString(R.string.repo_get_recent_edits_failed, result.localizedMessage()),
                )
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun recordRecentEdit(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        recentEditsBridge.recordRecentEdit(projectId, volumeId, chapterId)
    }

    fun flushRecentEdits() {
        recentEditsBridge.flushRecentEdits()
    }

    fun getChapterContentWithMeta(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): Pair<String, ChapterMeta> {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> Pair(result.data.content, result.data.meta)
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_chapter_content_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun updateChapterNote(
        projectId: String,
        volumeId: String,
        chapterId: String,
        note: String,
    ): Boolean {
        return when (val result = writingBridge.updateChapterNote(projectId, volumeId, chapterId, note)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_update_chapter_note_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun getVolumes(projectId: String): List<Volume> {
        return when (val result = projectBridge.listVolumes(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_volumes_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun getChapters(
        projectId: String,
        volumeId: String,
    ): List<ChapterMeta> {
        return when (val result = chapterBridge.listChapters(projectId, volumeId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_chapters_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun getChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): String {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> result.data.content
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_chapter_content_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    override suspend fun saveChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        content: String,
    ): BridgeResult<ChapterSaveReceipt> {
        return writingBridge.saveChapterContent(projectId, volumeId, chapterId, content)
    }

    fun clearChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterSaveReceipt> {
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
        sessionId: String,
    ): BridgeResult<Boolean> {
        return writingBridge.recordWritingEvent(
            deviceId, projectId, volumeId, chapterId,
            source, insertedChars, deletedChars, pastedChars, aiInsertedChars, durationSeconds, sessionId,
        )
    }

    fun flushWritingStats() {
        statsBridge.flushWritingStats()
    }

    fun getProjectStats(projectId: String): ProjectStats {
        return when (val result = statsBridge.getProjectStats(projectId)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_project_stats_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun createProject(title: String): Project {
        return when (val result = projectBridge.createProject(title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_create_project_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun createVolume(
        projectId: String,
        title: String,
    ): Volume {
        return when (val result = projectBridge.createVolume(projectId, title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_create_volume_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun createChapter(
        projectId: String,
        volumeId: String,
        title: String,
    ): ChapterMeta {
        return when (val result = chapterBridge.createChapter(projectId, volumeId, title)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_create_chapter_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun renameProject(
        projectId: String,
        newTitle: String,
    ) {
        when (val result = projectBridge.renameProject(projectId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_rename_project_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun deleteProject(projectId: String) {
        when (val result = projectBridge.deleteProject(projectId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_delete_project_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun reorderProjects(orderedProjectIds: List<String>) {
        when (val result = projectBridge.reorderProjects(orderedProjectIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_reorder_projects_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun renameVolume(
        projectId: String,
        volumeId: String,
        newTitle: String,
    ) {
        when (val result = projectBridge.renameVolume(projectId, volumeId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_rename_volume_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun deleteVolume(
        projectId: String,
        volumeId: String,
    ) {
        when (val result = projectBridge.deleteVolume(projectId, volumeId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_delete_volume_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun reorderVolumes(
        projectId: String,
        orderedVolumeIds: List<String>,
    ) {
        when (val result = projectBridge.reorderVolumes(projectId, orderedVolumeIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_reorder_volumes_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun renameChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
        newTitle: String,
    ) {
        when (val result = chapterBridge.renameChapter(projectId, volumeId, chapterId, newTitle)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_rename_chapter_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun deleteChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        when (val result = chapterBridge.deleteChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_delete_chapter_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun reorderChapters(
        projectId: String,
        volumeId: String,
        orderedChapterIds: List<String>,
    ) {
        when (val result = chapterBridge.reorderChapters(projectId, volumeId, orderedChapterIds)) {
            is BridgeResult.Success -> {}
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_reorder_chapters_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

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
        sessionId: String,
    ): BridgeResult<Boolean> {
        return writingBridge.processWritingEvent(
            deviceId, platform, projectId, volumeId, chapterId, oldText, newText,
            durationSeconds, sessionId,
        )
    }
}
