package com.xiwei.sujian.core.interop.common
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.settings.data.model.SyncableSettings
import com.xiwei.sujian.feature.stats.data.model.ChapterWritingStatsItem
import com.xiwei.sujian.feature.stats.data.model.ChapterWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.DeviceWritingStatsItem
import com.xiwei.sujian.feature.stats.data.model.DeviceWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsItem
import com.xiwei.sujian.feature.stats.data.model.ProjectWritingStatsSummary
import com.xiwei.sujian.feature.stats.data.model.WritingSpeedBucket
import com.xiwei.sujian.feature.stats.data.model.WritingSpeedCurve
import com.xiwei.sujian.feature.stats.data.model.WritingStatsRange
import com.xiwei.sujian.feature.stats.data.model.WritingStatsSummary
import com.xiwei.sujian.feature.sync.data.SyncFailureKind
import com.xiwei.sujian.feature.sync.data.model.FullSyncDiagnosticsResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncDryRunResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncState
import com.xiwei.sujian.feature.sync.data.model.LegacyMigrationOutcome
import com.xiwei.sujian.feature.sync.data.model.LegacyProfileMetadata
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncConflict
import com.xiwei.sujian.feature.sync.data.model.SyncDiagnosticsResult
import com.xiwei.sujian.feature.sync.data.model.SyncPlan
import com.xiwei.sujian.feature.sync.data.model.SyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncState
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import com.xiwei.sujian.feature.sync.data.model.SyncTransport
import com.xiwei.sujian.feature.sync.data.model.TargetSyncPlan
import com.xiwei.sujian.feature.sync.data.model.TargetSyncResult
import uniffi.writer_core.ChapterStatsRecordDto
import uniffi.writer_core.ChapterStatsSummaryDto
import uniffi.writer_core.DateRangeDto
import uniffi.writer_core.DeviceStatsRecordDto
import uniffi.writer_core.DeviceStatsSummaryDto
import uniffi.writer_core.FullSyncDiagnosticsResultDto
import uniffi.writer_core.FullSyncDryRunResultDto
import uniffi.writer_core.FullSyncResultDto
import uniffi.writer_core.FullSyncStateDto
import uniffi.writer_core.LegacyMigrationOutcomeDto
import uniffi.writer_core.LegacyProfileMetadataDto
import uniffi.writer_core.LocalSettingsDto
import uniffi.writer_core.ProjectStatsRecordDto
import uniffi.writer_core.ProjectStatsSummaryDto
import uniffi.writer_core.ProviderConfigDto
import uniffi.writer_core.ProviderSecretsDto
import uniffi.writer_core.SpeedCurvePointDto
import uniffi.writer_core.SpeedCurveSummaryDto
import uniffi.writer_core.SyncConfigDto
import uniffi.writer_core.SyncConflictDto
import uniffi.writer_core.SyncDiagnosticsResultDto
import uniffi.writer_core.SyncPlanDto
import uniffi.writer_core.SyncResultDto
import uniffi.writer_core.SyncSecretsDto
import uniffi.writer_core.SyncStateDto
import uniffi.writer_core.SyncableSettingsDto
import uniffi.writer_core.TargetSyncPlanDto
import uniffi.writer_core.TargetSyncResultDto
import uniffi.writer_core.WriterException
import uniffi.writer_core.WritingStatsSummaryDto

internal fun WriterException.toWireErrorCode(): String =
    when (this) {
        is WriterException.Io -> "IO_ERROR"
        is WriterException.Json -> "JSON_ERROR"
        is WriterException.ProjectNotFound -> "PROJECT_NOT_FOUND"
        is WriterException.VolumeNotFound -> "VOLUME_NOT_FOUND"
        is WriterException.ChapterNotFound -> "CHAPTER_NOT_FOUND"
        is WriterException.EmptyOverwriteBlocked -> "EMPTY_OVERWRITE_BLOCKED"
        is WriterException.NotImplemented -> "NOT_IMPLEMENTED"
        is WriterException.RefuseToDeleteRoot -> "REFUSE_DELETE_ROOT"
        is WriterException.InvalidDeleteTarget -> "INVALID_DELETE_TARGET"
        is WriterException.SyncConflict -> "SYNC_CONFLICT"
        is WriterException.SyncFailed -> "SYNC_FAILED"
        is WriterException.RetryableNetwork -> "RETRYABLE_NETWORK"
        is WriterException.RetryableIo -> "RETRYABLE_IO"
        is WriterException.Authentication -> "AUTHENTICATION"
        is WriterException.Conflict -> "CONFLICT"
        is WriterException.DirtyRepository -> "DIRTY_REPOSITORY"
        is WriterException.Protocol -> "PROTOCOL"
        is WriterException.Fatal -> "FATAL"
        is WriterException.Other -> "OTHER"
    }

