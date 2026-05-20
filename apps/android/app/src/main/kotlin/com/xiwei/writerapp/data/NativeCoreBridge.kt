package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.google.gson.GsonBuilder
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.*
import java.io.File
import android.content.Context
import android.content.pm.PackageManager
import android.Manifest

// Represents the result of a Native JNI call.
sealed class NativeResult<out T> {
    data class Success<out T>(val data: T) : NativeResult<T>()
    data class Error(val message: String) : NativeResult<Nothing>()
    object NotLoaded : NativeResult<Nothing>()
}

class NativeCoreBridge(context: Context) {
    private val workspaceDir = WorkspaceManager.getWorkspaceDir(context).absolutePath
    private val appContext = context.applicationContext
    private val gson = GsonBuilder()
        .registerTypeAdapter(SyncStatus::class.java, SyncStatusDeserializer())
        .create()

    var isLoaded = false
        private set

    // Cached permission states
    val hasInternetPermission: Boolean by lazy {
        appContext.checkSelfPermission(Manifest.permission.INTERNET) == PackageManager.PERMISSION_GRANTED
    }
    val hasAccessNetworkStatePermission: Boolean by lazy {
        appContext.checkSelfPermission(Manifest.permission.ACCESS_NETWORK_STATE) == PackageManager.PERMISSION_GRANTED
    }

    init {
        try {
            System.loadLibrary("writer_core_jni")
            isLoaded = true
        } catch (e: UnsatisfiedLinkError) {
            e.printStackTrace()
            isLoaded = false
        } catch (e: Throwable) {
            e.printStackTrace()
            isLoaded = false
        }
    }

    // Native methods
    private external fun createWorkspace(workspacePath: String): Boolean
    private external fun validateWorkspace(workspacePath: String): Boolean

    // Returns JSON string like {"success": true, "data": [...]} or {"success": false, "error": "..."}
    private external fun listProjects(workspacePath: String): String?
    private external fun getProjectStats(workspacePath: String, projectId: String): String?
    private external fun createProject(workspacePath: String, title: String): String?

    private external fun listVolumes(workspacePath: String, projectId: String): String?
    private external fun createVolume(workspacePath: String, projectId: String, title: String): String?

    private external fun listChapters(workspacePath: String, projectId: String, volumeId: String): String?
    private external fun createChapter(workspacePath: String, projectId: String, volumeId: String, title: String): String?

    private external fun readChapter(workspacePath: String, projectId: String, volumeId: String, chapterId: String): String?
    private external fun writeChapter(workspacePath: String, projectId: String, volumeId: String, chapterId: String, content: String): String?
    private external fun updateChapterNote(workspacePath: String, projectId: String, volumeId: String, chapterId: String, note: String): String?

    private external fun loadLocalSettings(workspacePath: String): String?
    private external fun saveLocalSettings(workspacePath: String, settingsJson: String): String?

    private external fun loadSyncableSettings(workspacePath: String): String?
    private external fun saveSyncableSettings(workspacePath: String, settingsJson: String): String?

