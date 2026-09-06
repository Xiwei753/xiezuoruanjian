package com.xiwei.sujian.feature.sync.data.model

enum class SyncTransport {
    HttpsToken,
    SshKey,
}

data class SyncConfig(
    val enabled: Boolean? = false,
    val activeProvider: String? = "github",
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
            activeProvider = activeProvider ?: "github",
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
    val providerType: String,
    val hasNetworkPermission: Boolean,
    val hasNetworkStatePermission: Boolean,
    val networkState: String,
    val networkOk: Boolean,
    val authOk: Boolean,
    val remoteOk: Boolean,
    val networkStatus: String,
    val authStatus: String,
    val errorCategory: String,
    val rawError: String?,
    val providerDetails: String?,
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
    val error: String? = null,
    val errorCategory: String? = null,
    val messageKey: String? = null,
    val searchIndexRebuildError: String? = null,
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
 * #6300 评论 5307423953 Part B：全量同步持久状态 — 一次全量同步事务的总体结果。
 *
 * 与 per-target 的 [SyncState] 分层：per-target state 记录每个 target 自己的
 * manifest/LWW 状态；[FullSyncState] 只记录"这一次全量事务整体是什么结果"。
 *
 * - [overallStatus]：总体状态（Success/NoChanges/LatestWinsApplied/BranchMissingRecovered
 *   视为整体成功；Error/PartialConflict/RecoverableError 等为失败）。
 * - [lastAttemptTime]：上次全量同步尝试时间（Unix 秒），每次尝试都更新。
 * - [lastSuccessTime]：上次全量同步整体成功时间（Unix 秒），仅整体成功类才更新；
 *   部分失败保留旧值。
 * - [failedTargets]：本次尝试中失败的 target 标识（"app" 或 "project:<id>"）。
 */
data class FullSyncState(
    val overallStatus: SyncStatus,
    val lastAttemptTime: Long?,
    val lastSuccessTime: Long?,
    val failedTargets: List<String>,
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

/**
 * #630 评论第 5 点 Part C-Android：旧 profile 精确 generation metadata（Android 侧 model）。
 *
 * 与 Core [uniffi.writer_core.LegacyProfileMetadataDto] 对齐。调用方（平台层 DataStore）
 * 通过此结构精确告诉 Core 应该读取哪个 `sync_token_<base>_g<N>` key，避免 Core 猜测枚举上限。
 *
 * - [source] = "app"：旧应用级 profile；[projectId] 应为 null
 * - [source] = "project:<id>"：旧作品级 profile；[projectId] 应为 Some(id)
 * - [activeGeneration] = Some(n)：精确读取 `sync_token_<base>_g{n}`
 * - [activeGeneration] = null：旧 DataStore 无 committed generation，回退 base key / 文件
 *
 * [activeGeneration] 用 Long 表达 DataStore 侧的 Long 值，映射到 Core u32 时检查范围。
 */
data class LegacyProfileMetadata(
    val source: String,
    val projectId: String? = null,
    val activeGeneration: Long? = null,
)
