package com.xiwei.sujian.data

import com.xiwei.sujian.model.BackendType
import com.xiwei.sujian.model.ChapterMeta
import com.xiwei.sujian.model.ChapterOpenResult
import com.xiwei.sujian.model.ChapterSaveReceipt
import com.xiwei.sujian.model.ChapterWritingStatsItem
import com.xiwei.sujian.model.ChapterWritingStatsSummary
import com.xiwei.sujian.model.DeviceWritingStatsItem
import com.xiwei.sujian.model.DeviceWritingStatsSummary
import com.xiwei.sujian.model.FirstSyncMode
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.NetworkProbeResult
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.ProjectStats
import com.xiwei.sujian.model.ProjectWritingStatsItem
import com.xiwei.sujian.model.ProjectWritingStatsSummary
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncConflict
import com.xiwei.sujian.model.SyncDiagnosticsResult
import com.xiwei.sujian.model.SyncPlan
import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncSecrets
import com.xiwei.sujian.model.SyncState
import com.xiwei.sujian.model.SyncStatus
import com.xiwei.sujian.model.SyncTransport
import com.xiwei.sujian.model.SyncableSettings
import com.xiwei.sujian.model.Volume
import com.xiwei.sujian.model.WritingSpeedBucket
import com.xiwei.sujian.model.WritingSpeedCurve
import com.xiwei.sujian.model.WritingStatsRange
import com.xiwei.sujian.model.WritingStatsSummary
import uniffi.writer_core.ChapterContentDto
import uniffi.writer_core.ChapterMetaDto
import uniffi.writer_core.ChapterSaveReceiptDto
import uniffi.writer_core.ChapterStatsRecordDto
import uniffi.writer_core.ChapterStatsSummaryDto
import uniffi.writer_core.DateRangeDto
import uniffi.writer_core.DeviceStatsRecordDto
import uniffi.writer_core.DeviceStatsSummaryDto
import uniffi.writer_core.LocalSettingsDto
import uniffi.writer_core.NetworkProbeResultDto
import uniffi.writer_core.ProjectDto
import uniffi.writer_core.ProjectStatsDto
import uniffi.writer_core.ProjectStatsRecordDto
import uniffi.writer_core.ProjectStatsSummaryDto
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
import uniffi.writer_core.VolumeDto
import uniffi.writer_core.WriterException
import uniffi.writer_core.WritingStatsSummaryDto

internal fun WriterException.toWireErrorCode(): String = when (this) {
    is WriterException.Io -> "IO_ERROR"
    is WriterException.Json -> "JSON_ERROR"
    is WriterException.InvalidWorkspace -> "INVALID_WORKSPACE"
    is WriterException.ProjectNotFound -> "PROJECT_NOT_FOUND"
    is WriterException.VolumeNotFound -> "VOLUME_NOT_FOUND"
    is WriterException.ChapterNotFound -> "CHAPTER_NOT_FOUND"
    is WriterException.EmptyOverwriteBlocked -> "EMPTY_OVERWRITE_BLOCKED"
    is WriterException.NotImplemented -> "NOT_IMPLEMENTED"
    is WriterException.RefuseToDeleteWorkspaceRoot -> "REFUSE_DELETE_WORKSPACE_ROOT"
    is WriterException.InvalidDeleteTarget -> "INVALID_DELETE_TARGET"
    is WriterException.SyncConflict -> "SYNC_CONFLICT"
    is WriterException.SyncFailed -> "SYNC_FAILED"
    is WriterException.Other -> "OTHER"
}

internal fun ProjectDto.toModel() = Project(id, title, createdAt, updatedAt)

internal fun ProjectStatsDto.toModel() = ProjectStats(
    totalWordCount = totalWordCount.toInt(),
    volumeCount = volumeCount.toInt(),
    chapterCount = chapterCount.toInt()
)

internal fun VolumeDto.toModel() = Volume(
    id = id,
    title = title,
    createdAt = createdAt,
    updatedAt = updatedAt,
    order = order
)

internal fun ChapterMetaDto.toModel() = ChapterMeta(
    id = id,
    title = title,
    createdAt = createdAt,
    updatedAt = updatedAt,
    order = order,
    wordCount = wordCount.toInt(),
    hash = hash,
    note = note
)

internal fun ChapterContentDto.toModel() = ChapterOpenResult(meta.toModel(), content)

internal fun ChapterSaveReceiptDto.toModel() = ChapterSaveReceipt(
    chapterRelativePath = chapterRelativePath,
    contentLen = contentLen.toLong(),
    contentHash = contentHash,
    metaHash = metaHash,
    updatedAt = updatedAt,
    wordCount = wordCount.toInt()
)

internal fun LocalSettingsDto.toModel() = LocalSettings(
    themeMode = themeMode,
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
    linuxSidebarWidth = desktopSidebarWidth,
    linuxEditorWidth = desktopEditorWidth
)

