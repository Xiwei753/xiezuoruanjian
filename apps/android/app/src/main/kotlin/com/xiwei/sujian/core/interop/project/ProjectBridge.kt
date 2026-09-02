package com.xiwei.sujian.core.interop.project
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.ProjectStats
import com.xiwei.sujian.feature.project.data.model.ProjectSummary
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.Volume

/**
 * 项目/卷 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责项目、卷的 CRUD 与排序操作。
 */
class ProjectBridge internal constructor(private val holder: WriterAppServiceHolder) {
    fun listProjects(): BridgeResult<List<Project>> =
        holder.wrapResult {
            holder.service.listProjects().map { it.toModel() }
        }

    /**
     * #625 第二段：批量作品摘要 — 一次 FFI 调用返回所有项目的
     * id/title/createdAt/updatedAt/totalWordCount/volumeCount/chapterCount。
     *
     * 用于作品卡片字数显示，避免端侧逐卡跨 FFI 调用 [getProjectStats]。
     */
    fun listProjectSummaries(): BridgeResult<List<ProjectSummary>> =
        holder.wrapResult {
            holder.service.listProjectSummaries().map { it.toModel() }
        }

    fun createProject(title: String): BridgeResult<Project> =
        holder.wrapResult {
            holder.service.createProject(title).toModel()
        }

    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> =
        holder.wrapResult {
            holder.service.getProjectStats(projectId).toModel()
        }

    /**
     * #644 评论 5467821839 第7节：一次返回作品的全部卷 + 章节 + 统计快照。
     *
     * Android `ProjectViewModel` 不再逐卷调 `listChapters`，
     * 而是一次拿到完整快照，减少 FFI 调用次数和中间状态不一致窗口。
     */
    fun getProjectWorkspaceSnapshot(projectId: String): BridgeResult<ProjectWorkspaceSnapshot> =
        holder.wrapResult {
            holder.service.getProjectWorkspaceSnapshot(projectId).toModel()
        }

    fun renameProject(
        projectId: String,
        newTitle: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.renameProject(projectId, newTitle)
        }

    fun deleteProject(projectId: String): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteProject(projectId)
        }

    fun reorderProjects(orderedIds: List<String>): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.reorderProjects(orderedIds)
        }

    fun listVolumes(projectId: String): BridgeResult<List<Volume>> =
        holder.wrapResult {
            holder.service.listVolumes(projectId).map { it.toModel() }
        }

    fun createVolume(
        projectId: String,
        title: String,
    ): BridgeResult<Volume> =
        holder.wrapResult {
            holder.service.createVolume(projectId, title).toModel()
        }

    fun renameVolume(
        projectId: String,
        volumeId: String,
        newTitle: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.renameVolume(projectId, volumeId, newTitle)
        }

    fun deleteVolume(
        projectId: String,
        volumeId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteVolume(projectId, volumeId)
        }

    fun reorderVolumes(
        projectId: String,
        orderedIds: List<String>,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.reorderVolumes(projectId, orderedIds)
        }
}
