package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.ChapterMeta
import com.xiwei.sujian.model.ChapterOpenResult
import com.xiwei.sujian.model.ChapterSaveReceipt
import com.xiwei.sujian.model.ChapterWritingStatsSummary
import com.xiwei.sujian.model.DeviceWritingStatsSummary
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.ProjectStats
import com.xiwei.sujian.model.ProjectWritingStatsSummary
import com.xiwei.sujian.model.SyncCapabilityData
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncDiagnosticsResult
import com.xiwei.sujian.model.SyncPlan
import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncSecrets
import com.xiwei.sujian.model.SyncState
import com.xiwei.sujian.model.SyncableSettings
import com.xiwei.sujian.model.Volume
import com.xiwei.sujian.model.WritingSpeedCurve
import com.xiwei.sujian.model.WritingStatsSummary
import uniffi.writer_core.ActionDescriptorDto
import uniffi.writer_core.ActionResultDto
import uniffi.writer_core.ScreenRoleDto
import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ShellModeDto

/**
 * AppServiceBridge — 门面类（向后兼容）。
 *
 * 内部已拆分为领域 Bridge（ProjectBridge、ChapterBridge、SettingsBridge 等），
 * 原有公开 API 全部委托到对应领域 Bridge，行为不变。
 *
 * 新代码应直接使用领域 Bridge，不再依赖此门面类。
 */
class AppServiceBridge(val holder: WriterAppServiceHolder) {
    constructor(workspacePath: String) : this(WriterAppServiceHolder(workspacePath))

    // ── 领域 Bridge ──
    val projectBridge: ProjectBridge by lazy { ProjectBridge(holder) }
    val chapterBridge: ChapterBridge by lazy { ChapterBridge(holder) }
    val settingsBridge: SettingsBridge by lazy { SettingsBridge(holder) }
    val syncBridge: SyncBridge by lazy { SyncBridge(holder) }
    val statsBridge: StatsBridge by lazy { StatsBridge(holder) }
    val starMapBridge: StarMapBridge by lazy { StarMapBridge(holder) }
    val layoutPolicyBridge: LayoutPolicyBridge by lazy { LayoutPolicyBridge(holder) }
    val secureStorageWarning: String? get() = holder.secureStorageWarning

    companion object {
        private const val TAG = "AppServiceBridge"
    }

    // ── 向后兼容委托 ──
    // 以下方法保持原有签名，委托到对应领域 Bridge。
    // 新代码应直接使用领域 Bridge。

    fun listProjects() = projectBridge.listProjects()
    fun getRecentEdits() = projectBridge.getRecentEdits()
    fun recordRecentEdit(projectId: String, volumeId: String, chapterId: String) = projectBridge.recordRecentEdit(projectId, volumeId, chapterId)
    fun flushRecentEdits() = projectBridge.flushRecentEdits()
    fun validateWorkspace(): BridgeResult<Boolean> = holder.wrapResult { holder.service.validateWorkspace() }
    fun createWorkspaceIfNeeded(): BridgeResult<Boolean> = holder.wrapResult { holder.service.createWorkspaceIfNeeded() }
    fun createProject(title: String) = projectBridge.createProject(title)
    fun getProjectStats(projectId: String) = projectBridge.getProjectStats(projectId)
    fun renameProject(projectId: String, newTitle: String) = projectBridge.renameProject(projectId, newTitle)
    fun deleteProject(projectId: String) = projectBridge.deleteProject(projectId)
    fun reorderProjects(orderedIds: List<String>) = projectBridge.reorderProjects(orderedIds)
    fun listVolumes(projectId: String) = projectBridge.listVolumes(projectId)
    fun createVolume(projectId: String, title: String) = projectBridge.createVolume(projectId, title)
    fun renameVolume(projectId: String, volumeId: String, newTitle: String) = projectBridge.renameVolume(projectId, volumeId, newTitle)
    fun deleteVolume(projectId: String, volumeId: String) = projectBridge.deleteVolume(projectId, volumeId)
    fun reorderVolumes(projectId: String, orderedIds: List<String>) = projectBridge.reorderVolumes(projectId, orderedIds)

