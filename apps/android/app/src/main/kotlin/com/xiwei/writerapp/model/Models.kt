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

data class SyncConfig(
    val enabled: Boolean? = false,
    @SerializedName("remote_url") val remoteUrl: String? = "",
    val transport: SyncTransport? = SyncTransport.HttpsToken,
    val branch: String? = "main",
    @SerializedName("auto_sync") val autoSync: Boolean? = false,
    @SerializedName("sync_interval_seconds") val syncIntervalSeconds: Int? = 300,
    @SerializedName("proxy_enabled") val proxyEnabled: Boolean? = false,
    @SerializedName("proxy_type") val proxyType: String? = "http",
    @SerializedName("proxy_host") val proxyHost: String? = "127.0.0.1",
    @SerializedName("proxy_port") val proxyPort: Int? = 7890
) {
    fun normalize(): SyncConfig {
        return copy(
            enabled = enabled ?: false,
            remoteUrl = remoteUrl ?: "",
            transport = transport ?: SyncTransport.HttpsToken,
            branch = if (branch.isNullOrEmpty()) "main" else branch,
            autoSync = autoSync ?: false,
            syncIntervalSeconds = if (syncIntervalSeconds == null || syncIntervalSeconds <= 0) 300 else syncIntervalSeconds,
            proxyEnabled = proxyEnabled ?: false,
            proxyType = if (proxyType.isNullOrEmpty()) "http" else proxyType,
            proxyHost = if (proxyHost.isNullOrEmpty()) "127.0.0.1" else proxyHost,
            proxyPort = if (proxyPort == null || proxyPort <= 0) {
                if (proxyType == "socks5") 7891 else 7890
            } else proxyPort
        )
    }
}

data class SyncSecrets(
    val token: String? = null,
    @SerializedName("ssh_private_key") val sshPrivateKey: String? = null
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

data class SyncResult(
    val status: SyncStatus,
    @SerializedName("uploaded_files") val uploadedFiles: List<String> = emptyList(),
    @SerializedName("downloaded_files") val downloadedFiles: List<String> = emptyList(),
    @SerializedName("ignored_files") val ignoredFiles: List<String> = emptyList(),
    val conflicts: List<SyncConflict> = emptyList(),
    @SerializedName("commit_hash") val commitHash: String? = null,
    val error: String? = null,
    @SerializedName("first_sync_mode") val firstSyncMode: FirstSyncMode = FirstSyncMode.None,
    @SerializedName("user_message") val userMessage: String? = null
)

data class SyncPlan(
    @SerializedName("files_to_upload") val filesToUpload: List<String> = emptyList(),
    @SerializedName("files_to_download") val filesToDownload: List<String> = emptyList(),
    @SerializedName("files_to_delete_local") val filesToDeleteLocal: List<String> = emptyList(),
    @SerializedName("files_to_delete_remote") val filesToDeleteRemote: List<String> = emptyList(),
    @SerializedName("ignored_files") val ignoredFiles: List<String> = emptyList()
)
