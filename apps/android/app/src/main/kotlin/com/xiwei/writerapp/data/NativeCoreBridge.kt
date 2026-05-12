package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.*
import java.io.File
import android.content.Context

class NativeCoreBridge(context: Context) {
    private val workspaceDir = WorkspaceManager.getWorkspaceDir(context).absolutePath
    private val gson = Gson()

    var isLoaded = false
        private set

    init {
        try {
            System.loadLibrary("writer_core_jni")
            isLoaded = true
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            isLoaded = false
        }
    }

    // Native methods
    private external fun createWorkspace(workspacePath: String): Boolean
    private external fun validateWorkspace(workspacePath: String): Boolean

    // Returns JSON string like {"success": true, "data": [...]} or {"success": false, "error": "..."}
    private external fun listProjects(workspacePath: String): String
    private external fun createProject(workspacePath: String, title: String): String

    private external fun listVolumes(workspacePath: String, projectId: String): String
    private external fun createVolume(workspacePath: String, projectId: String, title: String): String

    private external fun listChapters(workspacePath: String, projectId: String, volumeId: String): String
    private external fun createChapter(workspacePath: String, projectId: String, volumeId: String, title: String): String

    private external fun readChapter(workspacePath: String, projectId: String, volumeId: String, chapterId: String): String
    private external fun writeChapter(workspacePath: String, projectId: String, volumeId: String, chapterId: String, content: String): Boolean

    private external fun loadLocalSettings(workspacePath: String): String
    private external fun saveLocalSettings(workspacePath: String, settingsJson: String): Boolean

    // Helper classes for parsing Rust JSON responses
    private data class RustResponse<T>(
        val success: Boolean,
        val data: T?,
        val error: String?
    )

    private data class RustChapterContent(
        val meta: ChapterMeta,
        val content: String
    )

    fun createWorkspaceIfNeeded() {
        if (!isLoaded) return
        try {
            createWorkspace(workspaceDir)
        } catch (e: Exception) {
            e.printStackTrace()
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
        }
    }

    fun validateWorkspace(): Boolean {
        if (!isLoaded) return false
        return try {
            validateWorkspace(workspaceDir)
        } catch (e: Exception) {
            e.printStackTrace()
            false
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            false
        }
    }

    fun getProjects(): List<Project> {
        if (!isLoaded) return emptyList()
        try {
            val resultJson = listProjects(workspaceDir)
            if (resultJson.isEmpty()) return emptyList()
            val type = object : TypeToken<RustResponse<List<Project>>>() {}.type
            val response: RustResponse<List<Project>> = gson.fromJson(resultJson, type)
            return if (response.success) response.data ?: emptyList() else emptyList()
        } catch (e: Exception) {
            e.printStackTrace()
            return emptyList()
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return emptyList()
        }
    }

    fun createProject(title: String): Project? {
        if (!isLoaded) return null
        try {
            val resultJson = createProject(workspaceDir, title)
            if (resultJson.isEmpty()) return null
            val type = object : TypeToken<RustResponse<Project>>() {}.type
            val response: RustResponse<Project> = gson.fromJson(resultJson, type)
            return if (response.success) response.data else null
        } catch (e: Exception) {
            e.printStackTrace()
            return null
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return null
        }
    }

    fun getVolumes(projectId: String): List<Volume> {
        if (!isLoaded) return emptyList()
        try {
            val resultJson = listVolumes(workspaceDir, projectId)
            if (resultJson.isEmpty()) return emptyList()
            val type = object : TypeToken<RustResponse<List<Volume>>>() {}.type
            val response: RustResponse<List<Volume>> = gson.fromJson(resultJson, type)
            return if (response.success) response.data ?: emptyList() else emptyList()
        } catch (e: Exception) {
            e.printStackTrace()
            return emptyList()
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return emptyList()
        }
    }

    fun createVolume(projectId: String, title: String): Volume? {
        if (!isLoaded) return null
        try {
            val resultJson = createVolume(workspaceDir, projectId, title)
            if (resultJson.isEmpty()) return null
            val type = object : TypeToken<RustResponse<Volume>>() {}.type
            val response: RustResponse<Volume> = gson.fromJson(resultJson, type)
            return if (response.success) response.data else null
        } catch (e: Exception) {
            e.printStackTrace()
            return null
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return null
        }
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        if (!isLoaded) return emptyList()
        try {
            val resultJson = listChapters(workspaceDir, projectId, volumeId)
            if (resultJson.isEmpty()) return emptyList()
            val type = object : TypeToken<RustResponse<List<ChapterMeta>>>() {}.type
            val response: RustResponse<List<ChapterMeta>> = gson.fromJson(resultJson, type)
            return if (response.success) response.data ?: emptyList() else emptyList()
        } catch (e: Exception) {
            e.printStackTrace()
            return emptyList()
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return emptyList()
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta? {
        if (!isLoaded) return null
        try {
            val resultJson = createChapter(workspaceDir, projectId, volumeId, title)
            if (resultJson.isEmpty()) return null
            val type = object : TypeToken<RustResponse<ChapterMeta>>() {}.type
            val response: RustResponse<ChapterMeta> = gson.fromJson(resultJson, type)
            return if (response.success) response.data else null
        } catch (e: Exception) {
            e.printStackTrace()
            return null
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return null
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        if (!isLoaded) return ""
        try {
            val resultJson = readChapter(workspaceDir, projectId, volumeId, chapterId)
            if (resultJson.isEmpty()) return ""
            val type = object : TypeToken<RustResponse<RustChapterContent>>() {}.type
            val response: RustResponse<RustChapterContent> = gson.fromJson(resultJson, type)
            return if (response.success) response.data?.content ?: "" else ""
        } catch (e: Exception) {
            e.printStackTrace()
            return ""
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return ""
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        if (!isLoaded) return false
        return try {
            writeChapter(workspaceDir, projectId, volumeId, chapterId, content)
        } catch (e: Exception) {
            e.printStackTrace()
            false
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            false
        }
    }

    fun getLocalSettings(): LocalSettings? {
        if (!isLoaded) return null
        try {
            val resultJson = loadLocalSettings(workspaceDir)
            if (resultJson.isEmpty()) return null
            val type = object : TypeToken<RustResponse<LocalSettings>>() {}.type
            val response: RustResponse<LocalSettings> = gson.fromJson(resultJson, type)
            return if (response.success) response.data else null
        } catch (e: Exception) {
            e.printStackTrace()
            return null
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return null
        }
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        if (!isLoaded) return false
        return try {
            saveLocalSettings(workspaceDir, gson.toJson(settings))
        } catch (e: Exception) {
            e.printStackTrace()
            false
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            false
        }
    }
}
