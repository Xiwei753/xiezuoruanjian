package com.xiwei.sujian.ui

// ! # 编辑器章节切换操作（从 EditorViewModel 拆分）
// !
// ! 章节打开事务（latest-wins + 提交前 session 预准备）、章节加载、
// ! 同步合并检查、外部内容应用。

import android.app.Application
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.coordinator.DocumentFactOrigin
import com.xiwei.sujian.editor.v2.coordinator.DocumentVersion
import com.xiwei.sujian.editor.v2.coordinator.TargetDocumentFact
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.editor.v2.coordinator.commitPreparedSession
import com.xiwei.sujian.editor.v2.coordinator.documentCommittedVersionFor
import com.xiwei.sujian.editor.v2.coordinator.prepareTargetSessionForCommit
import com.xiwei.sujian.editor.v2.coordinator.releasePreparedTarget
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/**
 * 判断会话是否匹配指定章节三元组 — 拆分复杂条件，避免 ComplexCondition 抑制。
 */
private fun EditorSession.matchesChapter(
    projectId: String,
    volumeId: String,
    chapterId: String,
): Boolean = this.projectId == projectId && this.volumeId == volumeId && this.chapterId == chapterId

/**
 * 同步合并是否需要应用 — 拆分复杂条件，避免 ComplexCondition 抑制。
 */
private fun EditorViewModel.isSyncMergeApplicable(
    hash: String,
    currentHash: String,
    content: String,
    currentContent: String,
): Boolean =
    hash.isNotEmpty() &&
        hash != currentHash &&
        content != currentContent &&
        syncMergeEmitDedup.shouldEmit(hash)

fun EditorViewModel.restartSyncObserver() {
    syncObserverJob?.cancel()
    syncObserverJob =
        editorScope.launch(Dispatchers.IO) {
            val repo = _syncStatusRepository ?: return@launch
            var lastSynced = false
            repo.state.collect { state ->
                val isSynced = state == com.xiwei.sujian.model.SyncIndicatorState.Synced
                if (isSynced && !lastSynced) {
                    checkSyncMergedChapter()
                }
                lastSynced = isSynced
            }
        }
}

suspend fun EditorViewModel.checkSyncMergedChapter() {
    val session = currentSession ?: return
    if (inputFrozen || _uiState.value.loading) return
    try {
        val (content, meta) =
            withContext(Dispatchers.IO) {
                workspaceRepository.getChapterContentWithMeta(
                    session.projectId,
                    session.volumeId,
                    session.chapterId,
                )
            }
        val currentHash = _uiState.value.chapterHash
        if (isSyncMergeApplicable(meta.hash, currentHash, content, _uiState.value.content)) {
            val syncState =
                try {
                    settingsRepository.loadSyncState()
                } catch (_: Exception) {
                    null
                }
            val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
            val baseVersion = _sessionCoordinator?.documentCommittedVersionFor(targetId) ?: DocumentVersion()
            emitDocumentFact(
                TargetDocumentFact(
                    targetId = targetId,
                    text = content,
                    sourceVersion =
                        DocumentVersion(
                            contentHash = meta.hash,
                            syncCommitId = syncState?.lastSyncedCommit,
                            parentVersion = baseVersion,
                        ),
                    baseVersion = baseVersion,
                    origin = DocumentFactOrigin.SYNC_MERGED,
                ),
            )
        }
    } catch (e: kotlinx.coroutines.CancellationException) {
        throw e
    } catch (_: Exception) {
        // 同步合并检查失败不阻塞用户操作
    }
}

suspend fun EditorViewModel.requestOpenChapter(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
): ChapterSwitchResult {
    return when (
        val gate =
            chapterSwitchGate.runLatest { isLatest ->
                switchChapterLocked(isLatest, projectId, volumeId, chapterId, chapterTitle)
            }
    ) {
        is ChapterSwitchGate.Result.Completed -> gate.value
        ChapterSwitchGate.Result.Stale -> ChapterSwitchResult.Stale
    }
}

fun EditorViewModel.isCurrentChapter(
    projectId: String,
    volumeId: String,
    chapterId: String,
): Boolean {
    val s = currentSession ?: return false
    return s.projectId == projectId && s.volumeId == volumeId && s.chapterId == chapterId
}

fun EditorViewModel.confirmEditorAttached(targetId: String) {
    val s = currentSession ?: return
    if (targetId == chapterTargetId(s.projectId, s.volumeId, s.chapterId)) {
        inputFrozen = false
    }
}

