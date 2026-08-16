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

/**
 * #630 评论 #1：单个同步目标（App / Project）的执行结果。
 *
 * Core 的全量同步把 App 和每个 Project 当作独立 target，按 remote_prefix 路由到
 * 同一远端仓库的不同目录。Android 不再自己循环调用两套 API，直接消费 Core 返回的
 * 聚合结果。
 *
 * - [targetKind]：`"app"` 或 `"project"`，与 Core `SyncTarget.scope` 对应。
 * - [projectId]：仅 Project target 携带；App target 为 null。
 * - [remotePrefix]：远端路径前缀（`app` 或 `projects/<project_id>`）。
 * - [result]：该 target 的单目标同步结果。
 */
data class TargetSyncResult(
    val targetKind: String,
    val projectId: String?,
    val remotePrefix: String,
    val result: SyncResult,
)

/**
 * #630 评论 #1：单个同步目标的试运行计划。
 */
data class TargetSyncPlan(
    val targetKind: String,
    val projectId: String?,
    val remotePrefix: String,
    val plan: SyncPlan,
)

/**
 * #630 评论 #1：全量同步聚合结果。
 *
 * 一次 `performFullSync` 同时同步 App target（设置/星图/主题）和所有 Project target，
 * 每个目标独立返回 [TargetSyncResult]，[overallStatus] 是 Core 聚合后的总体状态。
 */
data class FullSyncResult(
    val overallStatus: SyncStatus,
    val targets: List<TargetSyncResult>,
    val totalUploaded: Int,
    val totalDownloaded: Int,
    val totalLocalDeletes: Int,
    val totalRemoteDeletes: Int,
    val totalOverwritten: Int,
    val totalIgnored: Int,
    val totalConflicts: Int,
    val error: String?,
    val errorCategory: String?,
    val messageKey: String?,
)

/**
 * #630 评论 #1：全量同步试运行聚合计划。
 */
data class FullSyncDryRunResult(
    val targets: List<TargetSyncPlan>,
    val totalToUpload: Int,
    val totalToDownload: Int,
    val totalToDeleteLocal: Int,
    val totalToDeleteRemote: Int,
    val totalIgnored: Int,
    val totalConflicts: Int,
)

/**
 * #630 评论 #1：全量同步连接诊断结果。
 *
 * 诊断只测一次仓库、分支、token，不为每个 target 重复打一轮 GitHub 网络请求。
 */
data class FullSyncDiagnosticsResult(
    val diagnostics: SyncDiagnosticsResult,
)

/**
 * #630 评论第 4 点 / D：旧→新同步 profile 一次性迁移结果（Android 侧 model）。
 *
 * 与 Core `LegacyMigrationOutcomeDto` 对齐，用 [outcomeKind] 字符串区分变体：
 * - `"not_needed"`：新全局已存在，无需迁移
 * - `"migrated"`：迁移成功，[config] / [secrets] 字段填充
 * - `"needs_reconfigure"`：多项目冲突，[reason] 字段填充
 * - `"no_legacy_config"`：没找到任何旧配置
 *
 * 失败/冲突时 Core 不删旧凭据；Android 侧据此决定是否继续提交。
 */
data class LegacyMigrationOutcome(
    val outcomeKind: String,
    val config: SyncConfig? = null,
    val secrets: SyncSecrets? = null,
    val reason: String? = null,
)
