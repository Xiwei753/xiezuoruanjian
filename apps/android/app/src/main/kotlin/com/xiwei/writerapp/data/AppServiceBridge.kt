package com.xiwei.writerapp.data

import uniffi.writer_core.WriterAppService
import uniffi.writer_core.WriterAppServiceException
import uniffi.writer_core.ProjectDto
import uniffi.writer_core.VolumeDto
import uniffi.writer_core.ChapterMetaDto
import uniffi.writer_core.ChapterContentDto
import uniffi.writer_core.LocalSettingsDto
import uniffi.writer_core.SyncableSettingsDto
import uniffi.writer_core.SyncConfigDto
import uniffi.writer_core.SyncSecretsDto
import uniffi.writer_core.SyncStateDto
import uniffi.writer_core.SyncDiagnosticsResultDto
import uniffi.writer_core.SyncPlanDto
import uniffi.writer_core.SyncResultDto
import uniffi.writer_core.SyncConflictDto
import uniffi.writer_core.NetworkProbeResultDto
import uniffi.writer_core.RecentEditDto

import com.xiwei.writerapp.data.BridgeErrorCode
import com.xiwei.writerapp.data.BridgeError
import com.xiwei.writerapp.data.BridgeResult
import com.xiwei.writerapp.model.Project
import com.xiwei.writerapp.model.Volume
import com.xiwei.writerapp.model.ChapterMeta
import com.xiwei.writerapp.model.ChapterContent
import com.xiwei.writerapp.model.LocalSettings
import com.xiwei.writerapp.model.SyncableSettings
import com.xiwei.writerapp.model.SyncConfig
import com.xiwei.writerapp.model.SyncSecrets
import com.xiwei.writerapp.model.SyncState
import com.xiwei.writerapp.model.SyncDiagnosticsResult
import com.xiwei.writerapp.model.SyncPlan
import com.xiwei.writerapp.model.SyncResult
import com.xiwei.writerapp.model.SyncConflict
import com.xiwei.writerapp.model.NetworkProbeResult
import com.xiwei.writerapp.model.BridgeError
import com.xiwei.writerapp.model.BridgeErrorCode
import com.xiwei.writerapp.model.BridgeResult
import com.xiwei.writerapp.model.BackendType
import com.xiwei.writerapp.model.SyncTransport

import android.util.Log

class AppServiceBridge(private val workspacePath: String) {
    private val service: WriterAppService = WriterAppService(workspacePath)

    companion object {
        private const val TAG = "AppServiceBridge"
    }

    private inline fun <T> wrapResult(block: () -> T): BridgeResult<T> {
        return try {
            BridgeResult.Success(block())
        } catch (e: uniffi.writer_core.WriterAppServiceException) {
            Log.e(TAG, "WriterError: \${e.message}", e)
            val code = when (e) {
                is WriterError.InvalidWorkspace -> BridgeErrorCode.INVALID_WORKSPACE
                is WriterError.ProjectNotFound -> BridgeErrorCode.PROJECT_NOT_FOUND
                is WriterError.VolumeNotFound -> BridgeErrorCode.VOLUME_NOT_FOUND
                is WriterError.ChapterNotFound -> BridgeErrorCode.CHAPTER_NOT_FOUND
                is WriterError.Io -> BridgeErrorCode.IO_ERROR
                is WriterError.Json -> BridgeErrorCode.JSON_PARSE_ERROR
                is WriterError.EmptyOverwriteBlocked -> BridgeErrorCode.EMPTY_OVERWRITE_BLOCKED
                else -> BridgeErrorCode.UNKNOWN
            }
            BridgeResult.Error(BridgeError(code, e.message ?: "Unknown WriterError"))
        } catch (e: Exception) {
            Log.e(TAG, "Exception: \${e.message}", e)
            BridgeResult.Error(BridgeError(BridgeErrorCode.UNKNOWN, e.message ?: "Unknown error"))
        }
    }

