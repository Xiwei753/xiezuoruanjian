package com.xiwei.sujian.data

import android.util.Log
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
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
import uniffi.writer_core.WriterAppService
import uniffi.writer_core.WriterException
import uniffi.writer_core.WritingStatsSummaryDto

class AppServiceBridge(workspacePath: String) {
    private val service: WriterAppService by lazy { WriterAppService(workspacePath) }
    private val gson = Gson()

    companion object {
        private const val TAG = "AppServiceBridge"
    }

    private inline fun <T> wrapResult(block: () -> T): BridgeResult<T> {
        return try {
            BridgeResult.Success(block())
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: WriterException) {
            Log.e(TAG, "Native exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error(e.toWireErrorCode(), e.message ?: "Unknown native exception"))
        } catch (e: Exception) {
            Log.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "Unknown error"))
        }
    }

    private inline fun <reified T> envelopeJsonResult(envelopeJson: String): BridgeResult<T> {
        return try {
            val type = object : TypeToken<ResultEnvelope<T>>() {}.type
            val envelope = gson.fromJson<ResultEnvelope<T>>(envelopeJson, type)
            if (envelope.success) {
                val data = envelope.data ?: return BridgeResult.Error(
                    ResultEnvelope.error("JSON_ERROR", "ResultEnvelope 缺少 data")
                )
                BridgeResult.Success(data, envelope)
            } else {
                BridgeResult.Error(
                    ResultEnvelope(
                        success = false,
                        errorCode = envelope.errorCode,
                        userMessage = envelope.userMessage,
                        rawError = envelope.rawError,
                        warnings = envelope.warnings,
                        changedPaths = envelope.changedPaths,
                        changedEntities = envelope.changedEntities
                    )
                )
            }
        } catch (e: Exception) {
            Log.e(TAG, "ResultEnvelope parse failed: ${e.message}", e)
            BridgeResult.Error(
                ResultEnvelope.error("JSON_ERROR", e.message ?: "ResultEnvelope 解析失败")
            )
        }
    }

    private inline fun <reified T> envelopeJsonCall(block: () -> String): BridgeResult<T> {
        return try {
            envelopeJsonResult(block())
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: Exception) {
            Log.e(TAG, "ResultEnvelope call failed: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "ResultEnvelope 调用失败"))
        }
    }

    fun listProjects(): BridgeResult<List<Project>> = wrapResult {
        service.listProjects().map { it.toModel() }
    }

    fun getRecentEdits(): BridgeResult<List<com.xiwei.sujian.model.RecentEdit>> = wrapResult {
        service.getRecentEdits().map { com.xiwei.sujian.model.RecentEdit(it.projectId, it.volumeId, it.chapterId, it.timestamp) }
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

    fun createProject(title: String): BridgeResult<Project> =
        envelopeJsonCall { service.createProjectEnvelopeJson(title) }

    fun getProjectStats(projectId: String): BridgeResult<ProjectStats> = wrapResult {
        service.getProjectStats(projectId).toModel()
    }

    fun renameProject(projectId: String, newTitle: String): BridgeResult<Boolean> =
        envelopeJsonCall { service.renameProjectEnvelopeJson(projectId, newTitle) }

    fun deleteProject(projectId: String): BridgeResult<Boolean> =
        envelopeJsonCall { service.deleteProjectEnvelopeJson(projectId) }

    fun reorderProjects(orderedIds: List<String>): BridgeResult<Boolean> =
        envelopeJsonCall { service.reorderProjectsEnvelopeJson(orderedIds) }

    fun listVolumes(projectId: String): BridgeResult<List<Volume>> = wrapResult {
        service.listVolumes(projectId).map { it.toModel() }
    }

    fun createVolume(projectId: String, title: String): BridgeResult<Volume> =
        envelopeJsonCall { service.createVolumeEnvelopeJson(projectId, title) }

    fun renameVolume(projectId: String, volumeId: String, newTitle: String): BridgeResult<Boolean> =
        envelopeJsonCall { service.renameVolumeEnvelopeJson(projectId, volumeId, newTitle) }

    fun deleteVolume(projectId: String, volumeId: String): BridgeResult<Boolean> =
        envelopeJsonCall { service.deleteVolumeEnvelopeJson(projectId, volumeId) }

    fun reorderVolumes(projectId: String, orderedIds: List<String>): BridgeResult<Boolean> =
        envelopeJsonCall { service.reorderVolumesEnvelopeJson(projectId, orderedIds) }

    fun listChapters(projectId: String, volumeId: String): BridgeResult<List<ChapterMeta>> = wrapResult {
        service.listChapters(projectId, volumeId).map { it.toModel() }
    }

    fun createChapter(projectId: String, volumeId: String, title: String): BridgeResult<ChapterMeta> =
        envelopeJsonCall { service.createChapterEnvelopeJson(projectId, volumeId, title) }

    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String): BridgeResult<Boolean> =
        envelopeJsonCall { service.renameChapterEnvelopeJson(projectId, volumeId, chapterId, newTitle) }

    fun deleteChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<Boolean> =
        envelopeJsonCall { service.deleteChapterEnvelopeJson(projectId, volumeId, chapterId) }

    fun reorderChapters(projectId: String, volumeId: String, orderedIds: List<String>): BridgeResult<Boolean> =
        envelopeJsonCall { service.reorderChaptersEnvelopeJson(projectId, volumeId, orderedIds) }

    fun openChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterOpenResult> = wrapResult {
        service.openChapter(projectId, volumeId, chapterId).toModel()
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<ChapterSaveReceipt> =
        envelopeJsonCall { service.saveChapterContentEnvelopeJson(projectId, volumeId, chapterId, content) }

    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterSaveReceipt> =
        envelopeJsonCall { service.clearChapterContentEnvelopeJson(projectId, volumeId, chapterId) }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): BridgeResult<Boolean> =
        envelopeJsonCall { service.updateChapterNoteEnvelopeJson(projectId, volumeId, chapterId, note) }

    fun calculateWordCount(text: String): Int {
        return try {
            service.calculateWordCount(text).toInt()
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Native library is not loaded", e)
            text.length
        }
    }

    fun loadLocalSettings(): BridgeResult<LocalSettings> = wrapResult {
        service.loadLocalSettings().toModel()
    }

    fun saveLocalSettings(settings: LocalSettings): BridgeResult<Boolean> =
        envelopeJsonCall { service.saveLocalSettingsEnvelopeJson(settings.toDto()) }

    fun loadSyncableSettings(): BridgeResult<SyncableSettings> = wrapResult {
        service.loadSyncableSettings().toModel()
    }

    fun saveSyncableSettings(settings: SyncableSettings): BridgeResult<Boolean> =
        envelopeJsonCall { service.saveSyncableSettingsEnvelopeJson(settings.toDto()) }

    fun loadSyncConfig(): BridgeResult<SyncConfig> = wrapResult {
        service.loadSyncConfig().toModel()
    }

    fun saveSyncConfig(config: SyncConfig): BridgeResult<Boolean> =
        envelopeJsonCall { service.saveSyncConfigEnvelopeJson(config.toDto()) }

    fun loadSyncSecrets(): BridgeResult<SyncSecrets> = wrapResult {
        service.loadSyncSecrets().toModel()
    }

    fun saveSyncSecrets(secrets: SyncSecrets): BridgeResult<Boolean> =
        envelopeJsonCall { service.saveSyncSecretsEnvelopeJson(secrets.toDto()) }

    fun loadSyncState(): BridgeResult<SyncState> = wrapResult {
        service.loadSyncState().toModel()
    }

    fun performSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> =
        envelopeJsonCall { service.performSyncDiagnosticsEnvelopeJson(config.toDto()) }

    fun performSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> =
        envelopeJsonCall { service.performSyncDryRunEnvelopeJson(config.toDto()) }

    fun performSync(config: SyncConfig): BridgeResult<SyncResult> =
        envelopeJsonCall { service.performSyncEnvelopeJson(config.toDto()) }

    fun getWritingStatsSummary(startDate: String, endDate: String): BridgeResult<WritingStatsSummary> = wrapResult {
        service.getWritingStatsSummary(startDate, endDate).toModel()
    }

    fun getWritingStatsByProject(startDate: String, endDate: String): BridgeResult<ProjectWritingStatsSummary> = wrapResult {
        service.getWritingStatsByProject(startDate, endDate).toModel()
    }

    fun getWritingStatsByChapter(startDate: String, endDate: String): BridgeResult<ChapterWritingStatsSummary> = wrapResult {
        service.getWritingStatsByChapter(startDate, endDate).toModel()
    }

    fun getWritingStatsByDevice(startDate: String, endDate: String): BridgeResult<DeviceWritingStatsSummary> = wrapResult {
        service.getWritingStatsByDevice(startDate, endDate).toModel()
    }

    fun getWritingSpeedCurve(startDate: String, endDate: String, bucketMinutes: Int): BridgeResult<WritingSpeedCurve> = wrapResult {
        service.getWritingSpeedCurve(startDate, endDate, bucketMinutes.toUInt()).toModel()
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

    fun getMindMapSnapshot(projectId: String): BridgeResult<uniffi.writer_core.MindMapSnapshotDto> = wrapResult {
        service.getMindmapSnapshot(projectId)
    }

    fun listStarMaps(): BridgeResult<List<uniffi.writer_core.StarMapMetaDto>> = wrapResult {
        service.listStarmaps()
    }

    fun getStarMapGraph(starmapId: String): BridgeResult<uniffi.writer_core.StarMapGraphDto> = wrapResult {
        service.getStarmapGraph(starmapId)
    }

    fun createStarMap(title: String, desc: String): BridgeResult<uniffi.writer_core.StarMapMetaDto> = wrapResult {
        service.createStarmap(title, desc)
    }

    fun addStarMapNode(starmapId: String, node: uniffi.writer_core.StarMapNodeDto, x: Float, y: Float): BridgeResult<uniffi.writer_core.StarMapNodeDto> = wrapResult {
        service.addStarmapNode(starmapId, node, x, y)
    }

    fun saveStarMapLayout(starmapId: String, layout: uniffi.writer_core.StarMapLayoutDto): BridgeResult<Boolean> = wrapResult {
        service.saveStarmapLayout(starmapId, layout)
    }

    fun computeStarMapEdgeRenders(graph: uniffi.writer_core.StarMapGraphDto, layout: uniffi.writer_core.StarMapLayoutDto): BridgeResult<List<uniffi.writer_core.StarMapEdgeRenderDto>> = wrapResult {
        service.computeStarmapEdgeRenders(graph, layout)
    }

    fun hitTestStarMapNode(layout: uniffi.writer_core.StarMapLayoutDto, x: Float, y: Float): BridgeResult<String?> = wrapResult {
        service.hitTestStarmapNode(layout, x, y)
    }

    fun addStarmapEmbed(starmapId: String, embed: uniffi.writer_core.StarMapEmbedDto): BridgeResult<uniffi.writer_core.StarMapEmbedDto> = wrapResult {
        service.addStarmapEmbed(starmapId, embed)
    }

    fun updateStarmapEmbed(starmapId: String, instanceId: String, patch: uniffi.writer_core.StarMapEmbedPatchInputDto): BridgeResult<uniffi.writer_core.StarMapEmbedDto> = wrapResult {
        service.updateStarmapEmbed(starmapId, instanceId, patch)
    }

    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = wrapResult {
        service.deleteStarmapEmbed(starmapId, instanceId)
    }

    fun addStarmapLink(starmapId: String, link: uniffi.writer_core.StarMapLinkDto): BridgeResult<uniffi.writer_core.StarMapLinkDto> = wrapResult {
        service.addStarmapLink(starmapId, link)
    }

    fun updateStarmapLink(starmapId: String, linkId: String, patch: uniffi.writer_core.StarMapLinkPatchInputDto): BridgeResult<uniffi.writer_core.StarMapLinkDto> = wrapResult {
        service.updateStarmapLink(starmapId, linkId, patch)
    }

    fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean> = wrapResult {
        service.deleteStarmapLink(starmapId, linkId)
    }

    fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<uniffi.writer_core.StarMapReferenceDto>> = wrapResult {
        service.findStarmapReferences(targetStarmapId)
    }

    fun listRegisteredActions(): BridgeResult<List<uniffi.writer_core.ActionDescriptorDto>> = wrapResult {
        service.listRegisteredActions()
    }

    fun executeAction(actionId: String, argsJson: String, contextJson: String): BridgeResult<uniffi.writer_core.ActionResultDto> = wrapResult {
        service.executeAction(actionId, argsJson, contextJson)
    }

    fun aiAvailable(): Boolean = try {
        service.aiAvailable()
    } catch (e: UnsatisfiedLinkError) {
        false
    }
}

private fun WriterException.toWireErrorCode(): String = when (this) {
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

private fun LocalSettings.toDto() = LocalSettingsDto(
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
        username = normalized.username ?: "",
        androidHasInternetPermission = normalized.androidHasInternetPermission ?: false,
        androidHasAccessNetworkStatePermission = normalized.androidHasAccessNetworkStatePermission ?: false
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
    errorCategory = errorCategory,
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
    "partial_conflict" -> SyncStatus.PartialConflict
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

private fun DateRangeDto.toModel() = WritingStatsRange(
    startDate = startDate,
    endDate = endDate
)

private fun WritingStatsSummaryDto.toModel() = WritingStatsSummary(
    range = range.toModel(),
    totalHumanTypedChars = totalHumanTypedChars.toLong(),
    totalActiveSeconds = totalActiveSeconds.toLong(),
    totalSessions = totalSessions.toInt(),
    daysCount = daysCount.toInt()
)

private fun ProjectStatsRecordDto.toModel() = ProjectWritingStatsItem(
    projectId = projectId,
    humanTypedChars = humanTypedChars.toLong(),
    pastedChars = pastedChars.toLong(),
    deletedChars = deletedChars.toLong(),
    aiInsertedChars = aiInsertedChars.toLong(),
    netDeltaChars = netDeltaChars,
    activeSeconds = activeSeconds.toLong()
)

private fun ProjectStatsSummaryDto.toModel() = ProjectWritingStatsSummary(
    range = range.toModel(),
    projects = projects.map { it.toModel() }
)

private fun ChapterStatsRecordDto.toModel() = ChapterWritingStatsItem(
    chapterId = chapterId,
    humanTypedChars = humanTypedChars.toLong(),
    pastedChars = pastedChars.toLong(),
    deletedChars = deletedChars.toLong(),
    aiInsertedChars = aiInsertedChars.toLong(),
    netDeltaChars = netDeltaChars,
    activeSeconds = activeSeconds.toLong()
)

private fun ChapterStatsSummaryDto.toModel() = ChapterWritingStatsSummary(
    range = range.toModel(),
    chapters = chapters.map { it.toModel() }
)

private fun DeviceStatsRecordDto.toModel() = DeviceWritingStatsItem(
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

private fun DeviceStatsSummaryDto.toModel() = DeviceWritingStatsSummary(
    range = range.toModel(),
    devices = devices.map { it.toModel() }
)

private fun SpeedCurvePointDto.toModel() = WritingSpeedBucket(
    startMs = startMs,
    endMs = endMs,
    charsTyped = charsTyped.toLong(),
    charsPerMinute = charsPerMinute.toDouble()
)

private fun SpeedCurveSummaryDto.toModel() = WritingSpeedCurve(
    range = range.toModel(),
    bucketMinutes = bucketMinutes.toInt(),
    buckets = buckets.map { it.toModel() }
)
