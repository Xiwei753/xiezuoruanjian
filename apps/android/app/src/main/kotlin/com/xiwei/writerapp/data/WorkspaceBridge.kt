package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.Project
import com.xiwei.writerapp.model.RecentEdit
import com.xiwei.writerapp.model.Volume
import com.xiwei.writerapp.model.ChapterMeta

class WorkspaceBridge(private val nativeBridge: NativeCoreBridge) {
    fun createWorkspaceIfNeeded() = nativeBridge.createWorkspaceIfNeeded()
    fun validateWorkspace(): Boolean = nativeBridge.validateWorkspace()
    fun getProjects(): BridgeResult<List<Project>> = nativeBridge.getProjects().toBridgeResult()
    fun getRecentEdits(): BridgeResult<List<RecentEdit>> = nativeBridge.getRecentEdits().toBridgeResult()
    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String): BridgeResult<Boolean> =
        nativeBridge.recordRecentEdit(projectId, volumeId, chapterId).toBridgeResult()
    fun getVolumes(projectId: String): BridgeResult<List<Volume>> = nativeBridge.getVolumes(projectId).toBridgeResult()
    fun createVolume(projectId: String, title: String): BridgeResult<Volume> =
        nativeBridge.createVolume(projectId, title).toBridgeResult()
    fun getChapters(projectId: String, volumeId: String): BridgeResult<List<ChapterMeta>> =
        nativeBridge.getChapters(projectId, volumeId).toBridgeResult()
    fun createProject(title: String): BridgeResult<Project> = nativeBridge.createProject(title).toBridgeResult()
    fun createChapter(projectId: String, volumeId: String, title: String): BridgeResult<ChapterMeta> =
        nativeBridge.createChapter(projectId, volumeId, title).toBridgeResult()

    fun renameProject(projectId: String, newTitle: String): BridgeResult<Any> =
        nativeBridge.renameProject(projectId, newTitle).toBridgeResult()
    fun deleteProject(projectId: String): BridgeResult<Any> =
        nativeBridge.deleteProject(projectId).toBridgeResult()
    fun reorderProjects(orderedProjectIds: List<String>): BridgeResult<Any> =
        nativeBridge.reorderProjects(orderedProjectIds).toBridgeResult()

    fun renameVolume(projectId: String, volumeId: String, newTitle: String): BridgeResult<Any> =
        nativeBridge.renameVolume(projectId, volumeId, newTitle).toBridgeResult()
    fun deleteVolume(projectId: String, volumeId: String): BridgeResult<Any> =
        nativeBridge.deleteVolume(projectId, volumeId).toBridgeResult()
    fun reorderVolumes(projectId: String, orderedVolumeIds: List<String>): BridgeResult<Any> =
        nativeBridge.reorderVolumes(projectId, orderedVolumeIds).toBridgeResult()

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String): BridgeResult<Any> =
        nativeBridge.renameChapter(projectId, volumeId, chapterId, newTitle).toBridgeResult()
    fun deleteChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<Any> =
        nativeBridge.deleteChapter(projectId, volumeId, chapterId).toBridgeResult()
    fun reorderChapters(projectId: String, volumeId: String, orderedChapterIds: List<String>): BridgeResult<Any> =
        nativeBridge.reorderChapters(projectId, volumeId, orderedChapterIds).toBridgeResult()
}
