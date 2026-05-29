package com.xiwei.writerapp.data

//! # JNI 兼容适配器（Android Data 层）
//!
//! 负责加载 native 库，并把旧 JNI 返回包装转换为 Kotlin DTO。
//!
//! ## 架构定位
//!
/**
 * `NativeCoreBridge` 是负责与 Rust JNI 层通信的 legacy/internal 适配器。
 *
 * 【架构边界警告 - 桥接第四阶段最终收口】
 * - `NativeCoreBridge` 是遗留的 JSON over JNI 兼容中心。
 * - 只有 `BridgeProvider` 和 `WorkspaceBridge`, `WritingBridge` 等专用领域 Bridge 允许直接调用它。
 * - Repository, ViewModel, Activity, Controller 严禁直接依赖此类。
 * - 新业务功能禁止继续向上暴露裸 JSON String / Boolean / null，必须新增或复用领域 Bridge。
 *
 * 该类目前主要维护旧有功能的序列化/反序列化逻辑，并将 JNI 返回的 JSON 字符串映射为 `NativeResult<T>`。
 */
//!
//! ## 职责边界
//!
//! - **做**：加载 native 库、调用 JNI 函数、兼容旧 JSON 包装
//! - **不做**：业务逻辑（全部委托给 Rust Core）
//! - **不做**：面向 UI 暴露大杂烩入口（新调用应通过领域 Bridge）
//!
//! ## 注意事项
//!
//! - @Deprecated("Legacy internal JNI bridge. UI/Repository 应该使用 Domain Bridges (如 WritingBridge) 而非直接调用此类。")
//! - `isLoaded` 标记 native 库是否加载成功，所有方法在调用前检查此标记
//! - 旧兼容包装为 `{ "success": true/false, "data": ..., "code": ..., "error": ... }`

import com.google.gson.Gson
import com.google.gson.GsonBuilder
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.*
import java.io.File
import android.content.Context
import android.content.pm.PackageManager
import android.Manifest

/// JNI 调用结果密封类，仅限 legacy adapter 和领域 Bridge 内部使用。
internal sealed class NativeResult<out T> {
    data class Success<out T>(val data: T) : NativeResult<T>()
    data class Error(val bridgeError: BridgeError) : NativeResult<Nothing>() {
        constructor(message: String) : this(BridgeError(BridgeErrorCode.Unknown, message))

        val message: String get() = bridgeError.message
        val code: BridgeErrorCode get() = bridgeError.code
    }
    object NotLoaded : NativeResult<Nothing>()
}

internal class NativeCoreBridge(context: Context) {
    private val workspaceDir = WorkspaceManager.getWorkspaceDir(context).absolutePath
    private val appContext = context.applicationContext
    private val gson = GsonBuilder()
        .registerTypeAdapter(SyncStatus::class.java, SyncStatusDeserializer())
        .registerTypeAdapter(com.xiwei.writerapp.model.MindMapNodeKind::class.java, com.xiwei.writerapp.model.MindMapNodeKindDeserializer())
        .registerTypeAdapter(com.xiwei.writerapp.model.StarMapNodeKind::class.java, com.xiwei.writerapp.model.StarMapNodeKindDeserializer())
        .registerTypeAdapter(com.xiwei.writerapp.model.StarMapEdgeKind::class.java, com.xiwei.writerapp.model.StarMapEdgeKindDeserializer())
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
    private external fun clearChapterContentNative(workspacePath: String, projectId: String, volumeId: String, chapterId: String): String?
    private external fun updateChapterNote(workspacePath: String, projectId: String, volumeId: String, chapterId: String, note: String): String?

    private external fun loadLocalSettings(workspacePath: String): String?
    private external fun saveLocalSettings(workspacePath: String, settingsJson: String): String?

    private external fun loadSyncableSettings(workspacePath: String): String?
    private external fun saveSyncableSettings(workspacePath: String, settingsJson: String): String?

