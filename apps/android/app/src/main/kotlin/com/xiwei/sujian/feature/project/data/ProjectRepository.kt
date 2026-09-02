package com.xiwei.sujian.feature.project.data
import android.content.Context
import com.xiwei.sujian.R
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.MessageKeyMapper
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.ProjectStats
import com.xiwei.sujian.feature.project.data.model.ProjectSummary
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.Volume
import com.xiwei.sujian.feature.sync.data.SyncFailureKind

/**
 * ProjectRepository — 项目树 CRUD 仓库层。
 *
 * #602 Phase 5：章节内容函数移到 ChapterRepository，最近编辑函数移到 RecentEditsRepository，
 * 统计函数移到 WritingStatsRepository。本类只保留项目树 CRUD 与 getProjectStats。
 *
 * 数据读取方法声明为 open：允许测试子类注入可控数据源，验证 ProjectViewModel
 * 加载纪元的陈旧结果丢弃语义；生产路径仍是单例真实实现。
 */
open class ProjectRepository(private val context: Context, private val appBridge: AppServiceBridge) {
    private val projectBridge = appBridge.projectBridge
    private val chapterBridge = appBridge.chapterBridge
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

    /**
     * #625 第二段：批量作品摘要 — 一次 FFI 调用返回所有项目摘要（含字数）。
     *
     * 用于作品卡片字数显示，避免端侧逐卡跨 FFI 调用 [getProjectStats]。
     * 声明为 open 以便测试子类注入可控数据源。
     */
    open fun getProjectSummaries(): List<ProjectSummary> {
        return when (val result = projectBridge.listProjectSummaries()) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_project_summaries_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    open fun getVolumes(projectId: String): List<Volume> {
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

    open fun getChapters(
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

    open fun getProjectStats(projectId: String): ProjectStats {
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

    /**
     * #644 评论 5467821839 第7节：一次返回作品的全部卷 + 章节 + 统计快照。
     *
     * 不再逐卷调 [getChapters]，减少 FFI 调用次数和中间状态不一致窗口。
     */
    open fun getProjectWorkspaceSnapshot(projectId: String): ProjectWorkspaceSnapshot {
        return when (val result = projectBridge.getProjectWorkspaceSnapshot(projectId)) {
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

    open fun createVolume(
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

    open fun createChapter(
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
}
