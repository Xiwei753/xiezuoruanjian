package com.xiwei.sujian.core.interop.app
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.project.ChapterBridge
import com.xiwei.sujian.core.interop.project.ProjectBridge
import com.xiwei.sujian.core.interop.project.RecentEditsBridge
import com.xiwei.sujian.core.interop.settings.SettingsBridge
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.settings.data.model.SyncableSettings
import com.xiwei.sujian.feature.starmap.data.interop.StarMapBridge
import com.xiwei.sujian.feature.stats.data.interop.StatsBridge
import com.xiwei.sujian.feature.sync.data.interop.SyncBridge
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncState
import uniffi.writer_core.ScreenRoleDto

/**
 * AppServiceBridge — 门面类（向后兼容）。
 *
 * 内部已拆分为领域 Bridge（ProjectBridge、ChapterBridge、SettingsBridge 等），
 * 原有公开 API 全部委托到对应领域 Bridge，行为不变。
 *
 * 新代码应直接使用领域 Bridge，不再依赖此门面类。
 */
class AppServiceBridge(val holder: WriterAppServiceHolder) {
    // ── 领域 Bridge ──
    val projectBridge: ProjectBridge by lazy { ProjectBridge(holder) }
    val recentEditsBridge: RecentEditsBridge by lazy { RecentEditsBridge(holder) }
    val chapterBridge: ChapterBridge by lazy { ChapterBridge(holder) }
    val settingsBridge: SettingsBridge by lazy { SettingsBridge(holder) }
    val syncBridge: SyncBridge by lazy { SyncBridge(holder) }
    val statsBridge: StatsBridge by lazy { StatsBridge(holder) }
    val starMapBridge: StarMapBridge by lazy { StarMapBridge(holder) }
    val secureStorageWarning: String? get() = holder.secureStorageWarning

    companion object {
        private const val TAG = "AppServiceBridge"
    }

    // ── 向后兼容委托 ──
    // 以下方法保持原有签名，委托到对应领域 Bridge。
    // 新代码应直接使用领域 Bridge。

    fun listProjects() = projectBridge.listProjects()

    fun getRecentEdits() = recentEditsBridge.getRecentEdits()

    fun recordRecentEdit(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) = recentEditsBridge.recordRecentEdit(
        projectId,
        volumeId,
        chapterId,
    )

    fun flushRecentEdits() = recentEditsBridge.flushRecentEdits()

    fun createProject(title: String) = projectBridge.createProject(title)

    fun getProjectStats(projectId: String) = projectBridge.getProjectStats(projectId)

    fun renameProject(
        projectId: String,
        newTitle: String,
    ) = projectBridge.renameProject(projectId, newTitle)

    fun deleteProject(projectId: String) = projectBridge.deleteProject(projectId)

    fun reorderProjects(orderedIds: List<String>) = projectBridge.reorderProjects(orderedIds)

    fun listVolumes(projectId: String) = projectBridge.listVolumes(projectId)

    fun createVolume(
        projectId: String,
        title: String,
    ) = projectBridge.createVolume(projectId, title)

    fun renameVolume(
        projectId: String,
        volumeId: String,
        newTitle: String,
    ) = projectBridge.renameVolume(
        projectId,
        volumeId,
        newTitle,
    )

    fun deleteVolume(
        projectId: String,
        volumeId: String,
    ) = projectBridge.deleteVolume(projectId, volumeId)

    fun reorderVolumes(
        projectId: String,
        orderedIds: List<String>,
    ) = projectBridge.reorderVolumes(
        projectId,
        orderedIds,
    )

    fun listChapters(
        projectId: String,
        volumeId: String,
    ) = chapterBridge.listChapters(projectId, volumeId)

    fun createChapter(
        projectId: String,
        volumeId: String,
        title: String,
    ) = chapterBridge.createChapter(
        projectId,
        volumeId,
        title,
    )