/**
 * #592 七：Core/Bridge 边界的类型化失败 — 由 WriterException 变体直接推导，
 * 不维护字符串错误码表。未知错误默认 Fatal，只有明确网络或 IO 失败可重试。
 */
internal fun WriterException.toSyncFailureKind(): SyncFailureKind? =
    when (this) {
        is WriterException.RetryableNetwork -> SyncFailureKind.RetryableNetwork
        is WriterException.RetryableIo -> SyncFailureKind.RetryableIo
        is WriterException.Authentication -> SyncFailureKind.Authentication
        is WriterException.Conflict,
        is WriterException.SyncConflict,
        -> SyncFailureKind.Conflict
        is WriterException.DirtyRepository -> SyncFailureKind.DirtyRepository
        is WriterException.Protocol -> SyncFailureKind.Protocol
        is WriterException.Fatal -> SyncFailureKind.Fatal
        // 明确 I/O 失败可重试；其余（Io/Json/Other/旧 SyncFailed 等）视为未知 → null → Fatal。
        is WriterException.Io -> SyncFailureKind.RetryableIo
        else -> null
    }

internal fun LocalSettingsDto.toModel() =
    LocalSettings(
        themeMode = themeMode,
        appearanceMode = appearanceMode,
        colorSource = colorSource,
        dynamicColorEnabled = dynamicColorEnabled,
        selectedBuiltinThemeId = selectedBuiltinThemeId,
        selectedPaletteId = selectedPaletteId,
        locale = locale,
        autoSaveEnabled = autoSaveEnabled,
        editorFontSize = editorFontSize,
        editorLineSpacingMultiplier = editorLineSpacingMultiplier,
        windowWidth = windowWidth.toDouble(),
        windowHeight = windowHeight.toDouble(),
        autoSaveDelayMs = autoSaveDelayMs.toLong(),
        autoIndentEnabled = autoIndentEnabled,
        autoIndentWidth = autoIndentWidth,
        editorTypingAnimationEnabled = editorTypingAnimationEnabled,
        editorSmoothCursorEnabled = editorSmoothCursorEnabled,
        editorTypingAnimationDurationMs = editorTypingAnimationDurationMs.toInt(),
        editorSmoothCursorDurationMs = editorSmoothCursorDurationMs.toInt(),
        aiEnabled = aiEnabled,
        statsDeviceId = statsDeviceId,
        desktopSidebarWidth = desktopSidebarWidth,
        desktopEditorWidth = desktopEditorWidth,
        editorCoordinatedTextCursorAnimationEnabled = editorCoordinatedTextCursorAnimationEnabled,
        diagnosticsEnabled = diagnosticsEnabled,
        diagnosticsVerbose = diagnosticsVerbose,
        useSelfRenderEditorOnAndroid = true,
    )

internal fun LocalSettings.toDto() =
    LocalSettingsDto(
        themeMode = themeMode,
        appearanceMode = appearanceMode,
        colorSource = colorSource,
        dynamicColorEnabled = dynamicColorEnabled,
        selectedBuiltinThemeId = selectedBuiltinThemeId,
        selectedPaletteId = selectedPaletteId,
        locale = locale,
        autoSaveEnabled = autoSaveEnabled,
        editorFontSize = editorFontSize,
        editorLineSpacingMultiplier = editorLineSpacingMultiplier,
        windowWidth = windowWidth.toFloat(),
        windowHeight = windowHeight.toFloat(),
        autoSaveDelayMs = autoSaveDelayMs.toULong(),
        autoIndentEnabled = autoIndentEnabled,
        autoIndentWidth = autoIndentWidth,
        editorTypingAnimationEnabled = editorTypingAnimationEnabled,
        editorSmoothCursorEnabled = editorSmoothCursorEnabled,
        editorTypingAnimationDurationMs = editorTypingAnimationDurationMs.toULong(),
        editorSmoothCursorDurationMs = editorSmoothCursorDurationMs.toULong(),
        aiEnabled = aiEnabled,
        statsDeviceId = statsDeviceId,
        desktopSidebarWidth = desktopSidebarWidth,
        desktopEditorWidth = desktopEditorWidth,
        editorCoordinatedTextCursorAnimationEnabled = editorCoordinatedTextCursorAnimationEnabled,
        diagnosticsEnabled = diagnosticsEnabled,
        diagnosticsVerbose = diagnosticsVerbose,
    )

