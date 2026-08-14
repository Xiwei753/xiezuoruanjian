package com.xiwei.sujian.feature.editor.ui

// ! # 编辑器章节切换操作（从 EditorViewModel 拆分）
// !
// ! 章节打开事务（latest-wins + 提交前 session 预准备）、章节加载、
// ! 同步合并检查、外部内容应用。

import android.app.Application
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.session.DocumentFactOrigin
import com.xiwei.sujian.feature.editor.session.DocumentVersion
import com.xiwei.sujian.feature.editor.session.TargetDocumentFact
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.commitPreparedSession
import com.xiwei.sujian.feature.editor.session.documentCommittedVersionFor
import com.xiwei.sujian.feature.editor.session.prepareTargetSessionForCommit
import com.xiwei.sujian.feature.editor.session.releasePreparedTarget
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
 * #624 评论11 第5项：同步合并前置筛选 — 只做 hash/dedup 判断。
 *
 * 旧实现还比较 `content != currentContent`：拿「同步后的磁盘正文」和「刚打开
 * 章节时的冷路径旧 UI 字符串」比较。评论9 之后本地正常输入不再更新
 * `_uiState.content`，这个比较会错误提前吞掉 hash 真变化的同步事实。
 * 正文相同/dirty/版本因果全部交给会话层
 * EditorSessionExternalOps.shouldApplyExternalContent（低频权威 snapshot 比较），
 * 这里不复制第二套正文真值。
 */
internal fun syncMergePrefilter(
    hash: String,
    currentHash: String,
    shouldEmit: Boolean,
): Boolean = hash.isNotEmpty() && hash != currentHash && shouldEmit