    private external fun getRecentEdits(workspacePath: String): String?
    private external fun recordRecentEdit(workspacePath: String, projectId: String, volumeId: String, chapterId: String): String?
    private external fun getMindMapSnapshotJsonNative(workspacePath: String, projectId: String): String?
    private external fun loadSyncConfig(workspacePath: String): String?
    private external fun saveSyncConfig(workspacePath: String, configJson: String): String?
    private external fun loadSyncSecrets(workspacePath: String): String?
    private external fun saveSyncSecrets(workspacePath: String, secretsJson: String): String?
    private external fun loadSyncState(workspacePath: String): String?
    private external fun performSyncDiagnostics(workspacePath: String, configJson: String): String?
    private external fun performSyncDryRun(workspacePath: String, configJson: String): String?
    private external fun performSync(workspacePath: String, configJson: String): String?

    private external fun recordWritingEventNative(
        workspacePath: String,
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
    ): Boolean

    private external fun flushWritingStatsNative(workspacePath: String)

    private external fun getWritingStatsSummaryNative(
        workspacePath: String,
        startDate: String,
        endDate: String
    ): String?


    // Helper classes for parsing Rust JSON responses
    private data class RustResponse<T>(
        val success: Boolean,
        val data: T?,
        val code: String?,
        val error: String?
    )

    private data class RustChapterContent(
        val meta: ChapterMeta,
        val content: String
    )

    private fun <T> nativeError(response: RustResponse<T>, fallback: String = "Unknown error"): NativeResult.Error {
        return NativeResult.Error(
            BridgeError(
                BridgeErrorCode.fromWire(response.code),
                response.error ?: fallback
            )
        )
    }

    fun createWorkspaceIfNeeded() {
        if (!isLoaded) return
        try {
            createWorkspace(workspaceDir)
        } catch (e: Throwable) {
            e.printStackTrace()
        }
    }

    fun workspaceDirPath(): String = workspaceDir

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