internal fun SyncableSettingsDto.toModel() = SyncableSettings(fontSize, themeMode, monetColor, themePaletteJson)

@Suppress("DEPRECATION")
internal fun SyncableSettings.toDto() = SyncableSettingsDto(fontSize, themeMode, monetColor, themePaletteJson)

internal fun SyncConfigDto.toModel(): SyncConfig {
    val gitHubConfig = providerConfig as? ProviderConfigDto.GitHub
    return SyncConfig(
        enabled = enabled,
        activeProvider = activeProvider,
        remoteUrl = gitHubConfig?.remoteUrl ?: "",
        transport = gitHubConfig?.transport.toSyncTransport(),
        branch = gitHubConfig?.branch ?: "main",
        autoSync = autoSync,
        syncIntervalSeconds = syncIntervalSeconds.toInt(),
        username = gitHubConfig?.username ?: "",
        hasNetworkStatePermission = hasNetworkStatePermission,
        hasNetworkPermission = hasNetworkPermission,
    )
}

internal fun SyncConfig.toDto(): SyncConfigDto {
    val normalized = normalize()
    return SyncConfigDto(
        enabled = normalized.enabled ?: false,
        activeProvider = normalized.activeProvider ?: "github",
        providerConfig =
            ProviderConfigDto.GitHub(
                remoteUrl = normalized.remoteUrl ?: "",
                branch = normalized.branch ?: "main",
                username = normalized.username ?: "",
                transport = normalized.transport.toWire(),
            ),
        autoSync = normalized.autoSync ?: false,
        syncIntervalSeconds = (normalized.syncIntervalSeconds ?: 300).toUInt(),
        hasNetworkPermission = normalized.hasNetworkPermission ?: false,
        hasNetworkStatePermission = normalized.hasNetworkStatePermission ?: false,
    )
}

internal fun SyncSecretsDto.toModel(): SyncSecrets =
    SyncSecrets((providerSecrets as? ProviderSecretsDto.GitHub)?.token)

internal fun SyncSecrets.toDto(): SyncSecretsDto =
    SyncSecretsDto(token?.let { ProviderSecretsDto.GitHub(it) })

internal fun SyncStatus.toWire(): String =
    when (this) {
        SyncStatus.Idle -> "idle"
        SyncStatus.Syncing -> "syncing"
        SyncStatus.Success -> "success"
        SyncStatus.ConfiguredNotTested -> "configured_not_tested"
        SyncStatus.Conflict -> "conflict"
        SyncStatus.PartialConflict -> "partial_conflict"
        SyncStatus.RecoverableError -> "recoverable_error"
        SyncStatus.FatalError -> "fatal_error"
        SyncStatus.DirtyRepoBlocked -> "dirty_repo_blocked"
        SyncStatus.BranchMissingRecovered -> "branch_missing_recovered"
        SyncStatus.NoChanges -> "no_changes"
        SyncStatus.LatestWinsApplied -> "latest_wins_applied"
        SyncStatus.Error -> "error"
    }

internal fun SyncConflict.toDto() =
    SyncConflictDto(
        localPath = localPath,
        remotePath = remotePath,
        localHash = localHash,
        remoteHash = remoteHash,
        baseHash = baseHash,
        createdAt = createdAt,
        description = description,
    )

internal fun SyncState.toDto() =
    SyncStateDto(
        status = status.toWire(),
        lastSyncTime = lastSyncTime,
        lastError = lastError,
        conflicts = conflicts?.map { it.toDto() },
    )

internal fun SyncStateDto.toModel() =
    SyncState(
        status = status.toSyncStatus(),
        lastSyncTime = lastSyncTime,
        lastError = lastError,
        conflicts = conflicts?.map { it.toModel() } ?: emptyList(),
    )

internal fun SyncConflictDto.toModel() =
    SyncConflict(
        localPath,
        remotePath,
        localHash,
        remoteHash,
        baseHash,
        createdAt,
        description,
    )

internal fun SyncDiagnosticsResultDto.toModel() =
    SyncDiagnosticsResult(
        success = success,
        providerType = providerType,
        hasNetworkPermission = hasNetworkPermission,
        hasNetworkStatePermission = hasNetworkStatePermission,
        networkState = networkState,
        networkOk = networkOk,
        authOk = authOk,
        remoteOk = remoteOk,
        networkStatus = networkStatus,
        authStatus = authStatus,
        errorCategory = errorCategory,
        rawError = rawError,
        providerDetails = providerDetails,
    )