    fun renameChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
        newTitle: String,
    ) = chapterBridge.renameChapter(
        projectId,
        volumeId,
        chapterId,
        newTitle,
    )

    fun deleteChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) = chapterBridge.deleteChapter(
        projectId,
        volumeId,
        chapterId,
    )

    fun reorderChapters(
        projectId: String,
        volumeId: String,
        orderedIds: List<String>,
    ) = chapterBridge.reorderChapters(
        projectId,
        volumeId,
        orderedIds,
    )

    fun openChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) = chapterBridge.openChapter(
        projectId,
        volumeId,
        chapterId,
    )

    fun saveChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        content: String,
    ) = chapterBridge.saveChapterContent(
        projectId,
        volumeId,
        chapterId,
        content,
    )

    fun clearChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) = chapterBridge.clearChapterContent(
        projectId,
        volumeId,
        chapterId,
    )

    fun updateChapterNote(
        projectId: String,
        volumeId: String,
        chapterId: String,
        note: String,
    ) = chapterBridge.updateChapterNote(
        projectId,
        volumeId,
        chapterId,
        note,
    )

    fun calculateWordCount(text: String) = chapterBridge.calculateWordCount(text)

    fun loadLocalSettings() = settingsBridge.loadLocalSettings()

    fun saveLocalSettings(settings: LocalSettings) = settingsBridge.saveLocalSettings(settings)

    fun loadSyncableSettings() = settingsBridge.loadSyncableSettings()

    fun saveSyncableSettings(settings: SyncableSettings) = settingsBridge.saveSyncableSettings(settings)

    // #600 评论 #3 问题二：作品级 sync 入口全部按 projectId 路由。
    fun loadSyncConfig(projectId: String) = syncBridge.loadSyncConfig(projectId)

    fun saveSyncConfig(
        projectId: String,
        config: SyncConfig,
    ) = syncBridge.saveSyncConfig(projectId, config)

    fun loadSyncSecrets(projectId: String) = syncBridge.loadSyncSecrets(projectId)

    fun saveSyncSecrets(
        projectId: String,
        secrets: SyncSecrets,
    ) = syncBridge.saveSyncSecrets(projectId, secrets)

    // #592 五/六/#595 十：进程级 override（操作作用域凭据）与按 generation 保存凭据。
    // override 是进程级，不按作品区分。
    fun setSyncSecretsOverride(secrets: SyncSecrets) = syncBridge.setSyncSecretsOverride(secrets)

    fun clearSyncSecretsOverride() = syncBridge.clearSyncSecretsOverride()

    fun saveSyncSecretsForGeneration(
        projectId: String,
        generation: ULong,
        secrets: SyncSecrets,
    ) = syncBridge.saveSyncSecretsForGeneration(
        projectId,
        generation,
        secrets,
    )

    fun loadSyncSecretsForGeneration(
        projectId: String,
        generation: ULong,
    ) = syncBridge.loadSyncSecretsForGeneration(projectId, generation)

    /** #595 五：删除指定 generation 的安全存储凭据（旧版本清理）。 */
    fun deleteSyncSecretsForGeneration(
        projectId: String,
        generation: ULong,
    ) = syncBridge.deleteSyncSecretsForGeneration(projectId, generation)

    fun loadSyncState(projectId: String) = syncBridge.loadSyncState(projectId)

    fun getSyncCapability(projectId: String) = syncBridge.getSyncCapability(projectId)

    fun performSyncDiagnostics(
        projectId: String,
        config: SyncConfig,
    ) = syncBridge.performSyncDiagnostics(projectId, config)

    fun performSyncDryRun(
        projectId: String,
        config: SyncConfig,
    ) = syncBridge.performSyncDryRun(projectId, config)

    fun performSync(
        projectId: String,
        config: SyncConfig,
        forceSync: Boolean = false,
    ) = syncBridge.performSync(projectId, config, forceSync)

    // #600 评论 #3 问题四：应用级同步通道（设置/全局星图/主题调色板）。
    fun loadAppSyncConfig() = syncBridge.loadAppSyncConfig()

    fun saveAppSyncConfig(config: SyncConfig) = syncBridge.saveAppSyncConfig(config)

    fun loadAppSyncSecrets() = syncBridge.loadAppSyncSecrets()

    fun saveAppSyncSecrets(secrets: SyncSecrets) = syncBridge.saveAppSyncSecrets(secrets)

    fun saveAppSyncSecretsForGeneration(
        generation: ULong,
        secrets: SyncSecrets,
    ) = syncBridge.saveAppSyncSecretsForGeneration(generation, secrets)

    fun loadAppSyncSecretsForGeneration(generation: ULong) = syncBridge.loadAppSyncSecretsForGeneration(generation)

    fun deleteAppSyncSecretsForGeneration(generation: ULong) = syncBridge.deleteAppSyncSecretsForGeneration(generation)

    fun performAppSyncDiagnostics(config: SyncConfig) = syncBridge.performAppSyncDiagnostics(config)

    fun performAppSyncDryRun(config: SyncConfig) = syncBridge.performAppSyncDryRun(config)

    fun performAppSync(
        config: SyncConfig,
        forceSync: Boolean = false,
    ) = syncBridge.performAppSync(config, forceSync)

    fun loadAppSyncState() = syncBridge.loadAppSyncState()

    fun saveAppSyncState(state: SyncState) = syncBridge.saveAppSyncState(state)

    fun getWritingStatsSummary(
        startDate: String,
        endDate: String,
    ) = statsBridge.getWritingStatsSummary(
        startDate,
        endDate,
    )

    fun getWritingStatsByProject(
        startDate: String,
        endDate: String,
    ) = statsBridge.getWritingStatsByProject(
        startDate,
        endDate,
    )

    fun getWritingStatsByChapter(
        startDate: String,
        endDate: String,
    ) = statsBridge.getWritingStatsByChapter(
        startDate,
        endDate,
    )

    fun getWritingStatsByDevice(
        startDate: String,
        endDate: String,
    ) = statsBridge.getWritingStatsByDevice(
        startDate,
        endDate,
    )

    fun getWritingSpeedCurve(
        startDate: String,
        endDate: String,
        bucketMinutes: Int,
    ) = statsBridge.getWritingSpeedCurve(
        startDate,
        endDate,
        bucketMinutes,
    )

    fun recordWritingEvent(
        deviceId: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        source: String,
        insertedChars: Int,
        deletedChars: Int,
        pastedChars: Int,
        aiInsertedChars: Int,
        durationSeconds: Int,
        sessionId: String,
    ) = statsBridge.recordWritingEvent(
        deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars,
        pastedChars, aiInsertedChars, durationSeconds, sessionId,
    )

    fun processWritingEvent(
        deviceId: String,
        platform: String,
        projectId: String,
        volumeId: String,
        chapterId: String,
        oldText: String,
        newText: String,
        durationSeconds: UInt,
        sessionId: String,
    ) = statsBridge.processWritingEvent(
        deviceId, platform, projectId, volumeId, chapterId, oldText, newText,
        durationSeconds, sessionId,
    )

    fun flushWritingStats() = statsBridge.flushWritingStats()

    fun ensureDeviceInfo(
        platform: String,
        deviceClass: String,
    ) = settingsBridge.ensureDeviceInfo(platform, deviceClass)

    fun listStarMaps() = starMapBridge.listStarMaps()

    @Suppress("DEPRECATION")
    @Deprecated("Use getStarmapPhasedSnapshot for progressive loading.")
    fun getStarMapGraph(starmapId: String) = starMapBridge.getStarMapGraph(starmapId)

    fun createStarMap(
        title: String,
        desc: String,
    ) = starMapBridge.createStarMap(title, desc)

    fun addStarMapNode(
        starmapId: String,
        node: uniffi.writer_core.StarMapNodeDto,
        x: Float,
        y: Float,
    ) = starMapBridge.addStarMapNode(
        starmapId,
        node,
        x,
        y,
    )

    fun saveStarMapLayout(
        starmapId: String,
        layout: uniffi.writer_core.StarMapLayoutDto,
    ) = starMapBridge.saveStarMapLayout(
        starmapId,
        layout,
    )

    fun getStarMapViewport(starmapId: String) = starMapBridge.getStarMapViewport(starmapId)

    fun saveStarMapViewport(
        starmapId: String,
        viewport: uniffi.writer_core.StarMapViewportDto,
    ) = starMapBridge.saveStarMapViewport(
        starmapId,
        viewport,
    )

    fun computeStarMapEdgeRenders(
        graph: uniffi.writer_core.StarMapGraphDto,
        layout: uniffi.writer_core.StarMapLayoutDto,
    ) = starMapBridge.computeStarMapEdgeRenders(
        graph,
        layout,
    )

    fun hitTestStarMapNode(
        layout: uniffi.writer_core.StarMapLayoutDto,
        x: Float,
        y: Float,
    ) = starMapBridge.hitTestStarMapNode(
        layout,
        x,
        y,
    )

    fun addStarmapEmbed(
        starmapId: String,
        embed: uniffi.writer_core.StarMapEmbedDto,
    ) = starMapBridge.addStarmapEmbed(
        starmapId,
        embed,
    )

    fun updateStarmapEmbed(
        starmapId: String,
        instanceId: String,
        patch: uniffi.writer_core.StarMapEmbedPatchInputDto,
    ) = starMapBridge.updateStarmapEmbed(
        starmapId,
        instanceId,
        patch,
    )

    fun deleteStarmapEmbed(
        starmapId: String,
        instanceId: String,
    ) = starMapBridge.deleteStarmapEmbed(
        starmapId,
        instanceId,
    )

    fun addStarmapLink(
        starmapId: String,
        link: uniffi.writer_core.StarMapLinkDto,
    ) = starMapBridge.addStarmapLink(
        starmapId,
        link,
    )

    fun updateStarmapLink(
        starmapId: String,
        linkId: String,
        patch: uniffi.writer_core.StarMapLinkPatchInputDto,
    ) = starMapBridge.updateStarmapLink(
        starmapId,
        linkId,
        patch,
    )

    fun deleteStarmapLink(
        starmapId: String,
        linkId: String,
    ) = starMapBridge.deleteStarmapLink(starmapId, linkId)

    fun findStarmapReferences(targetStarmapId: String) = starMapBridge.findStarmapReferences(targetStarmapId)

    fun getStarMapMotionPolicy() = starMapBridge.getStarMapMotionPolicy()

    fun aiAvailable(): Boolean =
        try {
            holder.service.aiAvailable()
        } catch (e: UnsatisfiedLinkError) {
            false
        }

    fun resolveLayout(
        capabilities: uniffi.writer_core.WindowCapabilitiesDto,
    ): BridgeResult<uniffi.writer_core.LayoutContractDto> =
        holder.wrapResult {
            holder.service.resolveLayout(capabilities)
        }

    fun resolveScreenPolicy(screenRole: ScreenRoleDto): BridgeResult<uniffi.writer_core.ScreenPolicyDto> =
        holder.wrapResult {
            holder.service.resolveScreenPolicy(screenRole)
        }

    fun editorKernelInsert(
        byteOffset: UInt,
        text: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelInsert(byteOffset, text, cause, expectedRevision)
        }

    fun editorKernelDelete(
        byteStart: UInt,
        byteEndExclusive: UInt,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelDelete(byteStart, byteEndExclusive, cause, expectedRevision)
        }

    fun editorKernelReplace(
        byteStart: UInt,
        byteEndExclusive: UInt,
        replacementText: String,
        originalText: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelReplace(
                byteStart,
                byteEndExclusive,
                replacementText,
                originalText,
                cause,
                expectedRevision,
            )
        }

    fun editorKernelSetSelection(
        anchorByteOffset: UInt,
        headByteOffset: UInt,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelSetSelection(anchorByteOffset, headByteOffset, expectedRevision)
        }

    fun editorKernelUndo(expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelUndo(expectedRevision)
        }

    fun editorKernelRedo(expectedRevision: ULong): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelRedo(expectedRevision)
        }

    fun editorKernelLoadText(
        text: String,
        cursorByteOffset: UInt,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelLoadText(text, cursorByteOffset)
        }

    fun editorKernelSetAnimationEnabled(enabled: Boolean): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.editorKernelSetAnimationEnabled(if (enabled) 1u else 0u)
        }

    fun editorKernelSetAnimationDurationMs(durationMs: ULong): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.editorKernelSetAnimationDurationMs(durationMs)
        }

    fun editorKernelGetText(): BridgeResult<String> =
        holder.wrapResult {
            holder.service.editorKernelGetText()
        }

    fun editorKernelGetRevision(): BridgeResult<ULong> =
        holder.wrapResult {
            holder.service.editorKernelGetRevision()
        }

    fun editorKernelGetCursor(): BridgeResult<UInt> =
        holder.wrapResult {
            holder.service.editorKernelGetCursor()
        }

    fun editorKernelSessionSnapshot(): BridgeResult<uniffi.writer_core.EditorSessionSnapshotDto> =
        holder.wrapResult {
            holder.service.editorKernelSessionSnapshot()
        }

    fun editorKernelReplaceAll(
        search: String,
        replacement: String,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelReplaceAll(search, replacement, expectedRevision)
        }

    fun editorKernelInsertLineBreak(
        byteOffset: UInt,
        autoIndentEnabled: UByte,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelInsertLineBreak(byteOffset, autoIndentEnabled, cause, expectedRevision)
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
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelCommitText(
                byteStart, byteEndExclusive, replacementText,
                resultingSelectionAnchor, resultingSelectionHead,
                compositionSessionId, compositionBaseRevision, compositionGeneration,
                cause, expectedRevision,
            )
        }

    fun editorKernelDeleteSurrounding(
        beforeByteStart: UInt,
        beforeByteEndExclusive: UInt,
        afterByteStart: UInt,
        afterByteEndExclusive: UInt,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelDeleteSurrounding(
                beforeByteStart,
                beforeByteEndExclusive,
                afterByteStart,
                afterByteEndExclusive,
                cause,
                expectedRevision,
            )
        }

    fun editorKernelBeginComposition(
        replaceStart: UInt,
        replaceEndExclusive: UInt,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelBeginComposition(replaceStart, replaceEndExclusive, expectedRevision)
        }

    fun editorKernelUpdateComposition(
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        newPreeditText: String,
        newPreeditCursorOffset: UInt,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelUpdateComposition(
                compositionSessionId,
                compositionGeneration,
                newPreeditText,
                newPreeditCursorOffset,
                expectedRevision,
            )
        }

    fun editorKernelFinishComposition(
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelFinishComposition(compositionSessionId, compositionGeneration, expectedRevision)
        }

    fun editorKernelCancelComposition(
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.editorKernelCancelComposition(compositionSessionId, compositionGeneration, expectedRevision)
        }

    // ── #541: Text Edit Session API ──

    fun textEditSessionCreate(
        targetId: String,
        initialText: String,
        initialCursorByteOffset: UInt,
        isPersistent: Boolean,
    ): BridgeResult<ULong?> =
        holder.wrapResult {
            holder.service.textEditSessionCreate(
                targetId,
                initialText,
                initialCursorByteOffset,
                if (isPersistent) 1u else 0u,
            )
        }

    fun textEditSessionClose(sessionId: ULong): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.textEditSessionClose(sessionId) != 0u.toUByte()
        }

    fun textEditSessionReset(
        sessionId: ULong,
        text: String,
        cursorByteOffset: UInt,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.textEditSessionReset(sessionId, text, cursorByteOffset) != 0u.toUByte()
        }

    fun textEditSessionInsert(
        sessionId: ULong,
        byteOffset: UInt,
        text: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionInsert(sessionId, byteOffset, text, cause, expectedRevision)
        }

    fun textEditSessionDelete(
        sessionId: ULong,
        byteStart: UInt,
        byteEndExclusive: UInt,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionDelete(sessionId, byteStart, byteEndExclusive, cause, expectedRevision)
        }

    fun textEditSessionReplace(
        sessionId: ULong,
        byteStart: UInt,
        byteEndExclusive: UInt,
        replacementText: String,
        originalText: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionReplace(
                sessionId,
                byteStart,
                byteEndExclusive,
                replacementText,
                originalText,
                cause,
                expectedRevision,
            )
        }

    fun textEditSessionSetSelection(
        sessionId: ULong,
        anchorByteOffset: UInt,
        headByteOffset: UInt,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionSetSelection(sessionId, anchorByteOffset, headByteOffset, expectedRevision)
        }

    fun textEditSessionUndo(
        sessionId: ULong,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionUndo(sessionId, expectedRevision)
        }

    fun textEditSessionRedo(
        sessionId: ULong,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionRedo(sessionId, expectedRevision)
        }

    fun textEditSessionLoadText(
        sessionId: ULong,
        text: String,
        cursorByteOffset: UInt,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionLoadText(sessionId, text, cursorByteOffset)
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
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionCommitText(
                sessionId, byteStart, byteEndExclusive, replacementText,
                resultingSelectionAnchor, resultingSelectionHead,
                compositionSessionId, compositionBaseRevision, compositionGeneration,
                cause, expectedRevision,
            )
        }

    fun textEditSessionDeleteSurrounding(
        sessionId: ULong,
        beforeByteStart: UInt,
        beforeByteEndExclusive: UInt,
        afterByteStart: UInt,
        afterByteEndExclusive: UInt,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionDeleteSurrounding(
                sessionId,
                beforeByteStart,
                beforeByteEndExclusive,
                afterByteStart,
                afterByteEndExclusive,
                cause,
                expectedRevision,
            )
        }

    fun textEditSessionBeginComposition(
        sessionId: ULong,
        replaceStart: UInt,
        replaceEndExclusive: UInt,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionBeginComposition(
                sessionId,
                replaceStart,
                replaceEndExclusive,
                expectedRevision,
            )
        }

    fun textEditSessionUpdateComposition(
        sessionId: ULong,
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        newPreeditText: String,
        newPreeditCursorOffset: UInt,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionUpdateComposition(
                sessionId,
                compositionSessionId,
                compositionGeneration,
                newPreeditText,
                newPreeditCursorOffset,
                expectedRevision,
            )
        }

    fun textEditSessionFinishComposition(
        sessionId: ULong,
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionFinishComposition(
                sessionId,
                compositionSessionId,
                compositionGeneration,
                expectedRevision,
            )
        }

    fun textEditSessionCancelComposition(
        sessionId: ULong,
        compositionSessionId: ULong,
        compositionGeneration: ULong,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionCancelComposition(
                sessionId,
                compositionSessionId,
                compositionGeneration,
                expectedRevision,
            )
        }

    fun textEditSessionSetAnimationEnabled(
        sessionId: ULong,
        enabled: Boolean,
    ): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.textEditSessionSetAnimationEnabled(sessionId, if (enabled) 1u else 0u)
        }

    fun textEditSessionSetAnimationDurationMs(
        sessionId: ULong,
        durationMs: ULong,
    ): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.textEditSessionSetAnimationDurationMs(sessionId, durationMs)
        }

    // #606: session-scoped grapheme 边界由 Core 唯一计算（unicode_segmentation）。
    fun textEditSessionPreviousGraphemeBoundary(
        sessionId: ULong,
        byteOffset: UInt,
    ): BridgeResult<UInt> =
        holder.wrapResult {
            holder.service.textEditSessionPreviousGraphemeBoundary(sessionId, byteOffset)
        }

    fun textEditSessionNextGraphemeBoundary(
        sessionId: ULong,
        byteOffset: UInt,
    ): BridgeResult<UInt> =
        holder.wrapResult {
            holder.service.textEditSessionNextGraphemeBoundary(sessionId, byteOffset)
        }

    /**
     * #606: 旧→新逻辑 slice 对应关系由 Core 唯一计算（session-scoped）。
     * 平台端 RebasePlanner 不再自己匹配，直接消费此结果。
     */
    fun textEditSessionComputeRebaseSliceMappings(
        sessionId: ULong,
        oldSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
        oldSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
        newSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
        newSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
        offsetMap: uniffi.writer_core.OffsetMapDto?,
    ): BridgeResult<List<uniffi.writer_core.RebaseSliceMappingDto>> =
        holder.wrapResult {
            holder.service.textEditSessionComputeRebaseSliceMappings(
                sessionId,
                oldSliceRoles,
                oldSliceByteRanges,
                newSliceRoles,
                newSliceByteRanges,
                offsetMap,
            )
        }

    fun textEditSessionGetText(sessionId: ULong): BridgeResult<String> =
        holder.wrapResult {
            holder.service.textEditSessionGetText(sessionId)
        }

    fun textEditSessionGetRevision(sessionId: ULong): BridgeResult<ULong> =
        holder.wrapResult {
            holder.service.textEditSessionGetRevision(sessionId)
        }

    fun textEditSessionSnapshot(sessionId: ULong): BridgeResult<uniffi.writer_core.EditorSessionSnapshotDto> =
        holder.wrapResult {
            holder.service.textEditSessionSnapshot(sessionId)
        }

    fun textEditSessionReplaceAll(
        sessionId: ULong,
        search: String,
        replacement: String,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionReplaceAll(sessionId, search, replacement, expectedRevision)
        }

    fun textEditSessionInsertLineBreak(
        sessionId: ULong,
        byteOffset: UInt,
        autoIndentEnabled: UByte,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: ULong,
    ): BridgeResult<uniffi.writer_core.EditorEditResultDto> =
        holder.wrapResult {
            holder.service.textEditSessionInsertLineBreak(
                sessionId,
                byteOffset,
                autoIndentEnabled,
                cause,
                expectedRevision,
            )
        }
}