    private external fun getRecentEdits(workspacePath: String): String?
    private external fun recordRecentEdit(workspacePath: String, projectId: String, volumeId: String, chapterId: String): String?
    private external fun loadSyncConfig(workspacePath: String): String?
    private external fun saveSyncConfig(workspacePath: String, configJson: String): String?
    private external fun loadSyncSecrets(workspacePath: String): String?
    private external fun saveSyncSecrets(workspacePath: String, secretsJson: String): String?
    private external fun performSyncDiagnostics(workspacePath: String, configJson: String): String?
    private external fun performSyncDryRun(workspacePath: String, configJson: String): String?
    private external fun performSync(workspacePath: String, configJson: String): String?


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
        } catch (e: Throwable) {
            e.printStackTrace()
        }
    }

    fun validateWorkspace(): Boolean {
        if (!isLoaded) return false
        return try {
            validateWorkspace(workspaceDir)
        } catch (e: Throwable) {
            e.printStackTrace()
            false
        }
    }

    fun getProjects(): NativeResult<List<Project>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = listProjects(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<List<Project>>>() {}.type
            val response: RustResponse<List<Project>> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data ?: emptyList())
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getRecentEdits(): NativeResult<List<RecentEdit>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = getRecentEdits(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<List<RecentEdit>>>() {}.type
            val response: RustResponse<List<RecentEdit>> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = recordRecentEdit(workspaceDir, projectId, volumeId, chapterId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = updateChapterNote(workspaceDir, projectId, volumeId, chapterId, note)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getProjectStats(projectId: String): NativeResult<ProjectStats> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = getProjectStats(workspaceDir, projectId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<ProjectStats>>() {}.type
            val response: RustResponse<ProjectStats> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createProject(title: String): NativeResult<Project> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = createProject(workspaceDir, title)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Project>>() {}.type
            val response: RustResponse<Project> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getVolumes(projectId: String): NativeResult<List<Volume>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = listVolumes(workspaceDir, projectId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<List<Volume>>>() {}.type
            val response: RustResponse<List<Volume>> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data ?: emptyList())
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createVolume(projectId: String, title: String): NativeResult<Volume> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = createVolume(workspaceDir, projectId, title)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Volume>>() {}.type
            val response: RustResponse<Volume> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getChapters(projectId: String, volumeId: String): NativeResult<List<ChapterMeta>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = listChapters(workspaceDir, projectId, volumeId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<List<ChapterMeta>>>() {}.type
            val response: RustResponse<List<ChapterMeta>> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data ?: emptyList())
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): NativeResult<ChapterMeta> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = createChapter(workspaceDir, projectId, volumeId, title)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<ChapterMeta>>() {}.type
            val response: RustResponse<ChapterMeta> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    // Update to return Pair to provide meta containing notes, or create a specific data class
    fun getChapterContentWithMeta(projectId: String, volumeId: String, chapterId: String): NativeResult<Pair<String, ChapterMeta>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = readChapter(workspaceDir, projectId, volumeId, chapterId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<RustChapterContent>>() {}.type
            val response: RustResponse<RustChapterContent> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(Pair(response.data.content, response.data.meta))
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): NativeResult<String> {
        return when (val result = getChapterContentWithMeta(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> NativeResult.Success(result.data.first)
            is NativeResult.Error -> result
            NativeResult.NotLoaded -> NativeResult.NotLoaded
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = writeChapter(workspaceDir, projectId, volumeId, chapterId, content)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getLocalSettings(): NativeResult<LocalSettings?> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = loadLocalSettings(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<LocalSettings>>() {}.type
            val response: RustResponse<LocalSettings> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun saveLocalSettings(settings: LocalSettings): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = saveLocalSettings(workspaceDir, gson.toJson(settings))
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getSyncableSettings(): NativeResult<SyncableSettings?> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = loadSyncableSettings(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<SyncableSettings>>() {}.type
            val response: RustResponse<SyncableSettings> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun saveSyncableSettings(settings: SyncableSettings): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = saveSyncableSettings(workspaceDir, gson.toJson(settings))
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun loadSyncConfig(): NativeResult<SyncConfig> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = loadSyncConfig(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<SyncConfig>>() {}.type
            val response: RustResponse<SyncConfig> = try {
                gson.fromJson(resultJson, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse SyncConfig JSON: ${e.message}")
            }
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun saveSyncConfig(config: SyncConfig): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = saveSyncConfig(workspaceDir, gson.toJson(config))
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun loadSyncSecrets(): NativeResult<SyncSecrets> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = loadSyncSecrets(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<SyncSecrets>>() {}.type
            val response: RustResponse<SyncSecrets> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun saveSyncSecrets(secrets: SyncSecrets): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = saveSyncSecrets(workspaceDir, gson.toJson(secrets))
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(resultJson, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun performSyncDiagnostics(config: SyncConfig): NativeResult<SyncDiagnosticsResult> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            // Inject Android permission status into config before sending to Rust
            val configWithPermissions = config.copy(
                androidHasInternetPermission = hasInternetPermission,
                androidHasAccessNetworkStatePermission = hasAccessNetworkStatePermission
            )
            val resultJson = performSyncDiagnostics(workspaceDir, gson.toJson(configWithPermissions))
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<SyncDiagnosticsResult>>() {}.type
            val response: RustResponse<SyncDiagnosticsResult> = try {
                gson.fromJson(resultJson, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse SyncDiagnosticsResult JSON: ${e.message}")
            }
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Exception) {
            e.printStackTrace()
            return NativeResult.Error("Exception calling performSyncDiagnostics: ${e.message}")
        }
    }

    fun performSyncDryRun(config: SyncConfig): NativeResult<SyncPlan> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = performSyncDryRun(workspaceDir, gson.toJson(config))
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<SyncPlan>>() {}.type
            val response: RustResponse<SyncPlan> = try {
                gson.fromJson(resultJson, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse SyncPlan JSON: ${e.message}")
            }
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun performSync(config: SyncConfig): NativeResult<SyncResult> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = performSync(workspaceDir, gson.toJson(config))
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<SyncResult>>() {}.type
            val response: RustResponse<SyncResult> = try {
                gson.fromJson(resultJson, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse SyncResult JSON: ${e.message}")
            }
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }


    private external fun listRegisteredActionsNative(workspacePath: String): String?
    private external fun executeActionNative(workspacePath: String, actionId: String, argsJson: String, contextJson: String): String?

    private external fun renameProjectNative(workspacePath: String, projectId: String, newTitle: String): String?
    private external fun deleteProjectNative(workspacePath: String, projectId: String): String?
    private external fun reorderProjectsNative(workspacePath: String, orderedIdsJson: String): String?

    private external fun renameVolumeNative(workspacePath: String, projectId: String, volumeId: String, newTitle: String): String?
    private external fun deleteVolumeNative(workspacePath: String, projectId: String, volumeId: String): String?
    private external fun reorderVolumesNative(workspacePath: String, projectId: String, orderedIdsJson: String): String?

    private external fun renameChapterNative(workspacePath: String, projectId: String, volumeId: String, chapterId: String, newTitle: String): String?
    private external fun deleteChapterNative(workspacePath: String, projectId: String, volumeId: String, chapterId: String): String?
    private external fun reorderChaptersNative(workspacePath: String, projectId: String, volumeId: String, orderedIdsJson: String): String?

    fun renameProject(projectId: String, newTitle: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = renameProjectNative(workspaceDir, projectId, newTitle)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteProject(projectId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = deleteProjectNative(workspaceDir, projectId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun reorderProjects(orderedProjectIds: List<String>): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val idsJson = gson.toJson(orderedProjectIds)
            val resultJson = reorderProjectsNative(workspaceDir, idsJson)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun renameVolume(projectId: String, volumeId: String, newTitle: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = renameVolumeNative(workspaceDir, projectId, volumeId, newTitle)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteVolume(projectId: String, volumeId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = deleteVolumeNative(workspaceDir, projectId, volumeId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun reorderVolumes(projectId: String, orderedVolumeIds: List<String>): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val idsJson = gson.toJson(orderedVolumeIds)
            val resultJson = reorderVolumesNative(workspaceDir, projectId, idsJson)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = renameChapterNative(workspaceDir, projectId, volumeId, chapterId, newTitle)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteChapter(projectId: String, volumeId: String, chapterId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = deleteChapterNative(workspaceDir, projectId, volumeId, chapterId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun reorderChapters(projectId: String, volumeId: String, orderedChapterIds: List<String>): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val idsJson = gson.toJson(orderedChapterIds)
            val resultJson = reorderChaptersNative(workspaceDir, projectId, volumeId, idsJson)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val response = gson.fromJson(resultJson, RustResponse::class.java)
            return if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }


    fun listRegisteredActions(): NativeResult<List<ActionDescriptor>> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = listRegisteredActionsNative(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")

            val type = object : TypeToken<RustResponse<List<ActionDescriptor>>>() {}.type
            val response: RustResponse<List<ActionDescriptor>> = try {
                gson.fromJson(resultJson, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse List<ActionDescriptor> JSON: ${e.message}")
            }

            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun executeAction(actionId: String, argsJson: String = "{}", contextJson: String = "{}"): NativeResult<ActionResult> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = executeActionNative(workspaceDir, actionId, argsJson, contextJson)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty response")

            val type = object : TypeToken<RustResponse<ActionResult>>() {}.type
            val response: RustResponse<ActionResult> = try {
                gson.fromJson(resultJson, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse ActionResult JSON: ${e.message}")
            }

            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }
}
