package com.xiwei.writerapp.model

//! # 核心数据模型（Android UI 层 - Model 层）
//!
//! 定义所有 Kotlin 侧的数据类，与 Rust Core 的数据结构一一对应。
//!
//! ## 架构定位
//!
//! 这些模型是 Rust Core UniFFI DTO 或 legacy JSON 响应的 Kotlin 映射，**不是业务实体**。
//! 业务实体的定义和操作都在 Rust Core 中。
//!
//! ## 设计原则
//!
//! - 所有字段名使用 `@SerializedName` 映射 Rust 的 snake_case
//! - 这些类只做数据承载，不包含业务逻辑
//! - 修改 Rust Core 数据结构时，必须同步更新这里的模型

import com.google.gson.annotations.SerializedName
import com.google.gson.JsonDeserializationContext
import com.google.gson.JsonDeserializer
import com.google.gson.JsonElement
import java.lang.reflect.Type

data class WorkspaceManifest(
    val version: Int
)

data class LocalSettings(
    val themeMode: String? = "system",
    val locale: String? = null,
    val editorFontSize: Float = 16f,
    val editorLineSpacingMultiplier: Float = 1.5f,
    val autoSaveEnabled: Boolean = true,
    val autoSaveDelayMs: Long = 1500L,
    val autoIndentEnabled: Boolean = true,
    val autoIndentWidth: Float = 2.0f,
    val windowWidth: Double = 800.0,
    val windowHeight: Double = 600.0,
    val editorTypingAnimationEnabled: Boolean = true,
    val editorSmoothCursorEnabled: Boolean = true,
    val editorTypingAnimationDurationMs: Int = 100,
    val editorSmoothCursorDurationMs: Int = 80,
    val aiEnabled: Boolean = false,
    val statsDeviceId: String? = null,
    val linuxSidebarWidth: Double = 240.0,
    val linuxEditorWidth: Double = 0.0
)

data class SyncableSettings(
    @SerializedName("fontSize") val fontSize: Double = 0.0,
    @SerializedName("themeMode") val themeMode: String = "",
    @SerializedName("monetColor") val monetColor: String = ""
)

data class Project(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String
)

data class ProjectStats(
    @SerializedName("total_word_count") val totalWordCount: Int,
    @SerializedName("volume_count") val volumeCount: Int,
    @SerializedName("chapter_count") val chapterCount: Int
)

data class Volume(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String,
    val order: Int = 0
)

data class ChapterMeta(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String,
    val order: Int = 0,
    @SerializedName("word_count") val wordCount: Int,
    val hash: String,
    val note: String? = null
)

enum class BridgeErrorCode(val wireName: String) {
    IoError("IO_ERROR"),
    JsonError("JSON_ERROR"),
    InvalidWorkspace("INVALID_WORKSPACE"),
    ProjectNotFound("PROJECT_NOT_FOUND"),
    VolumeNotFound("VOLUME_NOT_FOUND"),
    ChapterNotFound("CHAPTER_NOT_FOUND"),
    EmptyOverwriteBlocked("EMPTY_OVERWRITE_BLOCKED"),
    NotImplemented("NOT_IMPLEMENTED"),
    RefuseDeleteWorkspaceRoot("REFUSE_DELETE_WORKSPACE_ROOT"),
    InvalidDeleteTarget("INVALID_DELETE_TARGET"),
    Other("OTHER"),
    Unknown("UNKNOWN");

    companion object {
        fun fromWire(value: String?): BridgeErrorCode {
            return values().firstOrNull { it.wireName == value } ?: Unknown
        }
    }
}

data class BridgeError(
    val code: BridgeErrorCode = BridgeErrorCode.Unknown,
    val message: String
)

data class ChapterOpenResult(
    val meta: ChapterMeta,
    val content: String
)

typealias ChapterContent = ChapterOpenResult

data class ChapterSaveReceipt(
    @SerializedName("chapter_relative_path") val chapterRelativePath: String,
    @SerializedName("content_len") val contentLen: Long,
    @SerializedName("content_hash") val contentHash: String,
    @SerializedName("meta_hash") val metaHash: String,
    @SerializedName("updated_at") val updatedAt: String,
    @SerializedName("word_count") val wordCount: Int
)

