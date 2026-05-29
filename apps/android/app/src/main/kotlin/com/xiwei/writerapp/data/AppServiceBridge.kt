package com.xiwei.writerapp.data

import android.util.Log
import com.xiwei.writerapp.model.BackendType
import com.xiwei.writerapp.model.BridgeError
import com.xiwei.writerapp.model.BridgeErrorCode
import com.xiwei.writerapp.model.ChapterMeta
import com.xiwei.writerapp.model.ChapterOpenResult
import com.xiwei.writerapp.model.ChapterSaveReceipt
import com.xiwei.writerapp.model.FirstSyncMode
import com.xiwei.writerapp.model.LocalSettings
import com.xiwei.writerapp.model.NetworkProbeResult
import com.xiwei.writerapp.model.Project
import com.xiwei.writerapp.model.ProjectStats
import com.xiwei.writerapp.model.SyncConfig
import com.xiwei.writerapp.model.SyncConflict
import com.xiwei.writerapp.model.SyncDiagnosticsResult
import com.xiwei.writerapp.model.SyncPlan
import com.xiwei.writerapp.model.SyncResult
import com.xiwei.writerapp.model.SyncSecrets
import com.xiwei.writerapp.model.SyncState
import com.xiwei.writerapp.model.SyncStatus
import com.xiwei.writerapp.model.SyncTransport
import com.xiwei.writerapp.model.SyncableSettings
import com.xiwei.writerapp.model.Volume
import uniffi.writer_core.ChapterContentDto
import uniffi.writer_core.ChapterMetaDto
import uniffi.writer_core.ChapterSaveReceiptDto
import uniffi.writer_core.LocalSettingsDto
import uniffi.writer_core.NetworkProbeResultDto
import uniffi.writer_core.ProjectDto
import uniffi.writer_core.ProjectStatsDto
import uniffi.writer_core.SyncConfigDto
import uniffi.writer_core.SyncConflictDto
import uniffi.writer_core.SyncDiagnosticsResultDto
import uniffi.writer_core.SyncPlanDto
import uniffi.writer_core.SyncResultDto
import uniffi.writer_core.SyncSecretsDto
import uniffi.writer_core.SyncStateDto
import uniffi.writer_core.SyncableSettingsDto
import uniffi.writer_core.VolumeDto
import uniffi.writer_core.WriterAppService
import uniffi.writer_core.WriterException

class AppServiceBridge(workspacePath: String) {
    private val service: WriterAppService = WriterAppService(workspacePath)

    companion object {
        private const val TAG = "AppServiceBridge"
    }

    private inline fun <T> wrapResult(block: () -> T): BridgeResult<T> {
        return try {
            BridgeResult.Success(block())
        } catch (e: WriterException) {
            Log.e(TAG, "WriterException: ${e.message}", e)
            BridgeResult.Error(BridgeError(e.toBridgeErrorCode(), e.message ?: "Unknown WriterException"))
        } catch (e: Exception) {
            Log.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(BridgeError(BridgeErrorCode.Unknown, e.message ?: "Unknown error"))
        }
    }

    fun listProjects(): BridgeResult<List<Project>> = wrapResult {
        service.listProjects().map { it.toModel() }
    }

