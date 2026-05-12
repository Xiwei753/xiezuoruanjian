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
    private val bridge = TemporaryWorkspaceBridge(context)

    fun getProjects(): List<Project> {
        return bridge.getProjects()
    }

    fun getVolumes(projectId: String): List<Volume> {
        return bridge.getVolumes(projectId)
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        return bridge.getChapters(projectId, volumeId)
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        return bridge.getChapterContent(projectId, volumeId, chapterId)
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        return bridge.saveChapterContent(projectId, volumeId, chapterId, content)
    }

    fun createProject(title: String): Project {
        return bridge.createProject(title)
    }

    fun createVolume(projectId: String, title: String): Volume {
        return bridge.createVolume(projectId, title)
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        return bridge.createChapter(projectId, volumeId, title)
    }
}