data class RecentEdit(
    @SerializedName("project_id") val projectId: String,
    @SerializedName("volume_id") val volumeId: String,
    @SerializedName("chapter_id") val chapterId: String,
    val timestamp: String
)

// Sync Models

enum class SyncTransport {
    @SerializedName("https_token") HttpsToken,
    @SerializedName("ssh_deploy_key") SshKey
}

enum class BackendType {
    @SerializedName("git") Git,
    @SerializedName("github_api") GithubApi,
    @SerializedName("webdav") WebDav,
    @SerializedName("s3") S3,
    @SerializedName("local_folder") LocalFolder
}

data class SyncConfig(
    val enabled: Boolean? = false,
    @SerializedName("backend_type") val backendType: BackendType? = BackendType.GithubApi,
    @SerializedName("remote_url") val remoteUrl: String? = "",
    val transport: SyncTransport? = SyncTransport.HttpsToken,
    val branch: String? = "main",
    @SerializedName("auto_sync") val autoSync: Boolean? = false,
    @SerializedName("sync_interval_seconds") val syncIntervalSeconds: Int? = 300,
    @SerializedName("proxy_enabled") val proxyEnabled: Boolean? = false,
    @SerializedName("proxy_type") val proxyType: String? = "auto",
    @SerializedName("proxy_host") val proxyHost: String? = "127.0.0.1",
    @SerializedName("proxy_port") val proxyPort: Int? = 7890,
    val username: String? = "",
    @SerializedName("android_has_internet_permission") val androidHasInternetPermission: Boolean? = null,
    @SerializedName("android_has_access_network_state_permission") val androidHasAccessNetworkStatePermission: Boolean? = null
) {
    fun normalize(): SyncConfig {
        return copy(
            enabled = enabled ?: false,
            backendType = backendType ?: BackendType.GithubApi,
            remoteUrl = remoteUrl ?: "",
            transport = transport ?: SyncTransport.HttpsToken,
            branch = if (branch.isNullOrEmpty()) "main" else branch,
            autoSync = autoSync ?: false,
            syncIntervalSeconds = if (syncIntervalSeconds == null || syncIntervalSeconds <= 0) 300 else syncIntervalSeconds,
            proxyEnabled = proxyEnabled ?: false,
            proxyType = if (proxyType.isNullOrBlank()) "auto" else proxyType,
            proxyHost = if (proxyHost.isNullOrBlank()) "127.0.0.1" else proxyHost,
            proxyPort = if (proxyPort == null || proxyPort <= 0) 7890 else proxyPort,
            username = username ?: "",
            androidHasInternetPermission = androidHasInternetPermission ?: true,
            androidHasAccessNetworkStatePermission = androidHasAccessNetworkStatePermission ?: true
        )
    }
}

data class SyncSecrets(
    val token: String? = null,
    @SerializedName("ssh_private_key") val sshPrivateKey: String? = null
)

data class Tombstone(
    @SerializedName("original_path") val originalPath: String,
    @SerializedName("trash_path") val trashPath: String,
    @SerializedName("deleted_at") val deletedAt: Long,
    @SerializedName("purge_after") val purgeAfter: Long,
    @SerializedName("deleted_by") val deletedBy: String,
    @SerializedName("original_hash") val originalHash: String,
    val kind: String
)

data class SyncState(
    val status: SyncStatus = SyncStatus.Idle,
    @SerializedName("remote_url") val remoteUrl: String? = null,
    @SerializedName("backend_type") val backendType: String? = null,
    val transport: String? = null,
    @SerializedName("last_synced_commit") val lastSyncedCommit: String? = null,
    @SerializedName("last_sync_time") val lastSyncTime: Long? = null,
    @SerializedName("last_error") val lastError: String? = null,
    @SerializedName("last_successful_network_mode") val lastSuccessfulNetworkMode: String? = null,
    @SerializedName("known_files") val knownFiles: Map<String, String>? = emptyMap(),
    val conflicts: List<SyncConflict>? = emptyList(),
    val tombstones: List<Tombstone>? = emptyList(),
    @SerializedName("deleted_files") val deletedFiles: Set<String>? = emptySet()
)

