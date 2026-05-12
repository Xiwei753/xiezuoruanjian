package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.*

/**
 * A thin repository layer for the UI to interact with.
 *
 * It delegates all workspace logic to the underlying bridge/facade.
 * Under no circumstances should this class construct file paths or understand
 * the workspace format.
 */
class WorkspaceRepository(context: Context) {
    // Migration: We now use NativeCoreBridge instead of TemporaryWorkspaceBridge
    private val bridge = NativeCoreBridge(context)
    private val fallbackBridge = TemporaryWorkspaceBridge(context)

    init {
        bridge.createWorkspaceIfNeeded()
    }

    fun getProjects(): List<Project> {
        val projects = bridge.getProjects()
        return if (projects.isNotEmpty()) projects else fallbackBridge.getProjects()
    }

    fun getVolumes(projectId: String): List<Volume> {
        val volumes = bridge.getVolumes(projectId)
        return if (volumes.isNotEmpty()) volumes else fallbackBridge.getVolumes(projectId)
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        val chapters = bridge.getChapters(projectId, volumeId)
        return if (chapters.isNotEmpty()) chapters else fallbackBridge.getChapters(projectId, volumeId)
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        // Try native first. If native fails (e.g. no library), fallback is used in bridge, but let's just delegate.
        // For read, native bridge returns "" on failure. If "" is returned, fallback might also return "".
        // A better approach would be checking if JNI is loaded, but for now we try native, if empty try fallback.
        val nativeContent = bridge.getChapterContent(projectId, volumeId, chapterId)
        return if (nativeContent.isNotEmpty()) nativeContent else fallbackBridge.getChapterContent(projectId, volumeId, chapterId)
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        return bridge.saveChapterContent(projectId, volumeId, chapterId, content) || fallbackBridge.saveChapterContent(projectId, volumeId, chapterId, content)
    }

    fun createProject(title: String): Project {
        return bridge.createProject(title) ?: fallbackBridge.createProject(title)
    }

    fun createVolume(projectId: String, title: String): Volume {
        return bridge.createVolume(projectId, title) ?: fallbackBridge.createVolume(projectId, title)
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        return bridge.createChapter(projectId, volumeId, title) ?: fallbackBridge.createChapter(projectId, volumeId, title)
    }
}