// #597：章节切换事务收敛 — 旧章节保存/清空收敛到独立函数，事务本体只保留
// 串行流程（保存→加载→预准备→提交）与 3 个可见提交边界检查；回滚路径统一
// 走 restoreAfterSwitch / rollbackAfterLoadFailure。
private suspend fun EditorViewModel.saveChapterContentForSwitch(
    session: EditorSession,
    content: String,
    revision: Long,
): Boolean =
    saveMutex.withLock {
        try {
            when (
                val result =
                    workspaceRepository.saveChapterContent(
                        session.projectId,
                        session.volumeId,
                        session.chapterId,
                        content,
                    )
            ) {
                is com.xiwei.sujian.data.BridgeResult.Success -> {
                    result.data?.contentHash?.let { hash ->
                        saveReceipts.record(
                            buildSaveToken(
                                chapterTargetId(session.projectId, session.volumeId, session.chapterId),
                                revision,
                                hash,
                            ),
                        )
                    }
                    true
                }
                is com.xiwei.sujian.data.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_failed, result.message),
                    )
                    false
                }
                com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_native_not_loaded),
                    )
                    false
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
            emitErrorEvent(
                getApplication<Application>().getString(R.string.error_save_exception, e.message ?: ""),
            )
            false
        }
    }

private suspend fun EditorViewModel.clearChapterContentForSwitch(
    session: EditorSession,
    revision: Long,
): Boolean =
    saveMutex.withLock {
        try {
            when (
                val result =
                    workspaceRepository.clearChapterContent(
                        session.projectId,
                        session.volumeId,
                        session.chapterId,
                    )
            ) {
                is com.xiwei.sujian.data.BridgeResult.Success -> {
                    result.data?.contentHash?.let { hash ->
                        saveReceipts.record(
                            buildSaveToken(
                                chapterTargetId(session.projectId, session.volumeId, session.chapterId),
                                revision,
                                hash,
                            ),
                        )
                    }
                    true
                }
                is com.xiwei.sujian.data.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_failed, result.message),
                    )
                    false
                }
                com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    false
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
            false
        }
    }

/** #595 一：加载/session 预准备失败回滚 — 恢复旧 EditorUiState 与输入冻结。 */
private fun EditorViewModel.rollbackAfterLoadFailure(
    oldSession: EditorSession?,
    oldUiState: EditorUiState,
    oldContentExplicitlyCleared: Boolean,
) {
    currentSession = oldSession
    _uiState.value = oldUiState.copy(loading = false)
    contentExplicitlyCleared = oldContentExplicitlyCleared
    inputFrozen = false
}

/** #595 一：章节切换事务上下文（旧状态用于失败回滚，isLatest 为可见提交边界检查）。 */
private class SwitchContext(
    val isLatest: () -> Boolean,
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
    val chapterTitle: String,
    val oldSession: EditorSession?,
    val oldUiState: EditorUiState,
    val oldContentExplicitlyCleared: Boolean,
)

/** #595 一：阶段 1 — 保存旧章节（含保存回执与失败/过期回滚）。null 表示继续。 */
private suspend fun EditorViewModel.switchSaveOldChapter(
    ctx: SwitchContext,
    oldSession: EditorSession,
): ChapterSwitchResult? {
    autoSaveJob?.cancel()
    saveActorJob?.cancel()
    saveCommandChannel.close()

    val content = _uiState.value.content
    // #595 七：保存旧章节也记录保存回执（revision 在保存时从会话层读取）。
    val oldRevision = _sessionCoordinator?.sessionState?.revision ?: 0L
    val saveOk =
        if (content.trim().isNotEmpty()) {
            saveChapterContentForSwitch(oldSession, content, oldRevision)
        } else if (contentExplicitlyCleared) {
            clearChapterContentForSwitch(oldSession, oldRevision)
        } else {
            true
        }

    if (!saveOk) {
        // #595 一：保存失败返回明确失败结果，且完整恢复旧 EditorUiState。
        _uiState.value = ctx.oldUiState.copy(loading = false, saveStatus = SaveStatus.SaveFailed)
        contentExplicitlyCleared = ctx.oldContentExplicitlyCleared
        saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
        startSaveActor()
        inputFrozen = false
        return ChapterSwitchResult.SaveFailed(oldSession)
    }
    // #595 一：可见提交边界 1 — 保存完成后若已有更新请求，回滚并退出。
    if (!ctx.isLatest()) {
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState, ctx.oldContentExplicitlyCleared)
        return ChapterSwitchResult.Stale
    }
    return null
}

