package com.xiwei.sujian.feature.settings.ui

// ! # 同步结果映射（从 SettingsSyncOps 拆分）— 降低 TooManyFunctions

import com.xiwei.sujian.feature.sync.data.ExclusiveResult
import com.xiwei.sujian.feature.sync.data.SyncDiagnosticsOutcome
import com.xiwei.sujian.feature.sync.data.SyncDryRunOutcome
import com.xiwei.sujian.feature.sync.data.SyncFailureKind
import com.xiwei.sujian.feature.sync.data.SyncOutcome

// #597 同步事务协议字符串 — 提取为常量避免 StringLiteralDuplication
internal const val SYNC_STATUS_ERROR = "error"

/** 把 runExclusive 的忙/成功结果统一成调用方可直接消费的 [SyncCommandIoResult]。 */

internal fun ExclusiveResult<SyncCommandIoResult>.toIoResult(): SyncCommandIoResult =
    when (this) {
        is ExclusiveResult.Busy ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = "busy", messageKey = "sync_already_running"),
            )
        is ExclusiveResult.Success -> value
    }

/** 应用级 performAppSyncDryRun 的 UI 层类型化封装。 */
internal fun com.xiwei.sujian.feature.sync.data.SyncDryRunOutcome.toAppIoResult(): SyncCommandIoResult =
    when (this) {
        is com.xiwei.sujian.feature.sync.data.SyncDryRunOutcome.Success -> {
            val plan = plan
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = "ok",
                    messageKey = "sync_dry_run_result",
                    counts =
                        SyncCounts(
                            uploaded = plan.filesToUpload.size,
                            downloaded = plan.filesToDownload.size,
                            deletedRemote = plan.filesToDeleteRemote.size,
                            deletedLocal = plan.filesToDeleteLocal.size,
                            conflicts = plan.conflicts.size,
                            ignored = plan.ignoredFiles.size,
                        ),
                ),
            )
        }
        is com.xiwei.sujian.feature.sync.data.SyncDryRunOutcome.Error ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = syncFailureKind.messageKey(),
                    sanitizedDiagnostic = message,
                ),
            )
        com.xiwei.sujian.feature.sync.data.SyncDryRunOutcome.NotLoaded ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = SyncFailureKind.NativeUnavailable.messageKey(),
                    sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name,
                ),
            )
    }

/** 应用级 performAppSyncDiagnostics 成功分支的 UI 层封装 — 提取为独立 helper 降低认知复杂度。 */
internal fun SyncDiagnosticsOutcome.Success.appDiagnosticsSuccessToAppIoResult(): SyncCommandIoResult {
    val diag = result
    return SyncCommandIoResult(
        true,
        true,
        StructuredSyncResult(
            statusCode = if (diag.success) "ok" else "fail",
            messageKey = "sync_test_connection_result",
            messageArgs =
                mapOf(
                    "network" to if (diag.networkOk) "ok" else "fail",
                    "auth" to if (diag.authOk) "ok" else "fail",
                    "repo" to if (diag.repoOk) "ok" else "fail",
                    "branch" to if (diag.branchOk) "ok" else "fail",
                ),
            sanitizedDiagnostic = if (!diag.success) "connection_failed" else null,
        ),
    )
}

/** 应用级 performAppSyncDiagnostics 的 UI 层类型化封装。 */
internal fun com.xiwei.sujian.feature.sync.data.SyncDiagnosticsOutcome.toAppIoResult(): SyncCommandIoResult =
    when (this) {
        is com.xiwei.sujian.feature.sync.data.SyncDiagnosticsOutcome.Success -> appDiagnosticsSuccessToAppIoResult()
        is com.xiwei.sujian.feature.sync.data.SyncDiagnosticsOutcome.Error ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = syncFailureKind.messageKey(),
                    sanitizedDiagnostic = message,
                ),
            )
        com.xiwei.sujian.feature.sync.data.SyncDiagnosticsOutcome.NotLoaded ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = SyncFailureKind.NativeUnavailable.messageKey(),
                    sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name,
                ),
            )
    }