enum class SyncStatus {
    @SerializedName("idle") Idle,
    @SerializedName("syncing") Syncing,
    @SerializedName("success") Success,
    @SerializedName("configured_untested") ConfiguredUntested,
    @SerializedName("conflict") Conflict,
    @SerializedName("recoverable_error") RecoverableError,
    @SerializedName("fatal_error") FatalError,
    @SerializedName("dirty_repo_blocked") DirtyRepoBlocked,
    @SerializedName("branch_missing_recovered") BranchMissingRecovered,
    @SerializedName("error") Error,
    @SerializedName("no_changes") NoChanges,
    @SerializedName("latest_wins_applied") LatestWinsApplied
}

class SyncStatusDeserializer : JsonDeserializer<SyncStatus> {
    override fun deserialize(
        json: JsonElement,
        typeOfT: Type,
        context: JsonDeserializationContext
    ): SyncStatus {
        if (json.isJsonPrimitive) {
            val str = json.asString
            return when (str) {
                "idle" -> SyncStatus.Idle
                "syncing" -> SyncStatus.Syncing
                "success" -> SyncStatus.Success
                "configured_untested" -> SyncStatus.ConfiguredUntested
                "conflict" -> SyncStatus.Conflict
                "dirty_repo_blocked" -> SyncStatus.DirtyRepoBlocked
                "branch_missing_recovered" -> SyncStatus.BranchMissingRecovered
                "no_changes" -> SyncStatus.NoChanges
                "latest_wins_applied" -> SyncStatus.LatestWinsApplied
                else -> SyncStatus.Error
            }
        }
        if (json.isJsonObject) {
            val obj = json.asJsonObject
            if (obj.has("error") || obj.has("Error")) {
                return SyncStatus.Error
            }
            if (obj.has("recoverable_error") || obj.has("RecoverableError")) {
                return SyncStatus.RecoverableError
            }
            if (obj.has("fatal_error") || obj.has("FatalError")) {
                return SyncStatus.FatalError
            }
        }
        return SyncStatus.Error
    }
}

enum class FirstSyncMode {
    @SerializedName("not_attempted") NotAttempted,
    @SerializedName("clone_into_empty_workspace") CloneIntoEmptyWorkspace,
    @SerializedName("init_existing_workspace") InitExistingWorkspace,
    @SerializedName("already_git_repo") AlreadyGitRepo,
    @SerializedName("blocked_non_empty_remote") BlockedNonEmptyRemote,
    @SerializedName("unrelated_histories") UnrelatedHistories,
    @SerializedName("none") None
}

data class SyncConflict(
    @SerializedName("local_path") val localPath: String,
    @SerializedName("remote_path") val remotePath: String,
    @SerializedName("local_hash") val localHash: String,
    @SerializedName("remote_hash") val remoteHash: String,
    @SerializedName("base_hash") val baseHash: String,
    @SerializedName("created_at") val createdAt: Long,
    val description: String
)

data class NetworkProbeResult(
    val mode: String,
    val success: Boolean,
    val status: String,
    val message: String,
    @SerializedName("raw_error") val rawError: String? = null
)

data class SyncResult(
    val status: SyncStatus,
    @SerializedName("uploaded_files") val uploadedFiles: List<String> = emptyList(),
    @SerializedName("downloaded_files") val downloadedFiles: List<String> = emptyList(),
    @SerializedName("local_deletes") val localDeletes: List<String> = emptyList(),
    @SerializedName("remote_deletes") val remoteDeletes: List<String> = emptyList(),
    @SerializedName("overwritten_files") val overwrittenFiles: List<String> = emptyList(),
    @SerializedName("ignored_files") val ignoredFiles: List<String> = emptyList(),
    val conflicts: List<SyncConflict> = emptyList(),
    @SerializedName("commit_hash") val commitHash: String? = null,
    val error: String? = null,
    @SerializedName("error_category") val errorCategory: String? = null,
    @SerializedName("first_sync_mode") val firstSyncMode: FirstSyncMode = FirstSyncMode.None,
    @SerializedName("user_message") val userMessage: String? = null,
    @SerializedName("chosen_network_mode") val chosenNetworkMode: String? = null,
    @SerializedName("network_probe_summary") val networkProbeSummary: List<NetworkProbeResult>? = emptyList(),
    @SerializedName("conflict_summary") val conflictSummary: SyncConflictSummary? = null,
    @SerializedName("settings_conflicts") val settingsConflicts: List<SettingConflictDetail>? = emptyList()
)