internal fun SyncPlanDto.toModel() =
    SyncPlan(
        filesToUpload = filesToUpload,
        filesToDownload = filesToDownload,
        filesToDeleteLocal = filesToDeleteLocal,
        filesToDeleteRemote = filesToDeleteRemote,
        ignoredFiles = ignoredFiles,
        conflicts = conflicts,
    )

internal fun SyncResultDto.toModel() =
    SyncResult(
        status = status.toSyncStatus(),
        uploadedFiles = uploadedFiles,
        downloadedFiles = downloadedFiles,
        localDeletes = localDeletes,
        remoteDeletes = remoteDeletes,
        overwrittenFiles = overwrittenFiles,
        ignoredFiles = ignoredFiles,
        conflicts = conflicts.map { it.toModel() },
        error = error,
        errorCategory = errorCategory,
        messageKey = messageKey,
        searchIndexRebuildError = searchIndexRebuildError,
    )

internal fun TargetSyncResultDto.toModel() =
    TargetSyncResult(
        targetKind = targetKind,
        projectId = projectId,
        remotePrefix = remotePrefix,
        result = result.toModel(),
    )

internal fun TargetSyncPlanDto.toModel() =
    TargetSyncPlan(
        targetKind = targetKind,
        projectId = projectId,
        remotePrefix = remotePrefix,
        plan = plan.toModel(),
    )

internal fun FullSyncResultDto.toModel() =
    FullSyncResult(
        overallStatus = overallStatus.toSyncStatus(),
        targets = targets.map { it.toModel() },
        totalUploaded = totalUploaded.toInt(),
        totalDownloaded = totalDownloaded.toInt(),
        totalLocalDeletes = totalLocalDeletes.toInt(),
        totalRemoteDeletes = totalRemoteDeletes.toInt(),
        totalOverwritten = totalOverwritten.toInt(),
        totalIgnored = totalIgnored.toInt(),
        totalConflicts = totalConflicts.toInt(),
        error = error,
        errorCategory = errorCategory,
        messageKey = messageKey,
    )

internal fun FullSyncDryRunResultDto.toModel() =
    FullSyncDryRunResult(
        targets = targets.map { it.toModel() },
        totalToUpload = totalToUpload.toInt(),
        totalToDownload = totalToDownload.toInt(),
        totalToDeleteLocal = totalToDeleteLocal.toInt(),
        totalToDeleteRemote = totalToDeleteRemote.toInt(),
        totalIgnored = totalIgnored.toInt(),
        totalConflicts = totalConflicts.toInt(),
    )

internal fun FullSyncDiagnosticsResultDto.toModel() =
    FullSyncDiagnosticsResult(
        diagnostics = diagnostics.toModel(),
    )

/**
 * #630 评论 5307423953 Part B：Core [FullSyncStateDto] → Android [FullSyncState]。
 */
internal fun FullSyncStateDto.toModel() =
    FullSyncState(
        overallStatus = overallStatus.toSyncStatus(),
        lastAttemptTime = lastAttemptTime,
        lastSuccessTime = lastSuccessTime,
        failedTargets = failedTargets,
    )

/**
 * #630 评论第 4 点 / D：Core [LegacyMigrationOutcomeDto] → Android [LegacyMigrationOutcome]。
 *
 * outcome_kind 字符串原样透传，config/secrets 用现有 [SyncConfigDto.toModel] /
 * [SyncSecretsDto.toModel] 映射；reason 原样透传。
 */
internal fun LegacyMigrationOutcomeDto.toModel() =
    LegacyMigrationOutcome(
        outcomeKind = outcomeKind,
        config = config?.toModel(),
        secrets = secrets?.toModel(),
        reason = reason,
    )

/**
 * #630 评论第 5 点 Part C-Android：[LegacyProfileMetadata] → Core [LegacyProfileMetadataDto]。
 *
 * activeGeneration 用 Long 表达 DataStore 侧的 Long 值，映射到 Core u32 时检查范围：
 * 超出 [UInt.MAX_VALUE] 的 generation 视为无效，传 null 让 Core 回退 base key / 文件
 * （实际 generation 不会超过 UInt.MAX_VALUE，这是防御性处理）。
 */