    fun getRecentEdits(): BridgeResult<List<com.xiwei.writerapp.model.RecentEdit>> = wrapResult {
        service.getRecentEdits().map { com.xiwei.writerapp.model.RecentEdit(it.projectId, it.volumeId, it.chapterId, it.timestamp) }
    }

    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String): BridgeResult<Boolean> = wrapResult {
        service.recordRecentEdit(projectId, volumeId, chapterId)
    }

    fun validateWorkspace(): BridgeResult<Boolean> = wrapResult {
        service.validateWorkspace()
    }

    fun createWorkspaceIfNeeded(): BridgeResult<Boolean> = wrapResult {
        service.createWorkspaceIfNeeded()
    }

    fun createProject(title: String): BridgeResult<Project> = wrapResult {
        service.createProject(title).toModel()
    }

    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> = wrapResult {
        service.getProjectStats(projectId).toModel()
    }

    fun renameProject(projectId: String, newTitle: String): BridgeResult<Boolean> = wrapResult {
        service.renameProject(projectId, newTitle)
    }

    fun deleteProject(projectId: String): BridgeResult<Boolean> = wrapResult {
        service.deleteProject(projectId)
    }

    fun reorderProjects(orderedIds: List<String>): BridgeResult<Boolean> = wrapResult {
        service.reorderProjects(orderedIds)
    }

    fun listVolumes(projectId: String): BridgeResult<List<Volume>> = wrapResult {
        service.listVolumes(projectId).map { it.toModel() }
    }

    fun createVolume(projectId: String, title: String): BridgeResult<Volume> = wrapResult {
        service.createVolume(projectId, title).toModel()
    }

    fun renameVolume(projectId: String, volumeId: String, newTitle: String): BridgeResult<Boolean> = wrapResult {
        service.renameVolume(projectId, volumeId, newTitle)
    }

    fun deleteVolume(projectId: String, volumeId: String): BridgeResult<Boolean> = wrapResult {
        service.deleteVolume(projectId, volumeId)
    }

    fun reorderVolumes(projectId: String, orderedIds: List<String>): BridgeResult<Boolean> = wrapResult {
        service.reorderVolumes(projectId, orderedIds)
    }

    fun listChapters(projectId: String, volumeId: String): BridgeResult<List<ChapterMeta>> = wrapResult {
        service.listChapters(projectId, volumeId).map { it.toModel() }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): BridgeResult<ChapterMeta> = wrapResult {
        service.createChapter(projectId, volumeId, title).toModel()
    }

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String): BridgeResult<Boolean> = wrapResult {
        service.renameChapter(projectId, volumeId, chapterId, newTitle)
    }

    fun deleteChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<Boolean> = wrapResult {
        service.deleteChapter(projectId, volumeId, chapterId)
    }

    fun reorderChapters(projectId: String, volumeId: String, orderedIds: List<String>): BridgeResult<Boolean> = wrapResult {
        service.reorderChapters(projectId, volumeId, orderedIds)
    }

    fun openChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterOpenResult> = wrapResult {
        service.openChapter(projectId, volumeId, chapterId).toModel()
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<ChapterSaveReceipt> = wrapResult {
        service.saveChapterContent(projectId, volumeId, chapterId, content).toModel()
    }

    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterSaveReceipt> = wrapResult {
        service.clearChapterContent(projectId, volumeId, chapterId).toModel()
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): BridgeResult<Boolean> = wrapResult {
        service.updateChapterNote(projectId, volumeId, chapterId, note)
    }

    fun calculateWordCount(text: String): Int = service.calculateWordCount(text).toInt()

    fun loadLocalSettings(): BridgeResult<LocalSettings> = wrapResult {
        service.loadLocalSettings().toModel()
    }

    fun saveLocalSettings(settings: LocalSettings): BridgeResult<Boolean> = wrapResult {
        service.saveLocalSettings(settings.toDto())
    }

    fun loadSyncableSettings(): BridgeResult<SyncableSettings> = wrapResult {
        service.loadSyncableSettings().toModel()
    }

    fun saveSyncableSettings(settings: SyncableSettings): BridgeResult<Boolean> = wrapResult {
        service.saveSyncableSettings(settings.toDto())
    }

    fun loadSyncConfig(): BridgeResult<SyncConfig> = wrapResult {
        service.loadSyncConfig().toModel()
    }

    fun saveSyncConfig(config: SyncConfig): BridgeResult<Boolean> = wrapResult {
        service.saveSyncConfig(config.toDto())
    }

    fun loadSyncSecrets(): BridgeResult<SyncSecrets> = wrapResult {
        service.loadSyncSecrets().toModel()
    }

    fun saveSyncSecrets(secrets: SyncSecrets): BridgeResult<Boolean> = wrapResult {
        service.saveSyncSecrets(secrets.toDto())
    }

    fun loadSyncState(): BridgeResult<SyncState> = wrapResult {
        service.loadSyncState().toModel()
    }

    fun performSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> = wrapResult {
        service.performSyncDiagnostics(config.toDto()).toModel()
    }

    fun performSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> = wrapResult {
        service.performSyncDryRun(config.toDto()).toModel()
    }

    fun performSync(config: SyncConfig): BridgeResult<SyncResult> = wrapResult {
        service.performSync(config.toDto()).toModel()
    }

    fun getWritingStatsSummary(startDate: String, endDate: String): BridgeResult<String> = wrapResult {
        service.getWritingStatsSummary(startDate, endDate)
    }

    fun getWritingStatsByProject(startDate: String, endDate: String): BridgeResult<String> = wrapResult {
        service.getWritingStatsByProject(startDate, endDate)
    }

    fun getWritingStatsByChapter(startDate: String, endDate: String): BridgeResult<String> = wrapResult {
        service.getWritingStatsByChapter(startDate, endDate)
    }

    fun getWritingStatsByDevice(startDate: String, endDate: String): BridgeResult<String> = wrapResult {
        service.getWritingStatsByDevice(startDate, endDate)
    }

    fun getWritingSpeedCurve(startDate: String, endDate: String, bucketMinutes: Int): BridgeResult<String> = wrapResult {
        service.getWritingSpeedCurve(startDate, endDate, bucketMinutes.toUInt())
    }

    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, sessionId: String): BridgeResult<Boolean> = wrapResult {
        service.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId)
    }

    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, sessionId: String): BridgeResult<Boolean> = wrapResult {
        service.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, sessionId)
    }

    fun flushWritingStats(): BridgeResult<Boolean> = wrapResult {
        service.flushWritingStats()
    }

    fun getMindMapSnapshot(projectId: String): BridgeResult<String> = wrapResult {
        service.getMindmapSnapshotJson(projectId)
    }

    fun listStarMaps(): BridgeResult<String> = wrapResult {
        service.listStarmaps()
    }

    fun getStarMapGraph(starmapId: String): BridgeResult<String> = wrapResult {
        service.getStarmapGraph(starmapId)
    }

    fun createStarMap(title: String, desc: String): BridgeResult<String> = wrapResult {
        service.createStarmap(title, desc)
    }

    fun addStarMapNode(starmapId: String, nodeJson: String): BridgeResult<String> = wrapResult {
        service.addStarmapNode(starmapId, nodeJson)
    }

    fun saveStarMapLayout(starmapId: String, layoutJson: String): BridgeResult<Boolean> = wrapResult {
        service.saveStarmapLayout(starmapId, layoutJson)
    }

    fun addStarmapEmbed(starmapId: String, embedJson: String): BridgeResult<String> = wrapResult {
        service.addStarmapEmbed(starmapId, embedJson)
    }

    fun updateStarmapEmbed(starmapId: String, instanceId: String, patchJson: String): BridgeResult<String> = wrapResult {
        service.updateStarmapEmbed(starmapId, instanceId, patchJson)
    }

    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = wrapResult {
        service.deleteStarmapEmbed(starmapId, instanceId)
    }

    fun addStarmapLink(starmapId: String, linkJson: String): BridgeResult<String> = wrapResult {
        service.addStarmapLink(starmapId, linkJson)
    }

    fun updateStarmapLink(starmapId: String, linkId: String, patchJson: String): BridgeResult<String> = wrapResult {
        service.updateStarmapLink(starmapId, linkId, patchJson)
    }

    fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean> = wrapResult {
        service.deleteStarmapLink(starmapId, linkId)
    }

    fun findStarmapReferences(targetStarmapId: String): BridgeResult<String> = wrapResult {
        service.findStarmapReferences(targetStarmapId)
    }
}

