package com.xiwei.writerapp.data

import android.content.Context
import com.xiwei.writerapp.model.*

class WorkspaceRepository(context: Context) {
    private val bridge = NativeCoreBridge(context)

    init {
        bridge.createWorkspaceIfNeeded()
    }

    fun getProjects(): List<Project> {
        return when (val result = bridge.getProjects()) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取作品列表失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getVolumes(projectId: String): List<Volume> {
        return when (val result = bridge.getVolumes(projectId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取卷列表失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        return when (val result = bridge.getChapters(projectId, volumeId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取章节列表失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        return when (val result = bridge.getChapterContent(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("获取章节内容失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        return when (val result = bridge.saveChapterContent(projectId, volumeId, chapterId, content)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("保存章节内容失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createProject(title: String): Project {
        return when (val result = bridge.createProject(title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("创建作品失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createVolume(projectId: String, title: String): Volume {
        return when (val result = bridge.createVolume(projectId, title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("创建卷失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        return when (val result = bridge.createChapter(projectId, volumeId, title)) {
            is NativeResult.Success -> result.data
            is NativeResult.Error -> throw RepositoryException("创建章节失败: ${result.message}")
            NativeResult.NotLoaded -> throw RepositoryException("Native库未加载")
        }
    }
}