internal fun LegacyProfileMetadata.toDto(): LegacyProfileMetadataDto =
    LegacyProfileMetadataDto(
        source = source,
        projectId = projectId,
        activeGeneration = activeGeneration?.takeIf { it in 1L..UInt.MAX_VALUE.toLong() }?.toUInt(),
    )

internal fun String?.toSyncTransport(): SyncTransport =
    when (this) {
        "ssh", "ssh_deploy_key" -> SyncTransport.SshKey
        else -> SyncTransport.HttpsToken
    }

internal fun SyncTransport?.toWire(): String =
    when (this ?: SyncTransport.HttpsToken) {
        SyncTransport.HttpsToken -> "https_token"
        SyncTransport.SshKey -> "ssh_deploy_key"
    }

internal fun String?.toSyncStatus(): SyncStatus =
    when (this) {
        "idle" -> SyncStatus.Idle
        "syncing" -> SyncStatus.Syncing
        "success" -> SyncStatus.Success
        "configured_not_tested" -> SyncStatus.ConfiguredNotTested
        "conflict" -> SyncStatus.Conflict
        "partial_conflict" -> SyncStatus.PartialConflict
        "recoverable_error" -> SyncStatus.RecoverableError
        "fatal_error" -> SyncStatus.FatalError
        "dirty_repo_blocked" -> SyncStatus.DirtyRepoBlocked
        "branch_missing_recovered" -> SyncStatus.BranchMissingRecovered
        "no_changes" -> SyncStatus.NoChanges
        "latest_wins_applied" -> SyncStatus.LatestWinsApplied
        else -> SyncStatus.Error
    }

internal fun DateRangeDto.toModel() =
    WritingStatsRange(
        startDate = startDate,
        endDate = endDate,
    )

internal fun WritingStatsSummaryDto.toModel() =
    WritingStatsSummary(
        range = range.toModel(),
        totalHumanTypedChars = totalHumanTypedChars.toLong(),
        totalActiveSeconds = totalActiveSeconds.toLong(),
        totalSessions = totalSessions.toInt(),
        daysCount = daysCount.toInt(),
    )

internal fun ProjectStatsRecordDto.toModel() =
    ProjectWritingStatsItem(
        projectId = projectId,
        humanTypedChars = humanTypedChars.toLong(),
        pastedChars = pastedChars.toLong(),
        deletedChars = deletedChars.toLong(),
        aiInsertedChars = aiInsertedChars.toLong(),
        netDeltaChars = netDeltaChars,
        activeSeconds = activeSeconds.toLong(),
    )

internal fun ProjectStatsSummaryDto.toModel() =
    ProjectWritingStatsSummary(
        range = range.toModel(),
        projects = projects.map { it.toModel() },
    )

internal fun ChapterStatsRecordDto.toModel() =
    ChapterWritingStatsItem(
        chapterId = chapterId,
        humanTypedChars = humanTypedChars.toLong(),
        pastedChars = pastedChars.toLong(),
        deletedChars = deletedChars.toLong(),
        aiInsertedChars = aiInsertedChars.toLong(),
        netDeltaChars = netDeltaChars,
        activeSeconds = activeSeconds.toLong(),
    )

internal fun ChapterStatsSummaryDto.toModel() =
    ChapterWritingStatsSummary(
        range = range.toModel(),
        chapters = chapters.map { it.toModel() },
    )

internal fun DeviceStatsRecordDto.toModel() =
    DeviceWritingStatsItem(
        deviceId = deviceId,
        platform = platform.name,
        deviceClass = deviceClass,
        humanTypedChars = humanTypedChars.toLong(),
        pastedChars = pastedChars.toLong(),
        deletedChars = deletedChars.toLong(),
        aiInsertedChars = aiInsertedChars.toLong(),
        netDeltaChars = netDeltaChars,
        activeSeconds = activeSeconds.toLong(),
        sessionsCount = sessionsCount.toInt(),
    )

internal fun DeviceStatsSummaryDto.toModel() =
    DeviceWritingStatsSummary(
        range = range.toModel(),
        devices = devices.map { it.toModel() },
    )

internal fun SpeedCurvePointDto.toModel() =
    WritingSpeedBucket(
        startMs = startMs,
        endMs = endMs,
        charsTyped = charsTyped.toLong(),
        charsPerMinute = charsPerMinute.toDouble(),
    )

internal fun SpeedCurveSummaryDto.toModel() =
    WritingSpeedCurve(
        range = range.toModel(),
        bucketMinutes = bucketMinutes.toInt(),
        buckets = buckets.map { it.toModel() },
    )