private fun WriterException.toBridgeErrorCode(): BridgeErrorCode = when (this) {
    is WriterException.Io -> BridgeErrorCode.IoError
    is WriterException.Json -> BridgeErrorCode.JsonError
    is WriterException.InvalidWorkspace -> BridgeErrorCode.InvalidWorkspace
    is WriterException.ProjectNotFound -> BridgeErrorCode.ProjectNotFound
    is WriterException.VolumeNotFound -> BridgeErrorCode.VolumeNotFound
    is WriterException.ChapterNotFound -> BridgeErrorCode.ChapterNotFound
    is WriterException.EmptyOverwriteBlocked -> BridgeErrorCode.EmptyOverwriteBlocked
    is WriterException.NotImplemented -> BridgeErrorCode.NotImplemented
    is WriterException.RefuseToDeleteWorkspaceRoot -> BridgeErrorCode.RefuseDeleteWorkspaceRoot
    is WriterException.InvalidDeleteTarget -> BridgeErrorCode.InvalidDeleteTarget
    is WriterException.Other -> BridgeErrorCode.Other
}

private fun ProjectDto.toModel() = Project(id, title, createdAt, updatedAt)

private fun ProjectStatsDto.toModel() = ProjectStats(
    totalWordCount = totalWordCount.toInt(),
    volumeCount = volumeCount.toInt(),
    chapterCount = chapterCount.toInt()
)