internal fun LocalSettings.toDto() = LocalSettingsDto(
    themeMode = themeMode,
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
    desktopSidebarWidth = linuxSidebarWidth,
    desktopEditorWidth = linuxEditorWidth
)

internal fun SyncableSettingsDto.toModel() = SyncableSettings(fontSize, themeMode, monetColor)
internal fun SyncableSettings.toDto() = SyncableSettingsDto(fontSize, themeMode, monetColor)

internal fun SyncConfigDto.toModel() = SyncConfig(
    enabled = enabled,
    backendType = backendType.toBackendType(),
    remoteUrl = remoteUrl,
    transport = transport.toSyncTransport(),
    branch = branch,
    autoSync = autoSync,
    syncIntervalSeconds = syncIntervalSeconds.toInt(),
    username = username,
    androidHasAccessNetworkStatePermission = null,
    androidHasInternetPermission = null
)

internal fun SyncConfig.toDto(): SyncConfigDto {
    val normalized = normalize()
    return SyncConfigDto(
        enabled = normalized.enabled ?: false,
        backendType = normalized.backendType.toWire(),
        remoteUrl = normalized.remoteUrl ?: "",
        transport = normalized.transport.toWire(),
        branch = normalized.branch ?: "main",
        autoSync = normalized.autoSync ?: false,
        syncIntervalSeconds = (normalized.syncIntervalSeconds ?: 300).toUInt(),
        username = normalized.username ?: "",
        androidHasInternetPermission = normalized.androidHasInternetPermission ?: false,
        androidHasAccessNetworkStatePermission = normalized.androidHasAccessNetworkStatePermission ?: false
    )
}

internal fun SyncSecretsDto.toModel() = SyncSecrets(token, null)
internal fun SyncSecrets.toDto() = SyncSecretsDto(token)

internal fun SyncStateDto.toModel() = SyncState(
    status = status.toSyncStatus(),
    remoteUrl = remoteUrl,
    backendType = backendType,
    transport = transport,
    lastSyncedCommit = lastSyncedCommit,
    lastSyncTime = lastSyncTime,
    lastError = lastError,
    lastSuccessfulNetworkMode = lastSuccessfulNetworkMode,
    conflicts = conflicts?.map { it.toModel() } ?: emptyList()
)

internal fun SyncConflictDto.toModel() = SyncConflict(localPath, remotePath, localHash, remoteHash, baseHash, createdAt, description)
internal fun NetworkProbeResultDto.toModel() = NetworkProbeResult(mode, success, status, message, rawError)

internal fun SyncDiagnosticsResultDto.toModel() = SyncDiagnosticsResult(
    success = success,
    backendType = backendType,
    androidHasInternetPermission = androidHasInternetPermission,
    androidHasAccessNetworkStatePermission = androidHasAccessNetworkStatePermission,
    androidNetworkState = androidNetworkState,
    tcpProbeOk = tcpProbeOk,
    tcpProbeStatus = tcpProbeStatus,
    httpConnectProbeOk = httpConnectProbeOk,
    httpConnectProbeStatus = httpConnectProbeStatus,
    libgit2ProbeOk = libgit2ProbeOk,
    libgit2ProbeStatus = libgit2ProbeStatus,
    networkOk = networkOk,
    authOk = authOk,
    repoOk = repoOk,
    branchOk = branchOk,
    networkStatus = networkStatus,
    authStatus = authStatus,
    repoStatus = repoStatus,
    branchStatus = branchStatus,
    remoteUrlSanitized = remoteUrlSanitized,
    transport = transport,
    errorCategory = errorCategory,
    userMessage = userMessage,
    rawError = rawError,
    chosenNetworkMode = chosenNetworkMode,
    networkProbeSummary = networkProbeSummary?.map { it.toModel() } ?: emptyList()
)

internal fun SyncPlanDto.toModel() = SyncPlan(
    filesToUpload = filesToUpload,
    filesToDownload = filesToDownload,
    filesToDeleteLocal = filesToDeleteLocal,
    filesToDeleteRemote = filesToDeleteRemote,
    ignoredFiles = ignoredFiles,
    conflicts = conflicts
)

internal fun SyncResultDto.toModel() = SyncResult(
    status = status.toSyncStatus(),
    uploadedFiles = uploadedFiles,
    downloadedFiles = downloadedFiles,
    localDeletes = localDeletes,
    remoteDeletes = remoteDeletes,
    overwrittenFiles = overwrittenFiles,
    ignoredFiles = ignoredFiles,
    conflicts = conflicts.map { it.toModel() },
    commitHash = commitHash,
    error = error,
    errorCategory = errorCategory,
    firstSyncMode = firstSyncMode.toFirstSyncMode(),
    userMessage = userMessage,
    chosenNetworkMode = chosenNetworkMode,
    networkProbeSummary = networkProbeSummary?.map { it.toModel() } ?: emptyList()
)

