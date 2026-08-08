package com.xiwei.sujian.feature.sync.data.model

enum class SyncTransport {
    HttpsToken,
    SshKey,
}

enum class BackendType {
    Git,
    GithubApi,
}

data class SyncConfig(
    val enabled: Boolean? = false,
    val backendType: BackendType? = BackendType.GithubApi,
    val remoteUrl: String? = "",
    val transport: SyncTransport? = SyncTransport.HttpsToken,
    val branch: String? = "main",
    val autoSync: Boolean? = false,
    val syncIntervalSeconds: Int? = 300,
    val username: String? = "",
    val hasNetworkPermission: Boolean? = null,
    val hasNetworkStatePermission: Boolean? = null,
) {
    fun normalize(): SyncConfig {
        return copy(
            enabled = enabled ?: false,
            backendType = backendType ?: BackendType.GithubApi,
            remoteUrl = remoteUrl ?: "",
            transport = transport ?: SyncTransport.HttpsToken,
            branch = if (branch.isNullOrEmpty()) "main" else branch,
            autoSync = autoSync ?: false,
            syncIntervalSeconds =
                if (syncIntervalSeconds == null || syncIntervalSeconds <= 0) 300 else syncIntervalSeconds,
            username = username ?: "",
            hasNetworkPermission = hasNetworkPermission ?: true,
            hasNetworkStatePermission = hasNetworkStatePermission ?: true,
        )
    }
}

data class SyncSecrets(
    val token: String? = null,
    val sshPrivateKey: String? = null,
)

data class Tombstone(
    val originalPath: String,
    val trashPath: String,
    val deletedAt: Long,
    val purgeAfter: Long,
    val deletedBy: String,
    val originalHash: String,
    val kind: String,
)

data class SyncState(
    val status: SyncStatus = SyncStatus.Idle,
    val remoteUrl: String? = null,
    val backendType: String? = null,
    val transport: String? = null,
    val lastSyncedCommit: String? = null,
    val lastSyncTime: Long? = null,
    val lastError: String? = null,
    val knownFiles: Map<String, String>? = emptyMap(),
    val conflicts: List<SyncConflict>? = emptyList(),
    val tombstones: List<Tombstone>? = emptyList(),
    val deletedFiles: Set<String>? = emptySet(),
)

data class SyncCapabilityData(
    val canRun: Boolean = false,
    val blockReasonCode: String? = null,
    val blockMessageKey: String? = null,
    val messageArgs: Map<String, String> = emptyMap(),
)

enum class SyncStatus {
    Idle,
    Syncing,
    Success,
    ConfiguredNotTested,
    Conflict,
    PartialConflict,
    RecoverableError,
    FatalError,
    DirtyRepoBlocked,
    BranchMissingRecovered,
    Error,
    NoChanges,
    LatestWinsApplied,
}

enum class FirstSyncMode {
    NotAttempted,
    CloneIntoEmptyProject,
    InitExistingProject,
    AlreadyGitRepo,
    BlockedNonEmptyRemote,
    UnrelatedHistories,
    None,
}

data class SyncConflictSummary(
    val status: String,
    val localDirty: Boolean,
    val remoteChanged: Boolean,
    val conflictedFiles: List<String> = emptyList(),
    val blockedReason: String,
    val safeNextSteps: List<String> = emptyList(),
)

data class SyncConflict(
    val localPath: String,
    val remotePath: String,
    val localHash: String,
    val remoteHash: String,
    val baseHash: String,
    val createdAt: Long,
    val description: String,
)

data class SyncDiagnosticsResult(
    val success: Boolean,
    val backendType: String,
    val hasNetworkPermission: Boolean,
    val hasNetworkStatePermission: Boolean,
    val networkState: String,
    val networkOk: Boolean,
    val authOk: Boolean,
    val repoOk: Boolean,
    val branchOk: Boolean,
    val networkStatus: String,
    val authStatus: String,
    val repoStatus: String,
    val branchStatus: String,
    val remoteUrlSanitized: String,
    val transport: String,
    val errorCategory: String,
    val rawError: String?,
)

data class SyncResult(
    val status: SyncStatus,
    val uploadedFiles: List<String> = emptyList(),
    val downloadedFiles: List<String> = emptyList(),
    val localDeletes: List<String> = emptyList(),
    val remoteDeletes: List<String> = emptyList(),
    val overwrittenFiles: List<String> = emptyList(),
    val ignoredFiles: List<String> = emptyList(),
    val conflicts: List<SyncConflict> = emptyList(),
    val conflictSummary: SyncConflictSummary? = null,
    val commitHash: String? = null,
    val error: String? = null,
    val errorCategory: String? = null,
    val firstSyncMode: FirstSyncMode = FirstSyncMode.None,
)

data class SyncPlan(
    val filesToUpload: List<String> = emptyList(),
    val filesToDownload: List<String> = emptyList(),
    val filesToDeleteLocal: List<String> = emptyList(),
    val filesToDeleteRemote: List<String> = emptyList(),
    val ignoredFiles: List<String> = emptyList(),
    val conflicts: List<String> = emptyList(),
)
