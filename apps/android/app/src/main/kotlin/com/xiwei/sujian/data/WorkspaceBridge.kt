package com.xiwei.sujian.data

import com.xiwei.sujian.model.ChapterMeta
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.RecentEdit
import com.xiwei.sujian.model.Volume

/**
 * Android 端工作区 CRUD 桥接层 — 委托 Core `WriterAppService` 的工作区方法。
 *
 * 覆盖项目/卷/章节的增删改查和排序，以及最近编辑记录。
 * 所有方法通过 [AppServiceBridge] 访问 UniFFI 生成的 Core 服务，
 * 返回 [BridgeResult] 封装成功/失败。调用方必须在 UI 线程执行。
 *
 * ## `recordRecentEdit` 防抖
 *
 * Core 侧有 5 秒防抖（同一章节 5 秒内多次调用只记录一次），
 * Android 端无需额外防抖，直接委托即可。
 */
class WorkspaceBridge(val appService: AppServiceBridge) {
    fun listProjects(): BridgeResult<List<Project>> = appService.listProjects()
    fun getProjects(): BridgeResult<List<Project>> = listProjects()
    fun getRecentEdits(): BridgeResult<List<RecentEdit>> = appService.getRecentEdits()
    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String): BridgeResult<Boolean> = appService.recordRecentEdit(projectId, volumeId, chapterId)
    fun flushRecentEdits(): BridgeResult<Boolean> = appService.flushRecentEdits()
    fun validateWorkspace(): BridgeResult<Boolean> = appService.validateWorkspace()
    fun createWorkspaceIfNeeded(): BridgeResult<Boolean> = appService.createWorkspaceIfNeeded()
    fun createProject(title: String): BridgeResult<Project> = appService.createProject(title)
    fun renameProject(projectId: String, newTitle: String): BridgeResult<Boolean> = appService.renameProject(projectId, newTitle)
    fun deleteProject(projectId: String): BridgeResult<Boolean> = appService.deleteProject(projectId)
    fun reorderProjects(orderedIds: List<String>): BridgeResult<Boolean> = appService.reorderProjects(orderedIds)
    fun listVolumes(projectId: String): BridgeResult<List<Volume>> = appService.listVolumes(projectId)
    fun getVolumes(projectId: String): BridgeResult<List<Volume>> = listVolumes(projectId)
    fun createVolume(projectId: String, title: String): BridgeResult<Volume> = appService.createVolume(projectId, title)
    fun renameVolume(projectId: String, volumeId: String, newTitle: String): BridgeResult<Boolean> = appService.renameVolume(projectId, volumeId, newTitle)
    fun deleteVolume(projectId: String, volumeId: String): BridgeResult<Boolean> = appService.deleteVolume(projectId, volumeId)
    fun reorderVolumes(projectId: String, orderedIds: List<String>): BridgeResult<Boolean> = appService.reorderVolumes(projectId, orderedIds)
    fun listChapters(projectId: String, volumeId: String): BridgeResult<List<ChapterMeta>> = appService.listChapters(projectId, volumeId)
    fun getChapters(projectId: String, volumeId: String): BridgeResult<List<ChapterMeta>> = listChapters(projectId, volumeId)
    fun createChapter(projectId: String, volumeId: String, title: String): BridgeResult<ChapterMeta> = appService.createChapter(projectId, volumeId, title)
    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String): BridgeResult<Boolean> = appService.renameChapter(projectId, volumeId, chapterId, newTitle)
    fun deleteChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<Boolean> = appService.deleteChapter(projectId, volumeId, chapterId)
    fun reorderChapters(projectId: String, volumeId: String, orderedIds: List<String>): BridgeResult<Boolean> = appService.reorderChapters(projectId, volumeId, orderedIds)
}
