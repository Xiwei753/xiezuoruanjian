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
        return when (val result = bridge.getProjects()) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to get projects: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.getProjects()
        }
    }

    fun getVolumes(projectId: String): List<Volume> {
        return when (val result = bridge.getVolumes(projectId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to get volumes: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.getVolumes(projectId)
        }
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        return when (val result = bridge.getChapters(projectId, volumeId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to get chapters: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.getChapters(projectId, volumeId)
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        return when (val result = bridge.getChapterContent(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to get chapter content: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.getChapterContent(projectId, volumeId, chapterId)
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        return when (val result = bridge.saveChapterContent(projectId, volumeId, chapterId, content)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to save chapter content: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.saveChapterContent(projectId, volumeId, chapterId, content)
        }
    }

    fun createProject(title: String): Project {
        return when (val result = bridge.createProject(title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to create project: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.createProject(title)
        }
    }

    fun createVolume(projectId: String, title: String): Volume {
        return when (val result = bridge.createVolume(projectId, title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to create volume: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.createVolume(projectId, title)
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        return when (val result = bridge.createChapter(projectId, volumeId, title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RuntimeException("Failed to create chapter: ${result.message}")
            NativeResult.NotLoaded -> fallbackBridge.createChapter(projectId, volumeId, title)
        }
    }
}
