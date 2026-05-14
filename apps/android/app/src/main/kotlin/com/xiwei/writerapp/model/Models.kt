package com.xiwei.writerapp.model

import com.xiwei.writerapp.model.*
import com.xiwei.writerapp.data.*
import com.xiwei.writerapp.ui.*


import com.google.gson.annotations.SerializedName

data class WorkspaceManifest(
    val version: Int
)

data class LocalSettings(
    val themeMode: String? = "system",
    val locale: String? = null,
    val editorFontSize: Float = 16f,
    val editorLineSpacingMultiplier: Float = 1.5f,
    val autoSaveEnabled: Boolean = true,
    val autoSaveDelayMs: Long = 1500L
)

data class Project(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String
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
    val hash: String
)

// Sync Models

enum class SyncTransport {
    @SerializedName("HttpsToken") HttpsToken,
    @SerializedName("SshKey") SshKey
}

data class SyncConfig(
    val enabled: Boolean = false,
    @SerializedName("remote_url") val remoteUrl: String = "",
    val transport: SyncTransport = SyncTransport.HttpsToken,
    val branch: String = "main",
    @SerializedName("auto_sync") val autoSync: Boolean = false,
    @SerializedName("sync_interval_seconds") val syncIntervalSeconds: Int = 300
)

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

enum class FirstSyncMode {
    @SerializedName("CloneIntoEmptyWorkspace") CloneIntoEmptyWorkspace,
    @SerializedName("InitExistingWorkspace") InitExistingWorkspace,
    @SerializedName("UnrelatedHistories") UnrelatedHistories,
    @SerializedName("BlockedNonEmptyRemote") BlockedNonEmptyRemote,
    @SerializedName("None") None
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