data class SettingConflictDetail(
    val key: String,
    @SerializedName("local_value") val localValue: JsonElement,
    @SerializedName("remote_value") val remoteValue: JsonElement
)

data class SyncConflictSummary(
    val status: String,
    @SerializedName("local_dirty") val localDirty: Boolean,
    @SerializedName("remote_changed") val remoteChanged: Boolean,
    @SerializedName("conflicted_files") val conflictedFiles: List<String>,
    @SerializedName("blocked_reason") val blockedReason: String,
    @SerializedName("safe_next_steps") val safeNextSteps: List<String>
)


data class SyncDiagnosticsResult(
    val success: Boolean,
    @SerializedName("backend_type") val backendType: String,
    @SerializedName("android_has_internet_permission") val androidHasInternetPermission: Boolean,
    @SerializedName("android_has_access_network_state_permission") val androidHasAccessNetworkStatePermission: Boolean,
    @SerializedName("android_network_state") val androidNetworkState: String,
    @SerializedName("tcp_probe_ok") val tcpProbeOk: Boolean,
    @SerializedName("tcp_probe_status") val tcpProbeStatus: String,
    @SerializedName("http_connect_probe_ok") val httpConnectProbeOk: Boolean,
    @SerializedName("http_connect_probe_status") val httpConnectProbeStatus: String,
    @SerializedName("libgit2_probe_ok") val libgit2ProbeOk: Boolean,
    @SerializedName("libgit2_probe_status") val libgit2ProbeStatus: String,
    @SerializedName("network_ok") val networkOk: Boolean,
    @SerializedName("auth_ok") val authOk: Boolean,
    @SerializedName("repo_ok") val repoOk: Boolean,
    @SerializedName("branch_ok") val branchOk: Boolean,
    @SerializedName("network_status") val networkStatus: String,
    @SerializedName("auth_status") val authStatus: String,
    @SerializedName("repo_status") val repoStatus: String,
    @SerializedName("branch_status") val branchStatus: String,
    @SerializedName("remote_url_sanitized") val remoteUrlSanitized: String,
    val transport: String,
    @SerializedName("error_category") val errorCategory: String,
    @SerializedName("user_message") val userMessage: String,
    @SerializedName("raw_error") val rawError: String?,
    @SerializedName("chosen_network_mode") val chosenNetworkMode: String? = null,
    @SerializedName("network_probe_summary") val networkProbeSummary: List<NetworkProbeResult>? = emptyList()
)

data class SyncPlan(
    @SerializedName("files_to_upload") val filesToUpload: List<String> = emptyList(),
    @SerializedName("files_to_download") val filesToDownload: List<String> = emptyList(),
    @SerializedName("files_to_delete_local") val filesToDeleteLocal: List<String> = emptyList(),
    @SerializedName("files_to_delete_remote") val filesToDeleteRemote: List<String> = emptyList(),
    @SerializedName("ignored_files") val ignoredFiles: List<String> = emptyList(),
    @SerializedName("conflicts") val conflicts: List<String> = emptyList()
)

data class ActionDescriptor(
    val id: String,
    val title: String,
    val description: String,
    val category: String,
    val kind: String,
    val riskLevel: String,
    val confirmRequired: Boolean,
    val undoable: Boolean,
    val platforms: List<String>,
    val inputSchema: JsonElement?,
    val uiSchema: JsonElement?
)

data class ActionResult(
    val success: Boolean,
    val message: String?,
    val data: JsonElement?,
    val proposedUi: JsonElement?,
    val requiresConfirmation: Boolean?
)

data class UiSchemaDescriptor(
    val type: String?,
    val min: Double?,
    val max: Double?,
    val step: Double?
) {
    companion object {
        fun fromJson(element: JsonElement?): UiSchemaDescriptor? {
            if (element == null || !element.isJsonObject) return null
            val obj = element.asJsonObject
            return UiSchemaDescriptor(
                type = obj.get("type")?.asString,
                min = obj.get("min")?.asDouble,
                max = obj.get("max")?.asDouble,
                step = obj.get("step")?.asDouble
            )
        }
    }
}

