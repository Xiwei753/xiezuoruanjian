package com.xiwei.sujian.data

import com.xiwei.sujian.BuildConfig
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
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

    companion object {
        private const val TAG = "AppServiceBridge"
    }

    private inline fun <T> wrapResult(block: () -> T): BridgeResult<T> {
        return try {
            BridgeResult.Success(block())
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: WriterException) {
            DiagnosticsLogger.e(TAG, "Native exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error(e.toWireErrorCode(), e.message ?: "Unknown native exception"))
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "Unknown error"))
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

    // TODO: Add flushRecentEdits() once UniFFI bindings are regenerated
    // with the new flush_recent_edits method from api.udl

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

    fun calculateWordCount(text: String): Int {
        return try {
            service.calculateWordCount(text).toInt()
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            text.length
        }
    }

    fun loadLocalSettings(): BridgeResult<LocalSettings> = wrapResult {
        service.loadLocalSettings().toModel()
    }

    fun saveLocalSettings(settings: LocalSettings): BridgeResult<Boolean> {
        return try {
            val res = service.saveLocalSettings(settings.toDto())
            BridgeResult.Success(res, ResultEnvelope(success = true, data = res, changedEntities = listOf(ChangedEntity("SettingsSaved"))))
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: WriterException) {
            DiagnosticsLogger.e(TAG, "Native exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error(e.toWireErrorCode(), e.message ?: "Unknown native exception"))
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "Unknown error"))
        }
    }

    fun loadSyncableSettings(): BridgeResult<SyncableSettings> = wrapResult {
        service.loadSyncableSettings().toModel()
    }

    fun saveSyncableSettings(settings: SyncableSettings): BridgeResult<Boolean> {
        return try {
            val res = service.saveSyncableSettings(settings.toDto())
            BridgeResult.Success(res, ResultEnvelope(success = true, data = res, changedEntities = listOf(ChangedEntity("SettingsSaved"))))
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: WriterException) {
            DiagnosticsLogger.e(TAG, "Native exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error(e.toWireErrorCode(), e.message ?: "Unknown native exception"))
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "Unknown error"))
        }
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

    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, durationSeconds: Int, sessionId: String): BridgeResult<Boolean> = wrapResult {
        service.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, durationSeconds, sessionId)
    }

    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, durationSeconds: UInt, sessionId: String): BridgeResult<Boolean> = wrapResult {
        service.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, durationSeconds, sessionId)
    }

    fun flushWritingStats(): BridgeResult<Boolean> = wrapResult {
        service.flushWritingStats()
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

    fun getStarMapViewport(starmapId: String): BridgeResult<uniffi.writer_core.StarMapViewportDto> = wrapResult {
        service.getStarmapViewport(starmapId)
    }

    fun saveStarMapViewport(starmapId: String, viewport: uniffi.writer_core.StarMapViewportDto): BridgeResult<Boolean> = wrapResult {
        service.saveStarmapViewport(starmapId, viewport)
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

    /**
     * 获取星图动画策略参数。
     *
     * 临时兼容：当前 UniFFI 生成绑定可能尚未暴露 getStarmapMotionPolicy()，所以这里仍保留
     * 反射探测 fallback。后续新能力必须放到领域 Bridge，不再继续往 AppServiceBridge 增加领域方法。
     * Debug 构建只要走 fallback 必须 Log.w，避免静默返回默认值掩盖绑定缺口。
     */
    fun getStarMapMotionPolicy(): BridgeResult<com.xiwei.sujian.model.StarMapMotionPolicyData> {
        fun fallback(reason: String, throwable: Throwable? = null): BridgeResult<com.xiwei.sujian.model.StarMapMotionPolicyData> {
            if (BuildConfig.DEBUG) {
                DiagnosticsLogger.w(TAG, "Temporary compatibility fallback for getStarmapMotionPolicy: $reason", throwable)
            }
            return BridgeResult.Success(com.xiwei.sujian.model.StarMapMotionPolicyData())
        }

        return try {
            val method = service.javaClass.getMethod("getStarmapMotionPolicy")
            val dto = method.invoke(service) ?: return fallback("UniFFI method returned null")
            val dtoClass = dto.javaClass
            val result = com.xiwei.sujian.model.StarMapMotionPolicyData(
                enabled = dtoClass.getField("enabled").getBoolean(dto),
                idleWobbleEnabled = dtoClass.getField("idleWobbleEnabled").getBoolean(dto),
                idleAmplitudeVp = dtoClass.getField("idleAmplitudeVp").getFloat(dto),
                idlePeriodMs = dtoClass.getField("idlePeriodMs").getInt(dto),
                dragLiftScale = dtoClass.getField("dragLiftScale").getFloat(dto),
                dragShadowBoost = dtoClass.getField("dragShadowBoost").getFloat(dto),
                settleDurationMs = dtoClass.getField("settleDurationMs").getInt(dto),
                reduceMotion = dtoClass.getField("reduceMotion").getBoolean(dto)
            )
            BridgeResult.Success(result)
        } catch (e: NoSuchMethodException) {
            fallback("UniFFI binding has no getStarmapMotionPolicy", e)
        } catch (e: NoSuchFieldException) {
            fallback("DTO field mismatch", e)
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to get motion policy: ${e.message}", e)
            fallback("reflection invocation failed", e)
        }
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

    // ── Layout Policy ──

    fun resolveLayout(metrics: uniffi.writer_core.WindowMetricsDto): BridgeResult<uniffi.writer_core.LayoutPlanDto> = wrapResult {
        service.resolveLayout(metrics)
    }

    // ── Screen Policy ──

    fun resolveScreenPolicy(screenRole: uniffi.writer_core.ScreenRoleDto, shellMode: uniffi.writer_core.ShellModeDto): BridgeResult<uniffi.writer_core.ScreenPolicyDto> = wrapResult {
        service.resolveScreenPolicy(screenRole, shellMode)
    }

    // ── Editor Animation ──
    /**
     * Internal UniFFI adapter for EditorAnimationBridge.
     *
     * Keep Android on typed DTO/model flow. Desktop QML may continue to expose animation_events_json,
     * but Android must not serialize typed DTOs to JSON and hand-parse them back.
     */
    internal fun editorAnimationEventDtos(
        oldText: String,
        newText: String,
        oldCursorIndex: UInt,
        newCursorIndex: UInt,
        cause: String,
        maxAnimatedChars: UInt,
        animationDurationMs: ULong
    ): BridgeResult<List<uniffi.writer_core.EditorAnimationEventDto>> = wrapResult {
        val causeDto = when (cause) {
            "Typing" -> uniffi.writer_core.EditorTransactionCauseDto.TYPING
            "Delete" -> uniffi.writer_core.EditorTransactionCauseDto.DELETE
            "ImeComposition" -> uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION
            "Paste" -> uniffi.writer_core.EditorTransactionCauseDto.PASTE
            "Undo" -> uniffi.writer_core.EditorTransactionCauseDto.UNDO
            "Redo" -> uniffi.writer_core.EditorTransactionCauseDto.REDO
            "Load" -> uniffi.writer_core.EditorTransactionCauseDto.LOAD
            "Format" -> uniffi.writer_core.EditorTransactionCauseDto.FORMAT
            "Programmatic" -> uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC
            else -> uniffi.writer_core.EditorTransactionCauseDto.TYPING
        }
        service.editorAnimationEvents(
            oldText, newText, oldCursorIndex, newCursorIndex,
            causeDto, maxAnimatedChars, animationDurationMs
        )
    }
}

