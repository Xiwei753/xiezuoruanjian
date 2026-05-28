package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.*

sealed class BridgeResult<out T> {
    data class Success<out T>(val data: T) : BridgeResult<T>()
    data class Error(val error: BridgeError) : BridgeResult<Nothing>() {
        val message: String get() = error.message
        val code: BridgeErrorCode get() = error.code
    }
    object NotLoaded : BridgeResult<Nothing>()
}

private fun <T> NativeResult<T>.toBridgeResult(): BridgeResult<T> {
    return when (this) {
        is NativeResult.Success -> BridgeResult.Success(data)
        is NativeResult.Error -> BridgeResult.Error(bridgeError)
        NativeResult.NotLoaded -> BridgeResult.NotLoaded
    }
}

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
}

class WritingBridge(private val nativeBridge: NativeCoreBridge) {
    fun openChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterOpenResult> =
        nativeBridge.openChapter(projectId, volumeId, chapterId).toBridgeResult()
    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<ChapterSaveReceipt> =
        nativeBridge.saveChapterContentReceipt(projectId, volumeId, chapterId, content).toBridgeResult()
    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterSaveReceipt> =
        nativeBridge.clearChapterContentReceipt(projectId, volumeId, chapterId).toBridgeResult()
    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): BridgeResult<Boolean> =
        nativeBridge.updateChapterNote(projectId, volumeId, chapterId, note).toBridgeResult()
    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, sessionId: String): Boolean =
        nativeBridge.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId)
    fun calculateWordCount(text: String): Int = nativeBridge.calculateWordCount(text)
    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, sessionId: String): Boolean =
        nativeBridge.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, sessionId)
}

class StatsBridge(private val nativeBridge: NativeCoreBridge) {
    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> = nativeBridge.getProjectStats(projectId).toBridgeResult()
    fun flushWritingStats() = nativeBridge.flushWritingStats()
}

class StarMapBridge(private val nativeBridge: NativeCoreBridge) {
    fun listStarmaps(): BridgeResult<List<StarMapMeta>> = nativeBridge.listStarmaps().toBridgeResult()
    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> = nativeBridge.createStarmap(title, desc).toBridgeResult()
    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> = nativeBridge.getStarmapGraph(starmapId).toBridgeResult()
    fun addStarmapNode(starmapId: String, node: StarMapGraphNode): BridgeResult<StarMapGraphNode> =
        nativeBridge.addStarmapNode(starmapId, node).toBridgeResult()
    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> =
        nativeBridge.saveStarmapLayout(starmapId, layout).toBridgeResult()
}