data class InputSchemaProperty(
    val name: String,
    val type: String,
    val minimum: Double?,
    val maximum: Double?
) {
    companion object {
        fun fromJson(element: JsonElement?): List<InputSchemaProperty> {
            if (element == null || !element.isJsonObject) return emptyList()
            val obj = element.asJsonObject
            val props = obj.getAsJsonObject("properties") ?: return emptyList()
            val required = obj.getAsJsonArray("required")?.map { it.asString } ?: emptyList()
            return props.entrySet().map { (key, value) ->
                val propObj = value.asJsonObject
                InputSchemaProperty(
                    name = key,
                    type = propObj.get("type")?.asString ?: "string",
                    minimum = propObj.get("minimum")?.asDouble,
                    maximum = propObj.get("maximum")?.asDouble
                )
            }
        }
    }
}

data class WritingStatsSummary(
    val range: WritingStatsRange? = null,
    val totalWordCount: Long = 0,
    val totalTimeSeconds: Long = 0,
    val activeDays: Int = 0,
    @SerializedName("total_human_typed_chars") val totalHumanTypedChars: Long? = null,
    @SerializedName("total_active_seconds") val totalActiveSeconds: Long? = null,
    @SerializedName("total_sessions") val totalSessions: Int? = null,
    @SerializedName("days_count") val daysCount: Int? = null
)

typealias WritingWritingStatsSummary = WritingStatsSummary

data class WritingStatsRange(
    @SerializedName("start_date") val startDate: String? = null,
    @SerializedName("end_date") val endDate: String? = null
)

data class ProjectWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val projects: List<ProjectWritingStatsItem>? = emptyList()
)

data class ProjectWritingStatsItem(
    @SerializedName("project_id") val projectId: String? = null,
    val projectTitle: String? = null,
    @SerializedName("human_typed_chars") val humanTypedChars: Long? = null,
    @SerializedName("pasted_chars") val pastedChars: Long? = null,
    @SerializedName("deleted_chars") val deletedChars: Long? = null,
    @SerializedName("ai_inserted_chars") val aiInsertedChars: Long? = null,
    @SerializedName("net_delta_chars") val netDeltaChars: Long? = null,
    @SerializedName("active_seconds") val activeSeconds: Long? = null
)

data class ChapterWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val chapters: List<ChapterWritingStatsItem>? = emptyList()
)

data class ChapterWritingStatsItem(
    @SerializedName("chapter_id") val chapterId: String? = null,
    @SerializedName("human_typed_chars") val humanTypedChars: Long? = null,
    @SerializedName("pasted_chars") val pastedChars: Long? = null,
    @SerializedName("deleted_chars") val deletedChars: Long? = null,
    @SerializedName("ai_inserted_chars") val aiInsertedChars: Long? = null,
    @SerializedName("net_delta_chars") val netDeltaChars: Long? = null,
    @SerializedName("active_seconds") val activeSeconds: Long? = null
)

data class DeviceWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val devices: List<DeviceWritingStatsItem>? = emptyList()
)

data class DeviceWritingStatsItem(
    @SerializedName("device_id") val deviceId: String? = null,
    val platform: String? = null,
    @SerializedName("human_typed_chars") val humanTypedChars: Long? = null,
    @SerializedName("pasted_chars") val pastedChars: Long? = null,
    @SerializedName("deleted_chars") val deletedChars: Long? = null,
    @SerializedName("ai_inserted_chars") val aiInsertedChars: Long? = null,
    @SerializedName("net_delta_chars") val netDeltaChars: Long? = null,
    @SerializedName("active_seconds") val activeSeconds: Long? = null,
    @SerializedName("sessions_count") val sessionsCount: Int? = null
)

data class WritingSpeedCurve(
    val range: WritingStatsRange? = null,
    @SerializedName("bucket_minutes") val bucketMinutes: Int = 0,
    val buckets: List<WritingSpeedBucket>? = emptyList()
)

data class WritingSpeedBucket(
    @SerializedName("start_ms") val startMs: Long = 0,
    @SerializedName("end_ms") val endMs: Long = 0,
    @SerializedName("chars_typed") val charsTyped: Long = 0,
    @SerializedName("chars_per_minute") val charsPerMinute: Double = 0.0
)

data class ProjectStatsSummary(
    val projectId: String,
    val wordCount: Long
)
