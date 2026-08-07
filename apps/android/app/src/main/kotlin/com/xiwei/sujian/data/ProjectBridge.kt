package com.xiwei.sujian.data

import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.ProjectStats
import com.xiwei.sujian.model.RecentEdit
import com.xiwei.sujian.model.Volume

/**
 * 项目/卷 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责项目、卷、最近编辑相关操作。
 */
class ProjectBridge internal constructor(private val holder: WriterAppServiceHolder) {
    fun listProjects(): BridgeResult<List<Project>> =
        holder.wrapResult {
            holder.service.listProjects().map { it.toModel() }
        }

    fun createProject(title: String): BridgeResult<Project> =
        holder.wrapResult {
            holder.service.createProject(title).toModel()
        }

    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> =
        holder.wrapResult {
            holder.service.getProjectStats(projectId).toModel()
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

    fun getRecentEdits(): BridgeResult<List<RecentEdit>> =
        holder.wrapResult {
            holder.service.getRecentEdits().map { it.toModel() }
        }

    fun recordRecentEdit(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.recordRecentEdit(projectId, volumeId, chapterId)
        }

    fun flushRecentEdits(): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.flushRecentEdits()
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
