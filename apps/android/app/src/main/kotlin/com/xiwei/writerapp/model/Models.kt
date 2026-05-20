package com.xiwei.writerapp.model

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
    val editorSmoothCursorDurationMs: Int = 80
)

data class SyncableSettings(
    @SerializedName("fontSize") val fontSize: Double = 0.0,
    @SerializedName("themeMode") val themeMode: String = ""
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
    @SerializedName("updated_at") val updatedAt: String
)

data class ChapterMeta(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String,
    @SerializedName("word_count") val wordCount: Int,
    val hash: String,
    val note: String? = null
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
    @SerializedName("proxy_enabled") val proxyEnabled: Boolean? = true,
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
            backendType = backendType ?: BackendType.Git,
            remoteUrl = remoteUrl ?: "",
            transport = transport ?: SyncTransport.HttpsToken,
            branch = if (branch.isNullOrEmpty()) "main" else branch,
            autoSync = autoSync ?: false,
            syncIntervalSeconds = if (syncIntervalSeconds == null || syncIntervalSeconds <= 0) 300 else syncIntervalSeconds,
            proxyEnabled = proxyEnabled ?: true,
            proxyType = if (proxyType.isNullOrEmpty()) "auto" else proxyType,
            proxyHost = if (proxyHost.isNullOrEmpty()) "127.0.0.1" else proxyHost,
            proxyPort = if (proxyPort == null || proxyPort <= 0) {
                if (proxyType == "socks5") 7891 else 7890
            } else proxyPort,
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

data class SyncState(
    @SerializedName("remote_url") val remoteUrl: String? = null,
    val transport: String? = null,
    @SerializedName("last_synced_commit") val lastSyncedCommit: String? = null,
    @SerializedName("last_sync_time") val lastSyncTime: Long? = null,
    @SerializedName("last_error") val lastError: String? = null,
    @SerializedName("last_successful_network_mode") val lastSuccessfulNetworkMode: String? = null
)

enum class SyncStatus {
    @SerializedName("idle") Idle,
    @SerializedName("syncing") Syncing,
    @SerializedName("success") Success,
    @SerializedName("conflict") Conflict,
    @SerializedName("error") Error
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
                "conflict" -> SyncStatus.Conflict
                else -> SyncStatus.Error
            }
        }
        if (json.isJsonObject) {
            val obj = json.asJsonObject
            if (obj.has("error")) {
                return SyncStatus.Error
            }
            if (obj.has("Error")) {
                return SyncStatus.Error
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
    val path: String,
    val description: String? = null
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
    @SerializedName("ignored_files") val ignoredFiles: List<String> = emptyList(),
    val conflicts: List<SyncConflict> = emptyList(),
    @SerializedName("commit_hash") val commitHash: String? = null,
    val error: String? = null,
    @SerializedName("first_sync_mode") val firstSyncMode: FirstSyncMode = FirstSyncMode.None,
    @SerializedName("user_message") val userMessage: String? = null,
    @SerializedName("chosen_network_mode") val chosenNetworkMode: String? = null,
    @SerializedName("network_probe_summary") val networkProbeSummary: List<NetworkProbeResult>? = emptyList()
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
    @SerializedName("proxy_used") val proxyUsed: Boolean,
    @SerializedName("proxy_type") val proxyType: String,
    @SerializedName("proxy_host") val proxyHost: String,
    @SerializedName("proxy_port") val proxyPort: Int,
    @SerializedName("remote_url_sanitized") val remoteUrlSanitized: String,
    val transport: String,
    @SerializedName("app_proxy_status") val appProxyStatus: String,
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
    @SerializedName("ignored_files") val ignoredFiles: List<String> = emptyList()
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