internal fun String?.toBackendType(): BackendType = when (this) {
    "git" -> BackendType.Git
    "github_api" -> BackendType.GithubApi
    else -> BackendType.GithubApi
}

internal fun BackendType?.toWire(): String = when (this ?: BackendType.GithubApi) {
    BackendType.Git -> "git"
    BackendType.GithubApi -> "github_api"
}

internal fun String?.toSyncTransport(): SyncTransport = when (this) {
    "ssh", "ssh_deploy_key" -> SyncTransport.SshKey
    else -> SyncTransport.HttpsToken
}

internal fun SyncTransport?.toWire(): String = when (this ?: SyncTransport.HttpsToken) {
    SyncTransport.HttpsToken -> "https_token"
    SyncTransport.SshKey -> "ssh_deploy_key"
}

internal fun String?.toSyncStatus(): SyncStatus = when (this) {
    "idle" -> SyncStatus.Idle
    "syncing" -> SyncStatus.Syncing
    "success" -> SyncStatus.Success
    "configured_untested" -> SyncStatus.ConfiguredUntested
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

internal fun String?.toFirstSyncMode(): FirstSyncMode = when (this) {
    "not_attempted" -> FirstSyncMode.NotAttempted
    "clone_into_empty_workspace" -> FirstSyncMode.CloneIntoEmptyWorkspace
    "init_existing_workspace" -> FirstSyncMode.InitExistingWorkspace
    "already_git_repo" -> FirstSyncMode.AlreadyGitRepo
    "blocked_non_empty_remote" -> FirstSyncMode.BlockedNonEmptyRemote
    "unrelated_histories" -> FirstSyncMode.UnrelatedHistories
    "none" -> FirstSyncMode.None
    else -> FirstSyncMode.None
}

internal fun DateRangeDto.toModel() = WritingStatsRange(
    startDate = startDate,
    endDate = endDate
)

internal fun WritingStatsSummaryDto.toModel() = WritingStatsSummary(
    range = range.toModel(),
    totalHumanTypedChars = totalHumanTypedChars.toLong(),
    totalActiveSeconds = totalActiveSeconds.toLong(),
    totalSessions = totalSessions.toInt(),
    daysCount = daysCount.toInt()
)

internal fun ProjectStatsRecordDto.toModel() = ProjectWritingStatsItem(
    projectId = projectId,
    humanTypedChars = humanTypedChars.toLong(),
    pastedChars = pastedChars.toLong(),
    deletedChars = deletedChars.toLong(),
    aiInsertedChars = aiInsertedChars.toLong(),
    netDeltaChars = netDeltaChars,
    activeSeconds = activeSeconds.toLong()
)

internal fun ProjectStatsSummaryDto.toModel() = ProjectWritingStatsSummary(
    range = range.toModel(),
    projects = projects.map { it.toModel() }
)

internal fun ChapterStatsRecordDto.toModel() = ChapterWritingStatsItem(
    chapterId = chapterId,
    humanTypedChars = humanTypedChars.toLong(),
    pastedChars = pastedChars.toLong(),
    deletedChars = deletedChars.toLong(),
    aiInsertedChars = aiInsertedChars.toLong(),
    netDeltaChars = netDeltaChars,
    activeSeconds = activeSeconds.toLong()
)

internal fun ChapterStatsSummaryDto.toModel() = ChapterWritingStatsSummary(
    range = range.toModel(),
    chapters = chapters.map { it.toModel() }
)

internal fun DeviceStatsRecordDto.toModel() = DeviceWritingStatsItem(
    deviceId = deviceId,
    platform = platform.name,
    humanTypedChars = humanTypedChars.toLong(),
    pastedChars = pastedChars.toLong(),
    deletedChars = deletedChars.toLong(),
    aiInsertedChars = aiInsertedChars.toLong(),
    netDeltaChars = netDeltaChars,
    activeSeconds = activeSeconds.toLong(),
    sessionsCount = sessionsCount.toInt()
)

internal fun DeviceStatsSummaryDto.toModel() = DeviceWritingStatsSummary(
    range = range.toModel(),
    devices = devices.map { it.toModel() }
)

internal fun SpeedCurvePointDto.toModel() = WritingSpeedBucket(
    startMs = startMs,
    endMs = endMs,
    charsTyped = charsTyped.toLong(),
    charsPerMinute = charsPerMinute.toDouble()
)

internal fun SpeedCurveSummaryDto.toModel() = WritingSpeedCurve(
    range = range.toModel(),
    bucketMinutes = bucketMinutes.toInt(),
    buckets = buckets.map { it.toModel() }
)