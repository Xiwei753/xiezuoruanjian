package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.*
import java.io.File
import android.content.Context

// Represents the result of a Native JNI call.
sealed class NativeResult<out T> {
    data class Success<out T>(val data: T) : NativeResult<T>()
    data class Error(val message: String) : NativeResult<Nothing>()
    object NotLoaded : NativeResult<Nothing>()
}

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

    fun getProjects(): NativeResult<List<Project>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = listProjects(workspaceDir)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<List<Project>>>() {}.type
            val response: RustResponse<List<Project>> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data ?: emptyList())
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun createProject(title: String): NativeResult<Project> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = createProject(workspaceDir, title)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<Project>>() {}.type
            val response: RustResponse<Project> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun getVolumes(projectId: String): NativeResult<List<Volume>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = listVolumes(workspaceDir, projectId)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<List<Volume>>>() {}.type
            val response: RustResponse<List<Volume>> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data ?: emptyList())
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun createVolume(projectId: String, title: String): NativeResult<Volume> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = createVolume(workspaceDir, projectId, title)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<Volume>>() {}.type
            val response: RustResponse<Volume> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun getChapters(projectId: String, volumeId: String): NativeResult<List<ChapterMeta>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = listChapters(workspaceDir, projectId, volumeId)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<List<ChapterMeta>>>() {}.type
            val response: RustResponse<List<ChapterMeta>> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data ?: emptyList())
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): NativeResult<ChapterMeta> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = createChapter(workspaceDir, projectId, volumeId, title)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<ChapterMeta>>() {}.type
            val response: RustResponse<ChapterMeta> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): NativeResult<String> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = readChapter(workspaceDir, projectId, volumeId, chapterId)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<RustChapterContent>>() {}.type
            val response: RustResponse<RustChapterContent> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data?.content ?: "")
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val success = writeChapter(workspaceDir, projectId, volumeId, chapterId, content)
            if (success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error("Native writeChapter returned false")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            NativeResult.NotLoaded
        }
    }

    fun getLocalSettings(): NativeResult<LocalSettings?> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = loadLocalSettings(workspaceDir)
            if (resultJson.isEmpty()) return NativeResult.Error("Empty response from native bridge")
            val type = object : TypeToken<RustResponse<LocalSettings>>() {}.type
            val response: RustResponse<LocalSettings> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            return NativeResult.NotLoaded
        }
    }

    fun saveLocalSettings(settings: LocalSettings): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val success = saveLocalSettings(workspaceDir, gson.toJson(settings))
            if (success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error("Native saveLocalSettings returned false")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            NativeResult.Error(e.message ?: "Exception occurred")
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            NativeResult.NotLoaded
        }
    }
}