private fun VolumeDto.toModel() = Volume(
    id = id,
    title = title,
    createdAt = createdAt,
    updatedAt = updatedAt,
    order = order
)

private fun ChapterMetaDto.toModel() = ChapterMeta(
    id = id,
    title = title,
    createdAt = createdAt,
    updatedAt = updatedAt,
    order = order,
    wordCount = wordCount.toInt(),
    hash = hash,
    note = note
)

private fun ChapterContentDto.toModel() = ChapterOpenResult(meta.toModel(), content)

private fun ChapterSaveReceiptDto.toModel() = ChapterSaveReceipt(
    chapterRelativePath = chapterRelativePath,
    contentLen = contentLen.toLong(),
    contentHash = contentHash,
    metaHash = metaHash,
    updatedAt = updatedAt,
    wordCount = wordCount.toInt()
)

private fun LocalSettingsDto.toModel() = LocalSettings(
    themeMode = themeMode,
    locale = locale,
    autoSaveEnabled = autoSaveEnabled,
    editorFontSize = editorFontSize,
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
    editorLineSpacingMultiplier = 1.5f
)

private fun LocalSettings.toDto() = LocalSettingsDto(
    themeMode = themeMode,
    locale = locale,
    autoSaveEnabled = autoSaveEnabled,
    editorFontSize = editorFontSize,
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
    editorLineSpacingMultiplier = editorLineSpacingMultiplier
)

private fun SyncableSettingsDto.toModel() = SyncableSettings(fontSize, themeMode, monetColor)
private fun SyncableSettings.toDto() = SyncableSettingsDto(fontSize, themeMode, monetColor)

private fun SyncConfigDto.toModel() = SyncConfig(
    enabled = enabled,
    backendType = backendType.toBackendType(),
    remoteUrl = remoteUrl,
    transport = transport.toSyncTransport(),
    branch = branch,
    autoSync = autoSync,
    syncIntervalSeconds = syncIntervalSeconds.toInt(),
    proxyEnabled = proxyEnabled,
    proxyType = proxyType,
    proxyHost = proxyHost,
    proxyPort = proxyPort.toInt(),
    username = username,
    androidHasAccessNetworkStatePermission = null,
    androidHasInternetPermission = null
)

private fun SyncConfig.toDto(): SyncConfigDto {
    val normalized = normalize()
    return SyncConfigDto(
        enabled = normalized.enabled ?: false,
        backendType = normalized.backendType.toWire(),
        remoteUrl = normalized.remoteUrl ?: "",
        transport = normalized.transport.toWire(),
        branch = normalized.branch ?: "main",
        autoSync = normalized.autoSync ?: false,
        syncIntervalSeconds = (normalized.syncIntervalSeconds ?: 300).toUInt(),
        proxyEnabled = normalized.proxyEnabled ?: false,
        proxyType = normalized.proxyType ?: "auto",
        proxyHost = normalized.proxyHost ?: "127.0.0.1",
        proxyPort = (normalized.proxyPort ?: 7890).coerceIn(0, UShort.MAX_VALUE.toInt()).toUShort(),
        username = normalized.username ?: ""
    )
}