// ── #597：结果映射 — 各同步结果类型 → SyncCommandIoResult（isSuccess 由 statusCode 决定）──

internal fun SyncOutcome.Completed.completedToIoResult(): SyncCommandIoResult {
    val sr = result
    return SyncCommandIoResult(
        true,
        true,
        StructuredSyncResult(
            statusCode = if (sr.error == null) "ok" else SYNC_STATUS_ERROR,
            messageKey = "sync_perform_result",
            counts =
                SyncCounts(
                    uploaded = sr.uploadedFiles.size,
                    downloaded = sr.downloadedFiles.size,
                    deletedRemote = sr.remoteDeletes.size,
                    deletedLocal = sr.localDeletes.size,
                    conflicts = sr.conflicts.size,
                    overwritten = sr.overwrittenFiles.size,
                    ignored = sr.ignoredFiles.size,
                ),
            sanitizedDiagnostic = if (sr.error != null) "sync_failed" else null,
        ),
    )
}

internal fun SyncOutcome.toIoResult(): SyncCommandIoResult =
    when (this) {
        is SyncOutcome.Completed -> completedToIoResult()
        is SyncOutcome.Unconfigured ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_unconfigured"),
            )
        is SyncOutcome.Disabled ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_disabled"),
            )
        is SyncOutcome.Busy ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_busy"),
            )
        is SyncOutcome.RetryableFailure ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = kind.messageKey()),
            )
        is SyncOutcome.TerminalFailure ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = kind.messageKey()),
            )
        else ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(statusCode = SYNC_STATUS_ERROR, messageKey = "sync_unknown"),
            )
    }

internal fun SyncDryRunOutcome.toIoResult(): SyncCommandIoResult =
    when (this) {
        is SyncDryRunOutcome.Success -> {
            val plan = plan
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = "ok",
                    messageKey = "sync_dry_run_result",
                    counts =
                        SyncCounts(
                            uploaded = plan.filesToUpload.size,
                            downloaded = plan.filesToDownload.size,
                            deletedRemote = plan.filesToDeleteRemote.size,
                            deletedLocal = plan.filesToDeleteLocal.size,
                            conflicts = plan.conflicts.size,
                            ignored = plan.ignoredFiles.size,
                        ),
                ),
            )
        }
        is SyncDryRunOutcome.Error ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = syncFailureKind.messageKey(),
                    sanitizedDiagnostic = message,
                ),
            )
        SyncDryRunOutcome.NotLoaded ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = SyncFailureKind.NativeUnavailable.messageKey(),
                    sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name,
                ),
            )
    }

internal fun SyncDiagnosticsOutcome.Success.diagnosticsSuccessToIoResult(): SyncCommandIoResult {
    val diag = result
    return SyncCommandIoResult(
        true,
        true,
        StructuredSyncResult(
            statusCode = if (diag.success) "ok" else "fail",
            messageKey = "sync_test_connection_result",
            messageArgs =
                mapOf(
                    "network" to if (diag.networkOk) "ok" else "fail",
                    "auth" to if (diag.authOk) "ok" else "fail",
                    "repo" to if (diag.repoOk) "ok" else "fail",
                    "branch" to if (diag.branchOk) "ok" else "fail",
                ),
            sanitizedDiagnostic = if (!diag.success) "connection_failed" else null,
        ),
    )
}

internal fun SyncDiagnosticsOutcome.toIoResult(): SyncCommandIoResult =
    when (this) {
        is SyncDiagnosticsOutcome.Success -> diagnosticsSuccessToIoResult()
        is SyncDiagnosticsOutcome.Error ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = syncFailureKind.messageKey(),
                    sanitizedDiagnostic = message,
                ),
            )
        SyncDiagnosticsOutcome.NotLoaded ->
            SyncCommandIoResult(
                true,
                true,
                StructuredSyncResult(
                    statusCode = SYNC_STATUS_ERROR,
                    messageKey = SyncFailureKind.NativeUnavailable.messageKey(),
                    sanitizedDiagnostic = SyncFailureKind.NativeUnavailable.name,
                ),
            )
    }