    fun listProjects(): BridgeResult<List<Project>> = wrapResult {
        service.listProjects().map { it.toModel() }
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

    fun openChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterContent> = wrapResult {
        service.openChapter(projectId, volumeId, chapterId).toModel()
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<Boolean> = wrapResult {
        service.saveChapterContent(projectId, volumeId, chapterId, content)
        true
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): BridgeResult<Boolean> = wrapResult {
        service.updateChapterNote(projectId, volumeId, chapterId, note)
    }

    fun calculateWordCount(text: String): Long {
        return service.calculateWordCount(text).toLong()
    }

    // Settings
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

    // Sync
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

    // Stats (still strings due to UI mapping complexity, but via typed core calls that serialize on rust side)
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
}

// Extension methods to convert between DTO and Model

fun ProjectDto.toModel() = Project(id, title, createdAt, updatedAt)
fun VolumeDto.toModel() = Volume(id, title, createdAt, updatedAt, order)
fun ChapterMetaDto.toModel() = ChapterMeta(id, title, createdAt, updatedAt, order, wordCount.toLong(), hash, note)
fun ChapterContentDto.toModel() = ChapterContent(meta.toModel(), content)

fun LocalSettingsDto.toModel() = LocalSettings(
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
    editorTypingAnimationDurationMs = editorTypingAnimationDurationMs.toLong(),
    editorSmoothCursorDurationMs = editorSmoothCursorDurationMs.toLong(),
    aiEnabled = aiEnabled,
    ,
    editorLineSpacingMultiplier = 1.5
)

fun LocalSettings.toDto() = LocalSettingsDto(
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
    aiEnabled = aiEnabled
)

fun SyncableSettingsDto.toModel() = SyncableSettings(fontSize, themeMode, monetColor)
fun SyncableSettings.toDto() = SyncableSettingsDto(fontSize, themeMode, monetColor)

fun SyncConfigDto.toModel() = SyncConfig(
    enabled = enabled,
    backendType = when(backendType) {
        "git" -> BackendType.GIT
        "github_api" -> BackendType.GITHUB_API
        "webdav" -> BackendType.WEBDAV
        "s3" -> BackendType.S3
        else -> BackendType.GITHUB_API
    },
    remoteUrl = remoteUrl,
    transport = when(transport) {
        "https_token" -> SyncTransport.HTTPS_TOKEN
        "ssh" -> SyncTransport.SSH_DEPLOY_KEY
        else -> SyncTransport.HTTPS_TOKEN
    },
    branch = branch,
    autoSync = autoSync,
    syncIntervalSeconds = syncIntervalSeconds.toInt(),




    username = username,
    androidHasAccessNetworkStatePermission = false,
    androidHasInternetPermission = false
)

fun SyncConfig.toDto() = SyncConfigDto(
    enabled = enabled,
    backendType = when(backendType) {
        BackendType.GIT -> "git"
        BackendType.GITHUB_API -> "github_api"
        BackendType.WEBDAV -> "webdav"
        BackendType.S3 -> "s3"
        else -> "github_api"
    },
    remoteUrl = remoteUrl,
    transport = when(transport) {
        SyncTransport.HTTPS_TOKEN -> "https_token"
        SyncTransport.SSH_DEPLOY_KEY -> "ssh"
        else -> "https_token"
    },
    branch = branch,
    autoSync = autoSync,
    syncIntervalSeconds = syncIntervalSeconds.toUInt(),




    username = username
)

fun SyncSecretsDto.toModel() = SyncSecrets(token, null)
fun SyncSecrets.toDto() = SyncSecretsDto(token)

fun SyncStateDto.toModel() = SyncState(
    status = com.xiwei.writerapp.model.SyncStatus.IDLE, // simplified
    backendType = null,
    transport = null,
    lastSyncedCommit = lastSyncedCommit,
    lastSyncTime = lastSyncTime,
    lastError = lastError,
    lastSuccessfulNetworkMode = lastSuccessfulNetworkMode,
    conflicts = conflicts?.map { it.toModel() }
)

fun SyncConflictDto.toModel() = SyncConflict(localPath, remotePath, localHash, remoteHash, baseHash, createdAt, description)

fun NetworkProbeResultDto.toModel() = NetworkProbeResult(mode, success, status, message, rawError)

fun SyncDiagnosticsResultDto.toModel() = SyncDiagnosticsResult(
    success = success,
    backendType = BackendType.GITHUB_API,
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
    transport = SyncTransport.HTTPS_TOKEN,
    errorCategory = errorCategory,
    userMessage = userMessage,
    rawError = rawError,
    chosenNetworkMode = chosenNetworkMode,
    networkProbeSummary = networkProbeSummary?.map { it.toModel() }
)

fun SyncPlanDto.toModel() = SyncPlan(
    filesToUpload = filesToUpload,
    filesToDownload = filesToDownload,
    filesToDeleteLocal = filesToDeleteLocal,
    filesToDeleteRemote = filesToDeleteRemote,
    ignoredFiles = ignoredFiles,
    conflicts = conflicts
)

fun SyncResultDto.toModel() = SyncResult(
    status = com.xiwei.writerapp.model.SyncStatus.IDLE,
    uploadedFiles = uploadedFiles,
    downloadedFiles = downloadedFiles,
    localDeletes = localDeletes,
    remoteDeletes = remoteDeletes,
    overwrittenFiles = overwrittenFiles,
    ignoredFiles = ignoredFiles,
    conflicts = conflicts.map { it.toModel() },
    commitHash = commitHash,
    error = error,
    firstSyncMode = com.xiwei.writerapp.model.FirstSyncMode.NONE,
    userMessage = userMessage,
    chosenNetworkMode = chosenNetworkMode,
    networkProbeSummary = networkProbeSummary?.map { it.toModel() }
)