private sealed interface SwitchPrepareOutcome {
    data class Ready(
        val session: EditorSession,
        val handle: com.xiwei.sujian.editor.v2.coordinator.PreparedSessionHandle,
    ) : SwitchPrepareOutcome

    data class Aborted(val result: ChapterSwitchResult) : SwitchPrepareOutcome
}

/**
 * #595 一：阶段 2 — 加载新章节 + 无副作用预准备 Rust session。
 * 失败/过期已在此回滚旧状态，调用方直接消费 [SwitchPrepareOutcome]。
 */
private suspend fun EditorViewModel.switchLoadAndPrepare(ctx: SwitchContext): SwitchPrepareOutcome {
    val newSession =
        EditorSession(
            sessionId = java.util.UUID.randomUUID().toString(),
            projectId = ctx.projectId,
            volumeId = ctx.volumeId,
            chapterId = ctx.chapterId,
        )
    currentSession = newSession
    // #595 二：章节提交后重置同步合并发射去重。
    syncMergeEmitDedup.reset()

    _uiState.value =
        _uiState.value.copy(
            loading = true,
            chapterTitle = ctx.chapterTitle,
            saveStatus = SaveStatus.Idle,
        )
    contentExplicitlyCleared = false
    startSaveActor()
    reloadSettings()
    // #595 一：加载在事务内完成 — 只有内容就绪后才提交 Success。
    val loaded = loadChapter(newSession)
    if (!loaded) {
        rollbackAfterLoadFailure(ctx.oldSession, ctx.oldUiState, ctx.oldContentExplicitlyCleared)
        return SwitchPrepareOutcome.Aborted(
            ChapterSwitchResult.LoadFailed(ChapterKey(ctx.projectId, ctx.volumeId, ctx.chapterId)),
        )
    }
    // #595 一：可见提交边界 2 — 加载完成后若已有更新请求，回滚并退出。
    if (!ctx.isLatest()) {
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState, ctx.oldContentExplicitlyCleared)
        return SwitchPrepareOutcome.Aborted(ChapterSwitchResult.Stale)
    }

    // #595 一：为 B 无副作用预准备 Rust session + 有效 snapshot/bind plan —
    // 在提交前完成，导航后编辑器立即可用；失败时 A 完全不变。
    val content = _uiState.value.content
    val handle = prepareTargetSession(chapterTargetId(ctx.projectId, ctx.volumeId, ctx.chapterId), content)
    if (handle == null) {
        rollbackAfterLoadFailure(ctx.oldSession, ctx.oldUiState, ctx.oldContentExplicitlyCleared)
        return SwitchPrepareOutcome.Aborted(
            ChapterSwitchResult.LoadFailed(ChapterKey(ctx.projectId, ctx.volumeId, ctx.chapterId)),
        )
    }
    // #595 一：可见提交边界 3 — session 预准备后再次校验 requestId。
    if (!ctx.isLatest()) {
        rollbackPreparedSession(handle)
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState, ctx.oldContentExplicitlyCleared)
        return SwitchPrepareOutcome.Aborted(ChapterSwitchResult.Stale)
    }
    return SwitchPrepareOutcome.Ready(newSession, handle)
}