    fun openChapter(projectId: String, volumeId: String, chapterId: String): NativeResult<ChapterOpenResult> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = readChapter(workspaceDir, projectId, volumeId, chapterId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<ChapterOpenResult>>() {}.type
            val response: RustResponse<ChapterOpenResult> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                nativeError(response)
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    @Deprecated("Legacy compatibility helper. Use WritingBridge.openChapter instead.")
    fun getChapterContentWithMeta(projectId: String, volumeId: String, chapterId: String): NativeResult<Pair<String, ChapterMeta>> {
        return when (val result = openChapter(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> NativeResult.Success(Pair(result.data.content, result.data.meta))
            is NativeResult.Error -> result
            NativeResult.NotLoaded -> NativeResult.NotLoaded
        }
    }

    @Deprecated("Legacy compatibility helper. Use WritingBridge.openChapter instead.")
    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): NativeResult<String> {
        return when (val result = getChapterContentWithMeta(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> NativeResult.Success(result.data.first)
            is NativeResult.Error -> result
            NativeResult.NotLoaded -> NativeResult.NotLoaded
        }
    }

    fun saveChapterContentReceipt(projectId: String, volumeId: String, chapterId: String, content: String): NativeResult<ChapterSaveReceipt> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = writeChapter(workspaceDir, projectId, volumeId, chapterId, content)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<ChapterSaveReceipt>>() {}.type
            val response: RustResponse<ChapterSaveReceipt> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                nativeError(response)
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    @Deprecated("Legacy Boolean entrypoint. Use WritingBridge.saveChapterContent for ChapterSaveReceipt and BridgeResult errors.")
    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): NativeResult<Boolean> {
        return when (val result = saveChapterContentReceipt(projectId, volumeId, chapterId, content)) {
            is NativeResult.Success -> NativeResult.Success(true)
            is NativeResult.Error -> result
            NativeResult.NotLoaded -> NativeResult.NotLoaded
        }
    }

    fun clearChapterContentReceipt(projectId: String, volumeId: String, chapterId: String): NativeResult<ChapterSaveReceipt> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = clearChapterContentNative(workspaceDir, projectId, volumeId, chapterId)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<ChapterSaveReceipt>>() {}.type
            val response: RustResponse<ChapterSaveReceipt> = gson.fromJson(resultJson, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                nativeError(response)
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) {
                return NativeResult.NotLoaded
            }
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    @Deprecated("Legacy Boolean entrypoint. Use WritingBridge.clearChapterContent for ChapterSaveReceipt and BridgeResult errors.")
    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): NativeResult<Boolean> {
        return when (val result = clearChapterContentReceipt(projectId, volumeId, chapterId)) {
            is NativeResult.Success -> NativeResult.Success(true)
            is NativeResult.Error -> result
            NativeResult.NotLoaded -> NativeResult.NotLoaded
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
        if (!isLoaded) return false
        return try {
            recordWritingEventNative(
                workspaceDir, deviceId, projectId, volumeId, chapterId,
                source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId
            )
        } catch (e: Throwable) {
            e.printStackTrace()
            false
        }
    }

    fun flushWritingStats() {
        if (!isLoaded) return
        try {
            flushWritingStatsNative(workspaceDir)
        } catch (e: Throwable) {
            e.printStackTrace()
        }
    }

    fun getWritingStatsSummary(startDate: String, endDate: String): NativeResult<StatsSummary> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = getWritingStatsSummaryNative(workspaceDir, startDate, endDate)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")

            // StatsSummary was previously parsed directly from JSON string (not wrapped in RustResponse in StatsBridge)
            // But let's try RustResponse first or fallback to direct.
            // Based on previous StatsBridge code: val summary = Gson().fromJson(json, StatsSummary::class.java)
            // We will do direct parsing as per legacy behavior.
            val summary = gson.fromJson(json, StatsSummary::class.java)
            NativeResult.Success(summary)
        } catch (e: Throwable) {
            e.printStackTrace()
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
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

    fun loadSyncState(): NativeResult<SyncState> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val resultJson = loadSyncState(workspaceDir)
            if (resultJson.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<SyncState>>() {}.type
            val response: RustResponse<SyncState> = try {
                gson.fromJson(resultJson, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse SyncState JSON: ${e.message}")
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


    fun getMindMapSnapshot(projectId: String): NativeResult<MindMapSnapshot> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val json = getMindMapSnapshotJsonNative(workspaceDir, projectId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val jsonBytesLength = json.toByteArray(Charsets.UTF_8).size

            val type = object : TypeToken<RustResponse<MindMapSnapshot>>() {}.type
            val startTime = System.currentTimeMillis()
            val response: RustResponse<MindMapSnapshot> = try {
                gson.fromJson(json, type)
            } catch (e: Exception) {
                return NativeResult.Error("Failed to parse MindMapSnapshot JSON: ${e.message}")
            }
            val parseTimeMs = System.currentTimeMillis() - startTime

            return if (response.success && response.data != null) {
                response.data.parseTimeMs = parseTimeMs
                response.data.jsonBytes = jsonBytesLength
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

    private external fun createMindMapGraphJson(workspacePath: String, projectId: String, title: String): String?
    private external fun listMindMapGraphsJson(workspacePath: String, projectId: String): String?
    private external fun setDefaultMindMapGraphJson(workspacePath: String, projectId: String, graphId: String): String?
    private external fun createMindMapNodeJson(workspacePath: String, projectId: String, graphId: String, nodeJson: String): String?
    private external fun updateMindMapNodeJson(workspacePath: String, projectId: String, graphId: String, nodeId: String, patchJson: String): String?
    private external fun deleteMindMapNodeJson(workspacePath: String, projectId: String, graphId: String, nodeId: String, cascade: Boolean): String?
    private external fun createMindMapEdgeJson(workspacePath: String, projectId: String, graphId: String, edgeJson: String): String?
    private external fun updateMindMapEdgeJson(workspacePath: String, projectId: String, graphId: String, edgeId: String, patchJson: String): String?
    private external fun deleteMindMapEdgeJson(workspacePath: String, projectId: String, graphId: String, edgeId: String): String?
    private external fun createMindMapAnchorJson(workspacePath: String, projectId: String, graphId: String, anchorJson: String): String?
    private external fun bindMindMapAnchorJson(workspacePath: String, projectId: String, graphId: String, nodeId: String, anchorId: String, linkKind: String): String?
    private external fun saveMindMapLayoutJson(workspacePath: String, projectId: String, graphId: String, layoutJson: String): String?

    private external fun aiAvailableNative(workspacePath: String): Boolean

    fun aiAvailable(): Boolean {
        if (!isLoaded) return false
        return try {
            aiAvailableNative(workspaceDir)
        } catch (e: Throwable) {
            false
        }
    }

    fun createMindMapGraph(projectId: String, title: String): NativeResult<MindMapGraph> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val json = createMindMapGraphJson(workspaceDir, projectId, title)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapGraph>>() {}.type
            val response: RustResponse<MindMapGraph> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun listMindMapGraphs(projectId: String): NativeResult<MindMapGraphsList> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val json = listMindMapGraphsJson(workspaceDir, projectId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapGraphsList>>() {}.type
            val response: RustResponse<MindMapGraphsList> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun setDefaultMindMapGraph(projectId: String, graphId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val json = setDefaultMindMapGraphJson(workspaceDir, projectId, graphId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createMindMapNode(projectId: String, graphId: String, node: MindMapGraphNode): NativeResult<MindMapGraphNode> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val nodeJson = gson.toJson(node)
            val json = createMindMapNodeJson(workspaceDir, projectId, graphId, nodeJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapGraphNode>>() {}.type
            val response: RustResponse<MindMapGraphNode> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun updateMindMapNode(projectId: String, graphId: String, nodeId: String, patch: MindMapGraphNodePatch): NativeResult<MindMapGraphNode> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val patchJson = gson.toJson(patch)
            val json = updateMindMapNodeJson(workspaceDir, projectId, graphId, nodeId, patchJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapGraphNode>>() {}.type
            val response: RustResponse<MindMapGraphNode> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteMindMapNode(projectId: String, graphId: String, nodeId: String, cascade: Boolean): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val json = deleteMindMapNodeJson(workspaceDir, projectId, graphId, nodeId, cascade)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createMindMapEdge(projectId: String, graphId: String, edge: MindMapGraphEdge): NativeResult<MindMapGraphEdge> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val edgeJson = gson.toJson(edge)
            val json = createMindMapEdgeJson(workspaceDir, projectId, graphId, edgeJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapGraphEdge>>() {}.type
            val response: RustResponse<MindMapGraphEdge> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun updateMindMapEdge(projectId: String, graphId: String, edgeId: String, patch: MindMapGraphEdgePatch): NativeResult<MindMapGraphEdge> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val patchJson = gson.toJson(patch)
            val json = updateMindMapEdgeJson(workspaceDir, projectId, graphId, edgeId, patchJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapGraphEdge>>() {}.type
            val response: RustResponse<MindMapGraphEdge> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteMindMapEdge(projectId: String, graphId: String, edgeId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val json = deleteMindMapEdgeJson(workspaceDir, projectId, graphId, edgeId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createMindMapAnchor(projectId: String, graphId: String, anchor: MindMapAnchor): NativeResult<MindMapAnchor> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val anchorJson = gson.toJson(anchor)
            val json = createMindMapAnchorJson(workspaceDir, projectId, graphId, anchorJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapAnchor>>() {}.type
            val response: RustResponse<MindMapAnchor> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun bindMindMapAnchor(projectId: String, graphId: String, nodeId: String, anchorId: String, linkKind: String): NativeResult<MindMapLink> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val json = bindMindMapAnchorJson(workspaceDir, projectId, graphId, nodeId, anchorId, linkKind)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<MindMapLink>>() {}.type
            val response: RustResponse<MindMapLink> = gson.fromJson(json, type)
            return if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            return NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun saveMindMapLayout(projectId: String, graphId: String, layout: MindMapLayout): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        try {
            val layoutJson = gson.toJson(layout)
            val json = saveMindMapLayoutJson(workspaceDir, projectId, graphId, layoutJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty response")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            return if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
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


    // --- StarMap API ---
    private external fun listStarmapsJson(workspaceDir: String): String?
    private external fun createStarmapJson(workspaceDir: String, title: String, desc: String): String?
    private external fun getStarmapGraphJson(workspaceDir: String, starmapId: String): String?
    private external fun addStarmapNodeJson(workspaceDir: String, starmapId: String, nodeJson: String): String?
    private external fun saveStarmapLayoutJson(workspaceDir: String, starmapId: String, layoutJson: String): String?
    private external fun renameStarmapNative(workspaceDir: String, starmapId: String, newTitle: String): String?
    private external fun deleteStarmapNative(workspaceDir: String, starmapId: String): String?
    private external fun bindStarmapToProjectJson(workspaceDir: String, starmapId: String, projectId: String): String?
    private external fun unbindStarmapFromProjectJson(workspaceDir: String, starmapId: String): String?
    private external fun setMainStarmapForProjectJson(workspaceDir: String, starmapId: String, projectId: String): String?
    private external fun getMainStarmapForProjectJson(workspaceDir: String, projectId: String): String?
    private external fun createChildStarmapJson(workspaceDir: String, parentId: String, title: String, desc: String): String?
    private external fun updateStarmapNodeJson(workspaceDir: String, starmapId: String, nodeId: String, patchJson: String): String?
    private external fun deleteStarmapNodeJson(workspaceDir: String, starmapId: String, nodeId: String): String?
    private external fun addStarmapEdgeJson(workspaceDir: String, starmapId: String, edgeJson: String): String?
    private external fun updateStarmapEdgeJson(workspaceDir: String, starmapId: String, edgeId: String, patchJson: String): String?
    private external fun deleteStarmapEdgeJson(workspaceDir: String, starmapId: String, edgeId: String): String?
    private external fun saveStarmapGraphJson(workspaceDir: String, starmapId: String, graphJson: String): String?

    // --- Writing Stats: extended ---
    private external fun getWritingStatsByProjectNative(workspaceDir: String, startDate: String, endDate: String): String?
    private external fun getWritingStatsByChapterNative(workspaceDir: String, startDate: String, endDate: String): String?
    private external fun getWritingStatsByDeviceNative(workspaceDir: String, startDate: String, endDate: String): String?
    private external fun getWritingSpeedCurveNative(workspaceDir: String, startDate: String, endDate: String, bucketMinutes: Int): String?

    fun listStarmaps(): NativeResult<List<StarMapMeta>> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = listStarmapsJson(workspaceDir)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<List<StarMapMeta>>>() {}.type
            val response: RustResponse<List<StarMapMeta>> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createStarmap(title: String, desc: String): NativeResult<StarMapMeta> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = createStarmapJson(workspaceDir, title, desc)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapMeta>>() {}.type
            val response: RustResponse<StarMapMeta> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getStarmapGraph(starmapId: String): NativeResult<StarMapData> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = getStarmapGraphJson(workspaceDir, starmapId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapData>>() {}.type
            val response: RustResponse<StarMapData> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun addStarmapNode(starmapId: String, node: StarMapGraphNode): NativeResult<StarMapGraphNode> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val nodeJson = gson.toJson(node)
            val json = addStarmapNodeJson(workspaceDir, starmapId, nodeJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapGraphNode>>() {}.type
            val response: RustResponse<StarMapGraphNode> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val layoutJson = gson.toJson(layout)
            val json = saveStarmapLayoutJson(workspaceDir, starmapId, layoutJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            if (response.success) {
                NativeResult.Success(true)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun renameStarmap(starmapId: String, newTitle: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = renameStarmapNative(workspaceDir, starmapId, newTitle)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteStarmap(starmapId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = deleteStarmapNative(workspaceDir, starmapId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun bindStarmapToProject(starmapId: String, projectId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = bindStarmapToProjectJson(workspaceDir, starmapId, projectId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun unbindStarmapFromProject(starmapId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = unbindStarmapFromProjectJson(workspaceDir, starmapId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun setMainStarmapForProject(starmapId: String, projectId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = setMainStarmapForProjectJson(workspaceDir, starmapId, projectId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getMainStarmapForProject(projectId: String): NativeResult<StarMapMeta?> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = getMainStarmapForProjectJson(workspaceDir, projectId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapMeta>>() {}.type
            val response: RustResponse<StarMapMeta> = gson.fromJson(json, type)
            if (response.success) NativeResult.Success(response.data) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun createChildStarmap(parentId: String, title: String, desc: String): NativeResult<StarMapMeta> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = createChildStarmapJson(workspaceDir, parentId, title, desc)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapMeta>>() {}.type
            val response: RustResponse<StarMapMeta> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun updateStarmapNode(starmapId: String, nodeId: String, patchJson: String): NativeResult<StarMapGraphNode> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = updateStarmapNodeJson(workspaceDir, starmapId, nodeId, patchJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapGraphNode>>() {}.type
            val response: RustResponse<StarMapGraphNode> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteStarmapNode(starmapId: String, nodeId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = deleteStarmapNodeJson(workspaceDir, starmapId, nodeId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun addStarmapEdge(starmapId: String, edgeJson: String): NativeResult<StarMapGraphEdge> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = addStarmapEdgeJson(workspaceDir, starmapId, edgeJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapGraphEdge>>() {}.type
            val response: RustResponse<StarMapGraphEdge> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun updateStarmapEdge(starmapId: String, edgeId: String, patchJson: String): NativeResult<StarMapGraphEdge> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = updateStarmapEdgeJson(workspaceDir, starmapId, edgeId, patchJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<StarMapGraphEdge>>() {}.type
            val response: RustResponse<StarMapGraphEdge> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun deleteStarmapEdge(starmapId: String, edgeId: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = deleteStarmapEdgeJson(workspaceDir, starmapId, edgeId)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun saveStarmapGraph(starmapId: String, graphJson: String): NativeResult<Boolean> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = saveStarmapGraphJson(workspaceDir, starmapId, graphJson)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val response = gson.fromJson(json, RustResponse::class.java)
            if (response.success) NativeResult.Success(true) else NativeResult.Error(response.error ?: "Unknown error")
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    // --- Writing Stats: extended ---

    fun getWritingStatsByProject(startDate: String, endDate: String): NativeResult<ProjectStatsSummary> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = getWritingStatsByProjectNative(workspaceDir, startDate, endDate)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<ProjectStatsSummary>>() {}.type
            val response: RustResponse<ProjectStatsSummary> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getWritingStatsByChapter(startDate: String, endDate: String): NativeResult<Any> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = getWritingStatsByChapterNative(workspaceDir, startDate, endDate)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getWritingStatsByDevice(startDate: String, endDate: String): NativeResult<Any> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = getWritingStatsByDeviceNative(workspaceDir, startDate, endDate)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    fun getWritingSpeedCurve(startDate: String, endDate: String, bucketMinutes: Int): NativeResult<Any> {
        if (!isLoaded) return NativeResult.NotLoaded
        return try {
            val json = getWritingSpeedCurveNative(workspaceDir, startDate, endDate, bucketMinutes)
            if (json.isNullOrEmpty()) return NativeResult.Error("Empty or null response from native bridge")
            val type = object : TypeToken<RustResponse<Any>>() {}.type
            val response: RustResponse<Any> = gson.fromJson(json, type)
            if (response.success && response.data != null) {
                NativeResult.Success(response.data)
            } else {
                NativeResult.Error(response.error ?: "Unknown error")
            }
        } catch (e: Throwable) {
            if (e is UnsatisfiedLinkError) return NativeResult.NotLoaded
            NativeResult.Error(e.message ?: "Exception occurred")
        }
    }

    private external fun calculateWordCountNative(text: String): Int

    private external fun processWritingEventNative(
        workspaceDir: String,
        deviceId: String,
        platform: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        oldText: String,
        newText: String,
        sessionId: String
    ): Boolean

    fun calculateWordCount(text: String): Int {
        if (!isLoaded) return 0
        return try {
            calculateWordCountNative(text)
        } catch (e: Throwable) {
            0
        }
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
        if (!isLoaded) return false
        return try {
            processWritingEventNative(workspaceDir, deviceId, platform, projectId, volumeId, chapterId, oldText, newText, sessionId)
        } catch (e: Throwable) {
            false
        }
    }

}