    fun listChapters(projectId: String, volumeId: String) = chapterBridge.listChapters(projectId, volumeId)
    fun createChapter(projectId: String, volumeId: String, title: String) = chapterBridge.createChapter(projectId, volumeId, title)
    fun renameChapter(projectId: String, volumeId: String, chapterId: String, newTitle: String) = chapterBridge.renameChapter(projectId, volumeId, chapterId, newTitle)
    fun deleteChapter(projectId: String, volumeId: String, chapterId: String) = chapterBridge.deleteChapter(projectId, volumeId, chapterId)
    fun reorderChapters(projectId: String, volumeId: String, orderedIds: List<String>) = chapterBridge.reorderChapters(projectId, volumeId, orderedIds)
    fun openChapter(projectId: String, volumeId: String, chapterId: String) = chapterBridge.openChapter(projectId, volumeId, chapterId)
    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String) = chapterBridge.saveChapterContent(projectId, volumeId, chapterId, content)
    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String) = chapterBridge.clearChapterContent(projectId, volumeId, chapterId)
    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String) = chapterBridge.updateChapterNote(projectId, volumeId, chapterId, note)
    fun calculateWordCount(text: String) = chapterBridge.calculateWordCount(text)

    fun loadLocalSettings() = settingsBridge.loadLocalSettings()
    fun saveLocalSettings(settings: LocalSettings) = settingsBridge.saveLocalSettings(settings)
    fun loadSyncableSettings() = settingsBridge.loadSyncableSettings()
    fun saveSyncableSettings(settings: SyncableSettings) = settingsBridge.saveSyncableSettings(settings)

    fun loadSyncConfig() = syncBridge.loadSyncConfig()
    fun saveSyncConfig(config: SyncConfig) = syncBridge.saveSyncConfig(config)
    fun loadSyncSecrets() = syncBridge.loadSyncSecrets()
    fun saveSyncSecrets(secrets: SyncSecrets) = syncBridge.saveSyncSecrets(secrets)
    // #592 五/六/#595 十：进程级 override（操作作用域凭据）与按 generation 保存凭据。
    fun setSyncSecretsOverride(secrets: SyncSecrets) = syncBridge.setSyncSecretsOverride(secrets)
    fun clearSyncSecretsOverride() = syncBridge.clearSyncSecretsOverride()
    fun saveSyncSecretsForGeneration(generation: ULong, secrets: SyncSecrets) = syncBridge.saveSyncSecretsForGeneration(generation, secrets)
    fun loadSyncSecretsForGeneration(generation: ULong) = syncBridge.loadSyncSecretsForGeneration(generation)
    fun loadSyncState() = syncBridge.loadSyncState()
    fun getSyncCapability() = syncBridge.getSyncCapability()
    fun performSyncDiagnostics(config: SyncConfig) = syncBridge.performSyncDiagnostics(config)
    fun performSyncDryRun(config: SyncConfig) = syncBridge.performSyncDryRun(config)
    fun performSync(config: SyncConfig, forceSync: Boolean = false) = syncBridge.performSync(config, forceSync)

    fun getWritingStatsSummary(startDate: String, endDate: String) = statsBridge.getWritingStatsSummary(startDate, endDate)
    fun getWritingStatsByProject(startDate: String, endDate: String) = statsBridge.getWritingStatsByProject(startDate, endDate)
    fun getWritingStatsByChapter(startDate: String, endDate: String) = statsBridge.getWritingStatsByChapter(startDate, endDate)
    fun getWritingStatsByDevice(startDate: String, endDate: String) = statsBridge.getWritingStatsByDevice(startDate, endDate)
    fun getWritingSpeedCurve(startDate: String, endDate: String, bucketMinutes: Int) = statsBridge.getWritingSpeedCurve(startDate, endDate, bucketMinutes)
    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, durationSeconds: Int, sessionId: String) = statsBridge.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, durationSeconds, sessionId)
    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, durationSeconds: UInt, sessionId: String) = statsBridge.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, durationSeconds, sessionId)
    fun flushWritingStats() = statsBridge.flushWritingStats()
    fun ensureDeviceInfo(platform: String, deviceClass: String) = settingsBridge.ensureDeviceInfo(platform, deviceClass)

    fun listStarMaps() = starMapBridge.listStarMaps()
    @Suppress("DEPRECATION")
    @Deprecated("Use getStarmapPhasedSnapshot for progressive loading.")
    fun getStarMapGraph(starmapId: String) = starMapBridge.getStarMapGraph(starmapId)
    fun createStarMap(title: String, desc: String) = starMapBridge.createStarMap(title, desc)
    fun addStarMapNode(starmapId: String, node: uniffi.writer_core.StarMapNodeDto, x: Float, y: Float) = starMapBridge.addStarMapNode(starmapId, node, x, y)
    fun saveStarMapLayout(starmapId: String, layout: uniffi.writer_core.StarMapLayoutDto) = starMapBridge.saveStarMapLayout(starmapId, layout)
    fun getStarMapViewport(starmapId: String) = starMapBridge.getStarMapViewport(starmapId)
    fun saveStarMapViewport(starmapId: String, viewport: uniffi.writer_core.StarMapViewportDto) = starMapBridge.saveStarMapViewport(starmapId, viewport)
    fun computeStarMapEdgeRenders(graph: uniffi.writer_core.StarMapGraphDto, layout: uniffi.writer_core.StarMapLayoutDto) = starMapBridge.computeStarMapEdgeRenders(graph, layout)
    fun hitTestStarMapNode(layout: uniffi.writer_core.StarMapLayoutDto, x: Float, y: Float) = starMapBridge.hitTestStarMapNode(layout, x, y)
    fun addStarmapEmbed(starmapId: String, embed: uniffi.writer_core.StarMapEmbedDto) = starMapBridge.addStarmapEmbed(starmapId, embed)
    fun updateStarmapEmbed(starmapId: String, instanceId: String, patch: uniffi.writer_core.StarMapEmbedPatchInputDto) = starMapBridge.updateStarmapEmbed(starmapId, instanceId, patch)
    fun deleteStarmapEmbed(starmapId: String, instanceId: String) = starMapBridge.deleteStarmapEmbed(starmapId, instanceId)
    fun addStarmapLink(starmapId: String, link: uniffi.writer_core.StarMapLinkDto) = starMapBridge.addStarmapLink(starmapId, link)
    fun updateStarmapLink(starmapId: String, linkId: String, patch: uniffi.writer_core.StarMapLinkPatchInputDto) = starMapBridge.updateStarmapLink(starmapId, linkId, patch)
    fun deleteStarmapLink(starmapId: String, linkId: String) = starMapBridge.deleteStarmapLink(starmapId, linkId)
    fun findStarmapReferences(targetStarmapId: String) = starMapBridge.findStarmapReferences(targetStarmapId)
    fun getStarMapMotionPolicy() = starMapBridge.getStarMapMotionPolicy()

    fun listRegisteredActions(): BridgeResult<List<ActionDescriptorDto>> = holder.wrapResult {
        holder.service.listRegisteredActions()
    }

    fun executeAction(actionId: String, argsJson: String, contextJson: String): BridgeResult<ActionResultDto> = holder.wrapResult {
        holder.service.executeAction(actionId, argsJson, contextJson)
    }

    fun aiAvailable(): Boolean = try {
        holder.service.aiAvailable()
    } catch (e: UnsatisfiedLinkError) {
        false
    }

    fun resolveLayout(metrics: uniffi.writer_core.WindowMetricsDto) = layoutPolicyBridge.resolveLayout(metrics)
    fun resolveScreenPolicy(screenRole: ScreenRoleDto, shellMode: ShellModeDto) = layoutPolicyBridge.resolveScreenPolicy(screenRole, shellMode)

    fun editorKernelInsert(byteOffset: UInt, text: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelInsert(byteOffset, text, cause, expectedRevision)
    }

    fun editorKernelDelete(byteStart: UInt, byteEndExclusive: UInt, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelDelete(byteStart, byteEndExclusive, cause, expectedRevision)
    }

    fun editorKernelReplace(byteStart: UInt, byteEndExclusive: UInt, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelReplace(byteStart, byteEndExclusive, replacementText, originalText, cause, expectedRevision)
    }

    fun editorKernelSetSelection(anchorByteOffset: UInt, headByteOffset: UInt, expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelSetSelection(anchorByteOffset, headByteOffset, expectedRevision)
    }

    fun editorKernelUndo(expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelUndo(expectedRevision)
    }

    fun editorKernelRedo(expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelRedo(expectedRevision)
    }

    fun editorKernelLoadText(text: String, cursorByteOffset: UInt): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelLoadText(text, cursorByteOffset)
    }

    fun editorKernelSetAnimationEnabled(enabled: Boolean): BridgeResult<Unit> = holder.wrapResult {
        holder.service.editorKernelSetAnimationEnabled(if (enabled) 1u else 0u)
    }

    fun editorKernelSetAnimationDurationMs(durationMs: ULong): BridgeResult<Unit> = holder.wrapResult {
        holder.service.editorKernelSetAnimationDurationMs(durationMs)
    }

    fun editorKernelGetText(): BridgeResult<String> = holder.wrapResult {
        holder.service.editorKernelGetText()
    }

    fun editorKernelGetRevision(): BridgeResult<ULong> = holder.wrapResult {
        holder.service.editorKernelGetRevision()
    }

    fun editorKernelGetCursor(): BridgeResult<UInt> = holder.wrapResult {
        holder.service.editorKernelGetCursor()
    }

    fun editorKernelCompositionUpdateVisualIntent(
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String
    ): BridgeResult<uniffi.writer_core.EditorVisualIntentDto> = holder.wrapResult {
        holder.service.editorKernelCompositionUpdateVisualIntent(
            compositionReplaceStart, compositionReplaceEndExclusive,
            oldPreeditText, newPreeditText
        )
    }

    fun editorKernelSessionSnapshot(): BridgeResult<uniffi.writer_core.EditorSessionSnapshotDto> = holder.wrapResult {
        holder.service.editorKernelSessionSnapshot()
    }

    fun editorKernelReplaceAll(
        search: String,
        replacement: String,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelReplaceAll(search, replacement, expectedRevision)
    }

    fun editorKernelInsertLineBreak(
        byteOffset: UInt,
        autoIndentPrefix: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelInsertLineBreak(byteOffset, autoIndentPrefix, cause, expectedRevision)
    }

    fun editorKernelCommitText(
        byteStart: UInt,
        byteEndExclusive: UInt,
        replacementText: String,
        resultingSelectionAnchor: UInt,
        resultingSelectionHead: UInt,
        compositionSessionId: ULong,
        compositionBaseRevision: ULong,
        compositionGeneration: ULong,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelCommitText(
            byteStart, byteEndExclusive, replacementText,
            resultingSelectionAnchor, resultingSelectionHead,
            compositionSessionId, compositionBaseRevision, compositionGeneration,
            cause, expectedRevision
        )
    }

    fun editorKernelDeleteSurrounding(
        beforeByteStart: UInt,
        beforeByteEndExclusive: UInt,
        afterByteStart: UInt,
        afterByteEndExclusive: UInt,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelDeleteSurrounding(
            beforeByteStart, beforeByteEndExclusive,
            afterByteStart, afterByteEndExclusive,
            cause, expectedRevision
        )
    }

    fun editorKernelBeginComposition(
        replaceStart: UInt,
        replaceEndExclusive: UInt,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelBeginComposition(replaceStart, replaceEndExclusive, expectedRevision)
    }

    fun editorKernelUpdateComposition(
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        newPreeditText: String,
        newPreeditCursorOffset: UInt,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelUpdateComposition(
            compositionSessionId, compositionGeneration,
            newPreeditText, newPreeditCursorOffset,
            expectedRevision
        )
    }

    fun editorKernelFinishComposition(
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelFinishComposition(compositionSessionId, compositionGeneration, expectedRevision)
    }

    fun editorKernelCancelComposition(
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.editorKernelCancelComposition(compositionSessionId, compositionGeneration, expectedRevision)
    }

    // ── #541: Text Edit Session API ──

    fun textEditSessionCreate(
        targetId: String,
        initialText: String,
        initialCursorByteOffset: UInt,
        isPersistent: Boolean
    ): BridgeResult<ULong?> = holder.wrapResult {
        holder.service.textEditSessionCreate(targetId, initialText, initialCursorByteOffset, if (isPersistent) 1u else 0u)
    }

    fun textEditSessionClose(sessionId: ULong): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.textEditSessionClose(sessionId) != 0u.toUByte()
    }

    fun textEditSessionReset(sessionId: ULong, text: String, cursorByteOffset: UInt): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.textEditSessionReset(sessionId, text, cursorByteOffset) != 0u.toUByte()
    }

    fun textEditSessionInsert(
        sessionId: ULong,
        byteOffset: UInt,
        text: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionInsert(sessionId, byteOffset, text, cause, expectedRevision)
    }

    fun textEditSessionDelete(
        sessionId: ULong,
        byteStart: UInt,
        byteEndExclusive: UInt,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionDelete(sessionId, byteStart, byteEndExclusive, cause, expectedRevision)
    }

    fun textEditSessionReplace(
        sessionId: ULong,
        byteStart: UInt,
        byteEndExclusive: UInt,
        replacementText: String,
        originalText: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionReplace(sessionId, byteStart, byteEndExclusive, replacementText, originalText, cause, expectedRevision)
    }

    fun textEditSessionSetSelection(
        sessionId: ULong,
        anchorByteOffset: UInt,
        headByteOffset: UInt,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionSetSelection(sessionId, anchorByteOffset, headByteOffset, expectedRevision)
    }

    fun textEditSessionUndo(sessionId: ULong, expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionUndo(sessionId, expectedRevision)
    }

    fun textEditSessionRedo(sessionId: ULong, expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionRedo(sessionId, expectedRevision)
    }

    fun textEditSessionLoadText(sessionId: ULong, text: String, cursorByteOffset: UInt): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionLoadText(sessionId, text, cursorByteOffset)
    }

    fun textEditSessionCompositionUpdateVisualIntent(
        sessionId: ULong,
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String
    ): BridgeResult<uniffi.writer_core.EditorVisualIntentDto> = holder.wrapResult {
        holder.service.textEditSessionCompositionUpdateVisualIntent(sessionId, compositionReplaceStart, compositionReplaceEndExclusive, oldPreeditText, newPreeditText)
    }

    fun textEditSessionCommitText(
        sessionId: ULong,
        byteStart: UInt,
        byteEndExclusive: UInt,
        replacementText: String,
        resultingSelectionAnchor: UInt,
        resultingSelectionHead: UInt,
        compositionSessionId: ULong,
        compositionBaseRevision: ULong,
        compositionGeneration: ULong,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionCommitText(
            sessionId, byteStart, byteEndExclusive, replacementText,
            resultingSelectionAnchor, resultingSelectionHead,
            compositionSessionId, compositionBaseRevision, compositionGeneration,
            cause, expectedRevision
        )
    }

    fun textEditSessionDeleteSurrounding(
        sessionId: ULong,
        beforeByteStart: UInt,
        beforeByteEndExclusive: UInt,
        afterByteStart: UInt,
        afterByteEndExclusive: UInt,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionDeleteSurrounding(sessionId, beforeByteStart, beforeByteEndExclusive, afterByteStart, afterByteEndExclusive, cause, expectedRevision)
    }

    fun textEditSessionBeginComposition(
        sessionId: ULong,
        replaceStart: UInt,
        replaceEndExclusive: UInt,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionBeginComposition(sessionId, replaceStart, replaceEndExclusive, expectedRevision)
    }

    fun textEditSessionUpdateComposition(
        sessionId: ULong,
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        newPreeditText: String,
        newPreeditCursorOffset: UInt,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionUpdateComposition(sessionId, compositionSessionId, compositionGeneration, newPreeditText, newPreeditCursorOffset, expectedRevision)
    }

    fun textEditSessionFinishComposition(
        sessionId: ULong,
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionFinishComposition(sessionId, compositionSessionId, compositionGeneration, expectedRevision)
    }

    fun textEditSessionCancelComposition(
        sessionId: ULong,
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionCancelComposition(sessionId, compositionSessionId, compositionGeneration, expectedRevision)
    }

    fun textEditSessionSetAnimationEnabled(sessionId: ULong, enabled: Boolean): BridgeResult<Unit> = holder.wrapResult {
        holder.service.textEditSessionSetAnimationEnabled(sessionId, if (enabled) 1u else 0u)
    }

    fun textEditSessionSetAnimationDurationMs(sessionId: ULong, durationMs: ULong): BridgeResult<Unit> = holder.wrapResult {
        holder.service.textEditSessionSetAnimationDurationMs(sessionId, durationMs)
    }

    fun textEditSessionGetText(sessionId: ULong): BridgeResult<String> = holder.wrapResult {
        holder.service.textEditSessionGetText(sessionId)
    }

    fun textEditSessionGetRevision(sessionId: ULong): BridgeResult<ULong> = holder.wrapResult {
        holder.service.textEditSessionGetRevision(sessionId)
    }

    fun textEditSessionSnapshot(sessionId: ULong): BridgeResult<uniffi.writer_core.EditorSessionSnapshotDto> = holder.wrapResult {
        holder.service.textEditSessionSnapshot(sessionId)
    }

    fun textEditSessionReplaceAll(
        sessionId: ULong,
        search: String,
        replacement: String,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionReplaceAll(sessionId, search, replacement, expectedRevision)
    }

    fun textEditSessionInsertLineBreak(
        sessionId: ULong,
        byteOffset: UInt,
        autoIndentPrefix: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> = holder.wrapResult {
        holder.service.textEditSessionInsertLineBreak(sessionId, byteOffset, autoIndentPrefix, cause, expectedRevision)
    }
}