/** #595 一：最终提交 — 一次性执行 A→B 切换；失败回滚到旧章节。 */
private suspend fun EditorViewModel.switchCommit(
    ctx: SwitchContext,
    newSession: EditorSession,
    handle: com.xiwei.sujian.editor.v2.coordinator.PreparedSessionHandle,
): ChapterSwitchResult {
    val coordinator = _sessionCoordinator
    if (coordinator == null || !coordinator.commitPreparedSession(handle)) {
        rollbackPreparedSession(handle)
        rollbackAfterLoadFailure(ctx.oldSession, ctx.oldUiState, ctx.oldContentExplicitlyCleared)
        return ChapterSwitchResult.LoadFailed(ChapterKey(ctx.projectId, ctx.volumeId, ctx.chapterId))
    }

    // #595 一：提交完成后的独立操作 — recordRecentEdit/统计失败只记录
    // 自身错误，不得回滚已成功打开的正文。
    editorScope.launch {
        try {
            withContext(Dispatchers.IO) {
                workspaceRepository.recordRecentEdit(ctx.projectId, ctx.volumeId, ctx.chapterId)
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (_: Exception) {
            // 最近编辑记录失败不影响章节打开
        }
    }
    // Success：inputFrozen 保持 true，由新 pane 附着编辑器后解除。
    return ChapterSwitchResult.Success(newSession, _uiState.value.content)
}

suspend fun EditorViewModel.switchChapterLocked(
    isLatest: () -> Boolean,
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
): ChapterSwitchResult {
    val ctx =
        SwitchContext(
            isLatest = isLatest,
            projectId = projectId,
            volumeId = volumeId,
            chapterId = chapterId,
            chapterTitle = chapterTitle,
            oldSession = currentSession,
            // #595 一：事务回滚需要完整的旧 EditorUiState — 保存/加载失败时整体恢复。
            oldUiState = _uiState.value,
            oldContentExplicitlyCleared = contentExplicitlyCleared,
        )

    val oldSession = ctx.oldSession
    if (oldSession != null && oldSession.matchesChapter(projectId, volumeId, chapterId)) {
        return ChapterSwitchResult.Success(oldSession, _uiState.value.content)
    }

    inputFrozen = true
    var preparedHandle: com.xiwei.sujian.editor.v2.coordinator.PreparedSessionHandle? = null
    try {
        // #595 一/转场：切换章节时同步置 loading — 在旧章节保存完成前就隐藏编辑器。
        _uiState.value = _uiState.value.copy(loading = true)

        val saveAbort =
            if (oldSession != null) {
                switchSaveOldChapter(ctx, oldSession)
            } else {
                saveActorJob?.cancel()
                saveCommandChannel.close()
                null
            }
        if (saveAbort != null) return saveAbort

        saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
        when (val outcome = switchLoadAndPrepare(ctx)) {
            is SwitchPrepareOutcome.Aborted -> return outcome.result
            is SwitchPrepareOutcome.Ready -> {
                preparedHandle = outcome.handle
                return switchCommit(ctx, outcome.session, outcome.handle)
            }
        }
    } catch (e: kotlinx.coroutines.CancellationException) {
        // #595 一：取消不是失败 — 恢复旧状态后向上重抛，让更新的请求
        // （若有）从一致状态开始；不允许把取消当普通加载失败回滚导航。
        if (preparedHandle != null) {
            rollbackPreparedSession(preparedHandle)
        }
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState, ctx.oldContentExplicitlyCleared)
        throw e
    } finally {
        // 注意：Success 路径不在此解除冻结 — 由 confirmEditorAttached 解除。
    }
}

suspend fun EditorViewModel.restoreAfterSwitch(
    oldSession: EditorSession?,
    oldUiState: EditorUiState,
    oldContentExplicitlyCleared: Boolean,
) {
    // #595 一：无副作用预准备不修改 A 的会话状态（不 commit/cancel A、
    // 不切换 activeTargetId），回滚无需重新 prepare A — A 的 Rust session ID、
    // Undo/Redo、composition、selection 与事务前完全一致。
    currentSession = oldSession
    _uiState.value = oldUiState.copy(loading = false)
    contentExplicitlyCleared = oldContentExplicitlyCleared
    saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
    startSaveActor()
    scheduleAutoSave(_uiState.value.content)
    inputFrozen = false
}

fun EditorViewModel.rollbackPreparedSession(handle: com.xiwei.sujian.editor.v2.coordinator.PreparedSessionHandle) {
    try {
        _sessionCoordinator?.releasePreparedTarget(handle)
    } catch (_: Exception) {
        // 释放失败不阻塞回滚
    }
}

fun EditorViewModel.prepareTargetSession(
    targetId: String,
    content: String,
): com.xiwei.sujian.editor.v2.coordinator.PreparedSessionHandle? {
    val coordinator = _sessionCoordinator ?: return null
    if (!coordinator.isTargetRegistered(targetId)) {
        coordinator.registerTargetMeta(targetId, TextEditorProfile.DocumentBody, persistent = true)
    }
    val cursorUtf8 = content.toByteArray(Charsets.UTF_8).size
    return coordinator.prepareTargetSessionForCommit(targetId, content, cursorUtf8)
}

fun EditorViewModel.initChapter(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
) {
    val existing = currentSession
    if (existing != null && existing.matchesChapter(projectId, volumeId, chapterId)) {
        return
    }

    currentSession =
        EditorSession(
            sessionId = java.util.UUID.randomUUID().toString(),
            projectId = projectId,
            volumeId = volumeId,
            chapterId = chapterId,
        )
    // #595 二：章节提交后重置同步合并发射去重（与 switchChapterLocked 一致）。
    syncMergeEmitDedup.reset()
    _uiState.value =
        _uiState.value.copy(
            loading = true,
            chapterTitle = chapterTitle,
        )
    contentExplicitlyCleared = false
    startSaveActor()
    reloadSettings()
    editorScope.launch {
        loadChapter(currentSession!!)
    }
}

fun EditorViewModel.initErrorState(errorMessage: String) {
    _uiState.value =
        _uiState.value.copy(
            loading = false,
            content = errorMessage,
            editorEnabled = false,
            saveStatus = SaveStatus.Idle,
        )
}

suspend fun EditorViewModel.loadChapter(session: EditorSession): Boolean {
    isLoadingChapter = true
    val sessionId = session.sessionId
    return try {
        val result =
            withContext(kotlinx.coroutines.Dispatchers.IO) {
                workspaceRepository.getChapterContentWithMeta(session.projectId, session.volumeId, session.chapterId)
            }
        val content = result.first
        val meta = result.second

        com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterLoad(
            session.projectId,
            session.chapterId,
            content.toByteArray(Charsets.UTF_8).size,
            "ok",
        )

        if (currentSession?.sessionId != sessionId) return false

        _uiState.value =
            _uiState.value.copy(
                loading = false,
                content = content,
                chapterHash = meta.hash,
                chapterNote = meta.note,
                editorEnabled = true,
                saveStatus = SaveStatus.Idle,
            )
        contentDirty = false
        // #595 七：加载即记录磁盘版本回执（revision 0 — 尚未编辑，屏幕与磁盘一致），
        // 同步前 flush 不把"从未保存"误判为假成功。
        saveReceipts.record(
            buildSaveToken(chapterTargetId(session.projectId, session.volumeId, session.chapterId), 0L, meta.hash),
        )
        val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
        // #595 四：Android 不自行填写 parentVersion — 磁盘版本可能来自 Git 回退、
        // 外部修改或迟到 IO，不能伪称为上次 committed 的后代。版本因果只能由
        // Core/Repository 返回（当前 Core 尚无独立章节 revision，保持无 parent）。
        // shouldApplyExternalContent 对 REPOSITORY_LOAD 信任磁盘内容直接 Apply。
        val loadedVersion = DocumentVersion(contentHash = meta.hash)
        // #595 二：Repository 加载完成即发布文档事实（真实 hash 锚点）。
        // 最终 revision 来自 reset 后的真实 Rust snapshot。
        emitDocumentFact(
            TargetDocumentFact(
                targetId = targetId,
                text = content,
                sourceVersion = loadedVersion,
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        previousText = content
        initialWordCount = calculateWordCount(content)
        sessionStartTime = System.currentTimeMillis()
        updateStats(content)
        isLoadingChapter = false
        true
    } catch (e: Throwable) {
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterLoad(
            session.projectId,
            session.chapterId,
            0,
            "error",
        )
        if (currentSession?.sessionId != sessionId) return false
        if (e is kotlinx.coroutines.CancellationException) {
            // #595 一：协程取消不是加载失败 — 恢复现场标记后向上重抛。
            isLoadingChapter = false
            throw e
        }
        isLoadingChapter = false
        _uiState.value =
            _uiState.value.copy(
                loading = false,
                editorEnabled = false,
                saveStatus = SaveStatus.Idle,
            )
        emitErrorEvent(getApplication<Application>().getString(R.string.error_load_chapter_failed, e.message ?: ""))
        false
    }
}

fun EditorViewModel.applyExternalContentToUi(
    targetId: String,
    text: String,
    fileHash: String,
) {
    val s = currentSession ?: return
    if (targetId != chapterTargetId(s.projectId, s.volumeId, s.chapterId)) return
    val current = _uiState.value
    _uiState.value =
        current.copy(
            content = text,
            chapterHash = fileHash,
            saveStatus = SaveStatus.Saved,
        )
    contentDirty = false
    contentExplicitlyCleared = false
    // #595 七：同步合并内容已由 Core 写入磁盘 — 记录回执（revision 取
    // reset 后的真实 session revision），同步后 flush 不误判为未保存。
    val revision = _sessionCoordinator?.sessionState?.revision ?: 0L
    saveReceipts.record(buildSaveToken(targetId, revision, fileHash))
    previousText = text
    updateStats(text)
}

fun EditorViewModel.notifySyncMergeConflict() {
    editorScope.launch {
        emitErrorEvent(getApplication<Application>().getString(R.string.error_sync_document_conflict))
    }
}