fun EditorViewModel.restartSyncObserver() {
    syncObserverJob?.cancel()
    syncObserverJob =
        editorScope.launch(Dispatchers.IO) {
            val repo = _syncStatusRepository ?: return@launch
            var lastSynced = false
            repo.state.collect { state ->
                val isSynced = state == com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState.Synced
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
                chapterRepository.getChapterContentWithMeta(
                    session.projectId,
                    session.volumeId,
                    session.chapterId,
                )
            }
        val currentHash = _uiState.value.chapterHash
        // #624 评论11 第5项：只做 hash/dedup 前置筛选 — 不再拿 _uiState.content
        // （冷路径旧正文）与磁盘正文比较；是否应用由 shouldApplyExternalContent 判定。
        if (syncMergePrefilter(meta.hash, currentHash, syncMergeEmitDedup.shouldEmit(meta.hash))) {
            val syncState =
                try {
                    syncRepository.loadSyncState(session.projectId)
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
                    effectiveChapterSavePort.saveChapterContent(
                        session.projectId,
                        session.volumeId,
                        session.chapterId,
                        content,
                    )
            ) {
                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> {
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
                is com.xiwei.sujian.core.interop.common.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_failed, result.message),
                    )
                    false
                }
                com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded -> {
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
                    chapterRepository.clearChapterContent(
                        session.projectId,
                        session.volumeId,
                        session.chapterId,
                    )
            ) {
                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> {
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
                is com.xiwei.sujian.core.interop.common.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_failed, result.message),
                    )
                    false
                }
                com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded -> {
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

/**
 * #624 评论10 第2项：离开正文（章节关闭）前保存当前真实正文。
 *
 * 与切章同一入口：从 Rust session 的真实 snapshot/lease（按 target 取得，
 * 活动窗口/Detached 持久 session 同一入口）取 text + revision，不回退
 * `_uiState.content`（评论9 后本地输入不再更新它 — 保存旧正文会把刚输入
 * 的内容覆盖回磁盘）。
 *
 * 返回 true 表示可安全关闭（已保存或无需保存）；false 表示拿不到真实
 * snapshot（未注册/session 已关/快照缺失/错版）或落盘失败 — 调用方必须
 * 中止关闭，保留 Detached session 与未落盘正文，绝不伪造空正文保存
 * （否则旧章节会被清空）。
 */
suspend fun EditorViewModel.saveTargetBeforeClose(
    targetId: String,
    projectId: String,
    volumeId: String,
    chapterId: String,
): Boolean {
    // #624 评论10 第1项：只有 snapshot 存在且 revision 匹配才返回 lease。
    val lease = _sessionCoordinator?.issueDocumentOperationLease(targetId)
    if (lease == null) {
        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
        return false
    }
    val session = EditorSession(java.util.UUID.randomUUID().toString(), projectId, volumeId, chapterId)
    val content = lease.text
    // #595 七：revision 从 lease 取，与正文同源（真实 snapshot）。
    val revision = lease.rustRevision
    // #624 评论11 第2项：空正文判定只看原始字符串 — "\n"、连续空行、只含空格/
    // 制表符的段落都是用户正文，原样保存；只有真正的 "" 且 contentDirty
    // （用户删空）才走 Clear；未编辑过的空正文无需保存。
    return if (content.isNotEmpty()) {
        saveChapterContentForSwitch(session, content, revision)
    } else if (contentDirty) {
        clearChapterContentForSwitch(session, revision)
    } else {
        true
    }
}

/** #595 一：加载/session 预准备失败回滚 — 恢复旧 EditorUiState 与输入冻结。 */
private fun EditorViewModel.rollbackAfterLoadFailure(
    oldSession: EditorSession?,
    oldUiState: EditorUiState,
) {
    currentSession = oldSession
    _uiState.value = oldUiState.copy(loading = false)
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
)

/** #595 一：阶段 1 — 保存旧章节（含保存回执与失败/过期回滚）。null 表示继续。 */
private suspend fun EditorViewModel.switchSaveOldChapter(
    ctx: SwitchContext,
    oldSession: EditorSession,
): ChapterSwitchResult? {
    autoSaveJob?.cancel()
    saveActorJob?.cancel()
    saveCommandChannel.close()

    // #624 评论10 第2项：旧章节保存必须从 Rust session 的真实 snapshot/lease 取
    // text + revision。评论9 之后本地正常输入不再更新 _uiState.content，
    // _uiState.content 是"刚打开章节时的旧正文"，用它保存会把刚才输入覆盖回去（数据丢失）。
    // lease 由 issueDocumentOperationLease 签发：只有活动 target/session 存在且
    // snapshot 可读且 revision 匹配时才非空（评论10 第1项收紧）。
    val lease = _sessionCoordinator?.issueDocumentOperationLease()
    if (lease == null) {
        // 拿不到真实 snapshot — 中止本次保存/切章，不回退 _uiState.content，
        // 不伪造空正文保存（否则会把旧章节清空）。
        _uiState.value = _uiState.value.copy(loading = false, saveStatus = SaveStatus.SaveFailed)
        saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
        startSaveActor()
        inputFrozen = false
        return ChapterSwitchResult.SaveFailed(oldSession)
    }
    val content = lease.text
    // #595 七：保存旧章节也记录保存回执（revision 从 lease 取，与正文同源）。
    val oldRevision = lease.rustRevision
    // #624 评论11 第2项：旧章节保存只看 contentDirty + lease.text：未 dirty 不保存；
    // dirty + 空正文（用户删空）→ Clear — 不得走 else true 让磁盘旧正文残留。
    val saveOk =
        if (content.isNotEmpty()) {
            saveChapterContentForSwitch(oldSession, content, oldRevision)
        } else if (contentDirty) {
            clearChapterContentForSwitch(oldSession, oldRevision)
        } else {
            true
        }

    if (!saveOk) {
        // #595 一：保存失败返回明确失败结果，且完整恢复旧 EditorUiState。
        _uiState.value = ctx.oldUiState.copy(loading = false, saveStatus = SaveStatus.SaveFailed)
        saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
        startSaveActor()
        inputFrozen = false
        return ChapterSwitchResult.SaveFailed(oldSession)
    }
    // #595 一：可见提交边界 1 — 保存完成后若已有更新请求，回滚并退出。
    if (!ctx.isLatest()) {
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
        return ChapterSwitchResult.Stale
    }
    return null
}

private sealed interface SwitchPrepareOutcome {
    data class Ready(
        val session: EditorSession,
        val handle: com.xiwei.sujian.feature.editor.session.PreparedSessionHandle,
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
    startSaveActor()
    reloadSettings()
    // #595 一：加载在事务内完成 — 只有内容就绪后才提交 Success。
    val loaded = loadChapter(newSession)
    if (!loaded) {
        rollbackAfterLoadFailure(ctx.oldSession, ctx.oldUiState)
        return SwitchPrepareOutcome.Aborted(
            ChapterSwitchResult.LoadFailed(ChapterKey(ctx.projectId, ctx.volumeId, ctx.chapterId)),
        )
    }
    // #595 一：可见提交边界 2 — 加载完成后若已有更新请求，回滚并退出。
    if (!ctx.isLatest()) {
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
        return SwitchPrepareOutcome.Aborted(ChapterSwitchResult.Stale)
    }

    // #595 一：为 B 无副作用预准备 Rust session + 有效 snapshot/bind plan —
    // 在提交前完成，导航后编辑器立即可用；失败时 A 完全不变。
    val content = _uiState.value.content
    val handle = prepareTargetSession(chapterTargetId(ctx.projectId, ctx.volumeId, ctx.chapterId), content)
    if (handle == null) {
        rollbackAfterLoadFailure(ctx.oldSession, ctx.oldUiState)
        return SwitchPrepareOutcome.Aborted(
            ChapterSwitchResult.LoadFailed(ChapterKey(ctx.projectId, ctx.volumeId, ctx.chapterId)),
        )
    }
    // #595 一：可见提交边界 3 — session 预准备后再次校验 requestId。
    if (!ctx.isLatest()) {
        rollbackPreparedSession(handle)
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
        return SwitchPrepareOutcome.Aborted(ChapterSwitchResult.Stale)
    }
    return SwitchPrepareOutcome.Ready(newSession, handle)
}

/** #595 一：最终提交 — 一次性执行 A→B 切换；失败回滚到旧章节。 */
private suspend fun EditorViewModel.switchCommit(
    ctx: SwitchContext,
    newSession: EditorSession,
    handle: com.xiwei.sujian.feature.editor.session.PreparedSessionHandle,
): ChapterSwitchResult {
    val coordinator = _sessionCoordinator
    if (coordinator == null || !coordinator.commitPreparedSession(handle)) {
        rollbackPreparedSession(handle)
        rollbackAfterLoadFailure(ctx.oldSession, ctx.oldUiState)
        return ChapterSwitchResult.LoadFailed(ChapterKey(ctx.projectId, ctx.volumeId, ctx.chapterId))
    }

    // #595 一：提交完成后的独立操作 — recordRecentEdit/统计失败只记录
    // 自身错误，不得回滚已成功打开的正文。
    editorScope.launch {
        try {
            withContext(Dispatchers.IO) {
                recentEditsRepository.recordRecentEdit(ctx.projectId, ctx.volumeId, ctx.chapterId)
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (_: Exception) {
            // 最近编辑记录失败不影响章节打开
        }
    }
    // Success：inputFrozen 保持 true，由新 pane 附着编辑器后解除。
    // #624 评论11 第5项：Success 不再携带正文 — 生产调用方只判断 Success 不消费
    // 正文；继续暴露只会让后续代码再次误把 load-only UI 字段当编辑正文真值。
    return ChapterSwitchResult.Success(newSession)
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
        )

    val oldSession = ctx.oldSession
    if (oldSession != null && oldSession.matchesChapter(projectId, volumeId, chapterId)) {
        // #624 评论11 第5项：Success 不再携带正文（冷路径字段不是编辑真值）。
        return ChapterSwitchResult.Success(oldSession)
    }

    // #597：真实切换事务开始 — 作废 initChapter 遗留入口启动的后台加载的
    // 可见状态写入权（迟到失败写入不得覆盖本事务的 SaveFailed/回滚状态）。
    chapterLoadEpoch.incrementAndGet()

    inputFrozen = true
    var preparedHandle: com.xiwei.sujian.feature.editor.session.PreparedSessionHandle? = null
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
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
        throw e
    } finally {
        // 注意：Success 路径不在此解除冻结 — 由 confirmEditorAttached 解除。
    }
}

fun EditorViewModel.restoreAfterSwitch(
    oldSession: EditorSession?,
    oldUiState: EditorUiState,
) {
    // #595 一：无副作用预准备不修改 A 的会话状态（不 commit/cancel A、
    // 不切换 activeTargetId），回滚无需重新 prepare A — A 的 Rust session ID、
    // Undo/Redo、composition、selection 与事务前完全一致。
    currentSession = oldSession
    _uiState.value = oldUiState.copy(loading = false)
    saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
    startSaveActor()
    scheduleAutoSave()
    inputFrozen = false
}

fun EditorViewModel.rollbackPreparedSession(handle: com.xiwei.sujian.feature.editor.session.PreparedSessionHandle) {
    try {
        _sessionCoordinator?.releasePreparedTarget(handle)
    } catch (_: Exception) {
        // 释放失败不阻塞回滚
    }
}

fun EditorViewModel.prepareTargetSession(
    targetId: String,
    content: String,
): com.xiwei.sujian.feature.editor.session.PreparedSessionHandle? {
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
    // #597：本加载的纪元 — 切换事务开始后纪元递增，旧加载不得写可见状态。
    val loadEpoch = chapterLoadEpoch.get()
    return try {
        val result =
            withContext(kotlinx.coroutines.Dispatchers.IO) {
                chapterRepository.getChapterContentWithMeta(session.projectId, session.volumeId, session.chapterId)
            }
        val content = result.first
        val meta = result.second

        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.chapterLoad(
            session.projectId,
            session.chapterId,
            content.toByteArray(Charsets.UTF_8).size,
            "ok",
        )

        if (currentSession?.sessionId != sessionId) return false
        if (chapterLoadEpoch.get() != loadEpoch) return false

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
        // #624 评论9：previousText 已删除 — 统计改增量 recordWritingEvent。
        // 冷路径 load：用整章 content 重算 wordCount 并设入 _uiState，再 updateStats() 算 speed。
        initialWordCount = calculateWordCount(content)
        sessionStartTime = System.currentTimeMillis()
        _uiState.value = _uiState.value.copy(wordCount = initialWordCount)
        updateStats()
        isLoadingChapter = false
        true
    } catch (e: Throwable) {
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.chapterLoad(
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
        // #597：事务已接管（纪元递增）的迟到背景加载不得写可见状态，
        // 也不得发射错误事件 — 只复位自己的加载标记。
        if (chapterLoadEpoch.get() != loadEpoch) return false
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
    // #595 七：同步合并内容已由 Core 写入磁盘 — 记录回执（revision 取
    // reset 后的真实 session revision），同步后 flush 不误判为未保存。
    val revision = _sessionCoordinator?.sessionState?.revision ?: 0L
    saveReceipts.record(buildSaveToken(targetId, revision, fileHash))
    // #624 评论9：previousText 已删除 — 统计改增量 recordWritingEvent。
    // 冷路径 external-apply：用整章 text 重算 wordCount 并设入 _uiState，再 updateStats() 算 speed。
    _uiState.value = _uiState.value.copy(wordCount = calculateWordCount(text))
    updateStats()
}

fun EditorViewModel.notifySyncMergeConflict() {
    editorScope.launch {
        emitErrorEvent(getApplication<Application>().getString(R.string.error_sync_document_conflict))
    }
}
