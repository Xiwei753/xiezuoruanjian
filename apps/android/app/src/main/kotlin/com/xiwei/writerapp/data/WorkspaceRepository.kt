package com.xiwei.writerapp.data

//! # 仓库层封装（Android UI 层 - Data 层）
//!
//! 对领域 Bridge 的高层封装，将 BridgeResult 转换为异常。
//!
//! ## 架构定位
//!
//! ```text
//! ViewModel → WorkspaceRepository → 领域 Bridge → JNI → Rust Core
//! ```
//!
//! ## 职责边界
//!
//! - **做**：统一错误处理（BridgeResult → RepositoryException）、简化 API 调用
//! - **不做**：业务逻辑（只做类型转换和错误传播）
//!
//! ## 设计原则
//!
//! - 所有方法在失败时抛出 RepositoryException，由 ViewModel 捕获
//! - 不吞掉错误，不伪造成功状态
//! - 不添加任何业务逻辑，只做 BridgeResult ↔ Exception 转换

import android.content.Context
import com.xiwei.writerapp.model.*

class WorkspaceRepository(val context: Context) {
    private val nativeBridge = NativeCoreBridge(context)
    private val workspaceBridge = WorkspaceBridge(nativeBridge)
    private val writingBridge = WritingBridge(nativeBridge)
    private val statsBridge = StatsBridge(nativeBridge)

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

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        return when (val result = writingBridge.saveChapterContent(projectId, volumeId, chapterId, content)) {
            is BridgeResult.Success -> true
            is BridgeResult.Error -> throw RepositoryException("保存章节内容失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): Boolean {
        return when (val result = writingBridge.clearChapterContent(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> true
            is BridgeResult.Error -> throw RepositoryException("清空章节内容失败: ${result.message}")
            BridgeResult.NotLoaded -> throw RepositoryException("Native库未加载")
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
        when (val result = nativeBridge.renameProject(projectId, newTitle)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重命名作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteProject(projectId: String) {
        when (val result = nativeBridge.deleteProject(projectId)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("删除作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderProjects(orderedProjectIds: List<String>) {
        when (val result = nativeBridge.reorderProjects(orderedProjectIds)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重排作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameVolume(projectId: String, volumeId: String, newTitle: String) {
        when (val result = nativeBridge.renameVolume(projectId, volumeId, newTitle)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重命名分卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteVolume(projectId: String, volumeId: String) {
        when (val result = nativeBridge.deleteVolume(projectId, volumeId)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("删除分卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderVolumes(projectId: String, orderedVolumeIds: List<String>) {
        when (val result = nativeBridge.reorderVolumes(projectId, orderedVolumeIds)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重排分卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String) {
        when (val result = nativeBridge.renameChapter(projectId, volumeId, chapterId, newTitle)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重命名章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun deleteChapter(projectId: String, volumeId: String, chapterId: String) {
        when (val result = nativeBridge.deleteChapter(projectId, volumeId, chapterId)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("删除章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun reorderChapters(projectId: String, volumeId: String, orderedChapterIds: List<String>) {
        when (val result = nativeBridge.reorderChapters(projectId, volumeId, orderedChapterIds)) {
            is NativeResult.Success<*> -> {}
            is NativeResult.Error -> throw RepositoryException("重排章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
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