private fun SyncSecretsDto.toModel() = SyncSecrets(token, null)
private fun SyncSecrets.toDto() = SyncSecretsDto(token)

private fun SyncStateDto.toModel() = SyncState(
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

private fun SyncConflictDto.toModel() = SyncConflict(localPath, remotePath, localHash, remoteHash, baseHash, createdAt, description)
private fun NetworkProbeResultDto.toModel() = NetworkProbeResult(mode, success, status, message, rawError)

private fun SyncDiagnosticsResultDto.toModel() = SyncDiagnosticsResult(
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

private fun SyncPlanDto.toModel() = SyncPlan(
    filesToUpload = filesToUpload,
    filesToDownload = filesToDownload,
    filesToDeleteLocal = filesToDeleteLocal,
    filesToDeleteRemote = filesToDeleteRemote,
    ignoredFiles = ignoredFiles,
    conflicts = conflicts
)

private fun SyncResultDto.toModel() = SyncResult(
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
    firstSyncMode = firstSyncMode.toFirstSyncMode(),
    userMessage = userMessage,
    chosenNetworkMode = chosenNetworkMode,
    networkProbeSummary = networkProbeSummary?.map { it.toModel() } ?: emptyList()
)

private fun String?.toBackendType(): BackendType = when (this) {
    "git" -> BackendType.Git
    "github_api" -> BackendType.GithubApi
    "webdav" -> BackendType.WebDav
    "s3" -> BackendType.S3
    "local_folder" -> BackendType.LocalFolder
    else -> BackendType.GithubApi
}

private fun BackendType?.toWire(): String = when (this ?: BackendType.GithubApi) {
    BackendType.Git -> "git"
    BackendType.GithubApi -> "github_api"
    BackendType.WebDav -> "webdav"
    BackendType.S3 -> "s3"
    BackendType.LocalFolder -> "local_folder"
}

private fun String?.toSyncTransport(): SyncTransport = when (this) {
    "ssh", "ssh_deploy_key" -> SyncTransport.SshKey
    else -> SyncTransport.HttpsToken
}

private fun SyncTransport?.toWire(): String = when (this ?: SyncTransport.HttpsToken) {
    SyncTransport.HttpsToken -> "https_token"
    SyncTransport.SshKey -> "ssh_deploy_key"
}

private fun String?.toSyncStatus(): SyncStatus = when (this) {
    "idle" -> SyncStatus.Idle
    "syncing" -> SyncStatus.Syncing
    "success" -> SyncStatus.Success
    "configured_untested" -> SyncStatus.ConfiguredUntested
    "conflict" -> SyncStatus.Conflict
    "recoverable_error" -> SyncStatus.RecoverableError
    "fatal_error" -> SyncStatus.FatalError
    "dirty_repo_blocked" -> SyncStatus.DirtyRepoBlocked
    "branch_missing_recovered" -> SyncStatus.BranchMissingRecovered
    "no_changes" -> SyncStatus.NoChanges
    "latest_wins_applied" -> SyncStatus.LatestWinsApplied
    else -> SyncStatus.Error
}

private fun String?.toFirstSyncMode(): FirstSyncMode = when (this) {
    "not_attempted" -> FirstSyncMode.NotAttempted
    "clone_into_empty_workspace" -> FirstSyncMode.CloneIntoEmptyWorkspace
    "init_existing_workspace" -> FirstSyncMode.InitExistingWorkspace
    "already_git_repo" -> FirstSyncMode.AlreadyGitRepo
    "blocked_non_empty_remote" -> FirstSyncMode.BlockedNonEmptyRemote
    "unrelated_histories" -> FirstSyncMode.UnrelatedHistories
    "none" -> FirstSyncMode.None
    else -> FirstSyncMode.None
}
