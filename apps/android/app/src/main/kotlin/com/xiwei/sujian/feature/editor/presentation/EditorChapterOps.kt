package com.xiwei.sujian.feature.editor.presentation

// ! # 编辑器章节切换操作（从 EditorViewModel 拆分）
// !
// ! 章节打开事务（latest-wins + 提交前 session 预准备）、章节加载、
// ! 同步合并检查、外部内容应用。

import android.app.Application
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.session.DocumentFactOrigin
import com.xiwei.sujian.feature.editor.session.DocumentOperationLease
import com.xiwei.sujian.feature.editor.session.DocumentVersion
import com.xiwei.sujian.feature.editor.session.ExternalContentDecision
import com.xiwei.sujian.feature.editor.session.PendingExternalVersion
import com.xiwei.sujian.feature.editor.session.TargetDocumentFact
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.applyExternalContentFact
import com.xiwei.sujian.feature.editor.session.commitPreparedSession
import com.xiwei.sujian.feature.editor.session.commitSavedLease
import com.xiwei.sujian.feature.editor.session.consumePendingExternalFact
import com.xiwei.sujian.feature.editor.session.documentCommittedVersionFor
import com.xiwei.sujian.feature.editor.session.pendingExternalFactFor
import com.xiwei.sujian.feature.editor.session.prepareTargetSessionForCommit
import com.xiwei.sujian.feature.editor.session.releasePreparedTarget
import com.xiwei.sujian.feature.editor.session.shouldApplyExternalContent
import com.xiwei.sujian.feature.editor.session.toSaveToken
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
 * #624 评论174 第5项 / 评论17 问题3：同步合并前置筛选 — 只做 hash 比较。
 *
 * 旧实现还比较 `content != currentContent`：拿「同步后的磁盘正文」和「刚打开
 * 章节时的冷路径旧 UI 字符串」比较。评论9 之后本地正常输入不再更新
 * `_uiState.content`，这个比较会错误提前吞掉 hash 真变化的同步事实。
 * 正文相同/dirty/版本因果全部交给会话层
 * EditorSessionExternalOps.shouldApplyExternalContent（低频权威 snapshot 比较），
 * 这里不复制第二套正文真值。
 *
 * #624 评论17 问题3：删除 SyncMergeEmitDedup hash 去重 — 发射端不维护
 * "最后发过什么 hash"。Repository hash 与 documentCommittedVersion.contentHash
 * 不同即放行，真正的 Replay/Older/SameContent 判断只由 shouldApplyExternalContent 做。
 * 同 hash dirty conflict 事实不得被永久吞掉。
 */
internal fun syncMergePrefilter(
    hash: String,
    currentHash: String,
): Boolean = hash.isNotEmpty() && hash != currentHash

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
        val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
        // #624 评论17 问题3：用 Repository hash 与 documentCommittedVersion.contentHash 比较，
        // 不再用 SyncMergeEmitDedup hash 去重。同 hash dirty conflict 事实不被吞。
        val committedHash = _sessionCoordinator?.documentCommittedVersionFor(targetId)?.contentHash ?: ""
        if (syncMergePrefilter(meta.hash, committedHash)) {
            val baseVersion = _sessionCoordinator?.documentCommittedVersionFor(targetId) ?: DocumentVersion()
            emitDocumentFact(
                TargetDocumentFact(
                    targetId = targetId,
                    text = content,
                    sourceVersion =
                        DocumentVersion(
                            contentHash = meta.hash,
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

/**
 * #624 评论17 问题5：从 pending + Repository 读结果构造 reapply 事实 — 纯函数。
 *
 * 旧 `checkSyncMergedChapter()` 始终用 `origin = SYNC_MERGED` 构造事实，
 * 导致 `REPOSITORY_LOAD` origin 的 pending 保存清 dirty 后重读被错误标成
 * SYNC_MERGED，`shouldApplyExternalContent` 对不可比较版本返回
 * `IgnoreUncomparableConflict`（重新存 pending，永不解决）。
 *
 * 本函数从 [pending] 取 origin（保留原事实来源），从 Repository 读最新
 * [repositoryContent]/[repositoryHash] 构造事实版本（不拿 pending 旧 hash）。
 *
 * sourceVersion 保留 pending 原事实的版本锚点（[DocumentVersion.parentVersion]/
 * [DocumentVersion.repositoryRevision]）— 重读事实的版本关系与原事实一致，
 * 不假设基于当前 committedVersion。否则 parentVersion=baseVersion=committed
 * 会让二者可比较，无法暴露 origin 对不可比较版本决策的影响（可比较时
 * 不论 origin 都 Apply）。[baseVersion] 仅用于 [TargetDocumentFact.baseVersion]
 * （本地正文基于的版本，保存后的 committedVersion）。
 *
 * [syncCommitId] 仅在 `pending.origin == SYNC_MERGED` 时由调用方传入；Core
 * provider-neutral 重构后不再暴露 commit hash，调用方传 null。
 * `REPOSITORY_LOAD` origin 传 null（章节加载事实不带同步锚点）。
 */
internal fun buildPendingReapplyFact(
    pending: PendingExternalVersion,
    targetId: String,
    repositoryContent: String,
    repositoryHash: String,
    baseVersion: DocumentVersion,
    syncCommitId: String?,
): TargetDocumentFact =
    TargetDocumentFact(
        targetId = targetId,
        text = repositoryContent,
        sourceVersion =
            DocumentVersion(
                contentHash = repositoryHash,
                repositoryRevision = pending.sourceVersion.repositoryRevision,
                syncCommitId = syncCommitId,
                parentVersion = pending.sourceVersion.parentVersion,
            ),
        baseVersion = baseVersion,
        origin = pending.origin,
        // #624 评论17 问题5：reapply fact 标记 — 让 handleExternalDocumentFact
        // 在 IgnoreReplay/IgnoreOlder 分支也消费 pending，避免泄漏。
        isReapply = true,
    )

/**
 * #624 评论17 问题5：决定事实处理后是否消费 pendingExternal。
 *
 * - Apply/IgnoreSameContent → 消费（冲突已解决，版本已提交）
 * - IgnoreReplay/IgnoreOlder → 仅 reapply fact 消费（外部状态已对齐/本地更新）
 * - IgnoreDirtyConflict/IgnoreUncomparableConflict/IgnoreEmptyVersion → 不消费
 */
internal fun shouldConsumePendingAfterFact(
    decision: ExternalContentDecision,
    isReapply: Boolean,
): Boolean =
    when (decision) {
        ExternalContentDecision.Apply,
        ExternalContentDecision.IgnoreSameContent,
        -> true
        ExternalContentDecision.IgnoreReplay,
        ExternalContentDecision.IgnoreOlder,
        -> isReapply
        ExternalContentDecision.IgnoreDirtyConflict,
        ExternalContentDecision.IgnoreUncomparableConflict,
        ExternalContentDecision.IgnoreEmptyVersion,
        -> false
    }

/**
 * #624 评论17 问题5：保存成功后重读 pendingExternal 并重新应用 — 替换无条件
 * `checkSyncMergedChapter()`。
 *
 * 流程：
 * 1. 取 target 的 pendingExternal，无则返回 false（无事可做）；
 * 2. 从 Repository 读最新正文/hash（不拿 pending 旧正文/hash）；
 * 3. baseVersion = 当前 committedVersion（保存后版本）；
 * 4. syncCommitId 恒为 null（Core provider-neutral 重构后不再暴露 commit hash）；
 * 5. 用 [buildPendingReapplyFact] 构造事实（origin 来自 pending）并 emit；
 * 6. 返回 true 表示已触发 reapply。
 *
 * 异常处理：CancellationException 重抛，其他异常返回 false（pending 保留等下次触发）。
 */
suspend fun EditorViewModel.reapplyPendingExternalAfterSave(targetId: String): Boolean {
    val coordinator = _sessionCoordinator ?: return false
    val pending = coordinator.pendingExternalFactFor(targetId) ?: return false
    val session = currentSession ?: return false
    return try {
        val (content, meta) =
            withContext(Dispatchers.IO) {
                chapterRepository.getChapterContentWithMeta(
                    session.projectId,
                    session.volumeId,
                    session.chapterId,
                )
            }
        val baseVersion = coordinator.documentCommittedVersionFor(targetId)
        val syncCommitId: String? = null
        val fact =
            buildPendingReapplyFact(
                pending = pending,
                targetId = targetId,
                repositoryContent = content,
                repositoryHash = meta.hash,
                baseVersion = baseVersion,
                syncCommitId = syncCommitId,
            )
        // #624 评论17 问题5：预检查决策 — IgnoreReplay/IgnoreOlder 时直接消费 pending，
        // 不 emit（事实是 no-op，避免 collector 处理的竞态窗口）。
        val decision = coordinator.shouldApplyExternalContent(fact)
        if (shouldConsumePendingAfterFact(decision, fact.isReapply) &&
            decision != ExternalContentDecision.Apply &&
            decision != ExternalContentDecision.IgnoreSameContent
        ) {
            coordinator.consumePendingExternalFact(targetId)
            return true
        }
        emitDocumentFact(fact)
        true
    } catch (e: kotlinx.coroutines.CancellationException) {
        throw e
    } catch (_: Exception) {
        // 重读失败：pending 保留，等下次保存/同步触发再重试。
        false
    }
}

suspend fun EditorViewModel.requestOpenChapter(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
): ChapterSwitchResult {
    // #632 评论 5378239827 项4: 按 ChapterSwitchKey 进入 gate —
    // 同目标 join（已有 in-flight 请求直接 await），不同目标 latest-wins。
    val key = ChapterSwitchGate.ChapterSwitchKey(projectId, volumeId, chapterId)
    return when (
        val gate =
            chapterSwitchGate.runChapterSwitch(key) { isLatest ->
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

/**
 * #644 评论 5462826712 第2节：confirmEditorAttached 改为接收 lease。
 *
 * 除了当前章节身份，还必须调用 session coordinator 的 isInputLeaseCurrent(lease, targetId)。
 * 只有 lease 仍然属于当前 target/session/epoch 才 setInputFrozen(false)。
 */
fun EditorViewModel.confirmEditorAttached(
    targetId: String,
    lease: com.xiwei.sujian.feature.editor.session.EditorInputLease,
) {
    val s = currentSession ?: return
    if (targetId == chapterTargetId(s.projectId, s.volumeId, s.chapterId)) {
        val coordinator = _sessionCoordinator ?: return
        if (coordinator.isInputLeaseCurrent(lease, targetId)) {
            setInputFrozen(false)
        }
    }
}

// #597：章节切换事务收敛 — 旧章节保存/清空收敛到独立函数，事务本体只保留
// 串行流程（保存→加载→预准备→提交）与 3 个可见提交边界检查；回滚路径统一
// 走 restoreAfterSwitch / rollbackAfterLoadFailure。
// #624 评论12 第2项：保存完成统一提交（回执 + markSaved）— 切章保存成功后
// 持久 session 的 DocumentState.localDirty 必须清掉，否则后面的同步事实会被
// IgnoreDirtyConflict 拦截。
// #624 评论13 第2项：切章保存收成稳定 lease 事务 — Repository 成功只代表
// "这一版 lease 正文已落盘"，不代表"可以离开章节"；保存期间 revision 前进时
// 重新签发最新 lease 再保存，直到最新 revision 真正提交（Committed）。

/** #624 评论13 第2项：切章保存事务结果。 */
private enum class SwitchSaveOutcome {
    /** 最新 revision 已真正落盘（或无需保存），可以继续加载新章节。 */
    Committed,

    /** 事务已过期（有更新请求排队）— 回滚后由调用方返回 Stale。 */
    Stale,

    /** snapshot 缺失/错版或保存失败 — 中止切章，返回 SaveFailed。 */
    Failed,
}

/**
 * #624 评论13 第2项：切章保存的版本提交判定 — Repository 成功只代表这一版
 * lease 正文已写进磁盘。回执总是记录（该 revision 确实已落盘）；只有 lease
 * 仍匹配 target/session/epoch 且活动 revision 未前进时才 markSaved 并返回
 * true（Committed）。返回 false 表示保存期间 revision 前进 — 调用方必须重新
 * 签发最新 lease 再保存，不得把"旧 revision 已写盘"算成可以离开章节。
 */
private fun EditorViewModel.commitSwitchSave(
    lease: DocumentOperationLease,
    hash: String,
): Boolean {
    saveReceipts.record(lease.toSaveToken(hash))
    val coordinator = _sessionCoordinator ?: return false
    // #624 评论17 问题5：改成和正常保存相同的 commitSavedLease — 在 mutateSession
    // 临界区内原子校验 lease + 清 dirty，不再保留第二套先判断再清 dirty 的窗口。
    return coordinator.commitSavedLease(lease, DocumentVersion(contentHash = hash))
}

/**
 * #624 评论13 第2项：切章保存稳定 lease 事务 — 循环签发当前真实
 * [DocumentOperationLease]，直到最新 revision 真正落盘：
 *
 * - `!lease.localDirty` → 未编辑，直接 [SwitchSaveOutcome.Committed]；
 * - dirty+空正文 → Clear；dirty+非空 → Save（正文/revision 都来自真实 snapshot）；
 * - Repository 成功只代表这一版 lease 正文写进磁盘 — lease 已前进（保存期间
 *   revision 前进）就重新签发最新 snapshot 再保存，直到最新 revision 提交；
 * - `ctx.isLatest()` 失效返回 [SwitchSaveOutcome.Stale]（latest-wins 边界在
 *   保存循环内同样生效）；
 * - snapshot 缺失/错版返回 [SwitchSaveOutcome.Failed]（不伪造空正文，评论10）。
 */
private suspend fun EditorViewModel.persistOldChapterForSwitch(
    ctx: SwitchContext,
    oldSession: EditorSession,
): SwitchSaveOutcome {
    val oldTargetId = chapterTargetId(oldSession.projectId, oldSession.volumeId, oldSession.chapterId)
    while (true) {
        if (!ctx.isLatest()) return SwitchSaveOutcome.Stale
        val coordinator = _sessionCoordinator ?: return SwitchSaveOutcome.Failed
        // #624 评论10 第1/2项：每次循环都从真实 Rust session 重新签发权威 lease
        // （snapshot 缺失/错版 → null → 中止切章，不伪造空正文）。
        val lease = coordinator.issueDocumentOperationLease(oldTargetId) ?: return SwitchSaveOutcome.Failed
        if (!lease.localDirty) return SwitchSaveOutcome.Committed
        val savedHash =
            if (lease.text.isEmpty()) {
                clearChapterContentForSwitch(oldSession, lease)
            } else {
                saveChapterContentForSwitch(oldSession, lease.text, lease)
            }
        if (savedHash == null) return SwitchSaveOutcome.Failed
        // 回执已记录；只有 lease 仍匹配且最新 revision 已落盘才算事务完成 —
        // 否则循环重新签发最新 snapshot 再保存。
        if (commitSwitchSave(lease, savedHash)) return SwitchSaveOutcome.Committed
    }
}

/**
 * 保存旧章节正文。返回落盘 contentHash（Repository Success）；失败返回 null
 * （saveStatus=SaveFailed + 错误事件已上报）。
 */
private suspend fun EditorViewModel.saveChapterContentForSwitch(
    session: EditorSession,
    content: String,
    lease: DocumentOperationLease,
): String? =
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
                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> result.data?.contentHash ?: ""
                is com.xiwei.sujian.core.interop.common.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_failed, result.message),
                    )
                    null
                }
                com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_native_not_loaded),
                    )
                    null
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
            emitErrorEvent(
                getApplication<Application>().getString(R.string.error_save_exception, e.message ?: ""),
            )
            null
        }
    }

/**
 * 清空旧章节正文（dirty+空正文）。返回落盘 contentHash（Repository Success）；
 * 失败返回 null（saveStatus=SaveFailed + 错误事件已上报）。
 */
private suspend fun EditorViewModel.clearChapterContentForSwitch(
    session: EditorSession,
    lease: DocumentOperationLease,
): String? =
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
                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> result.data?.contentHash ?: ""
                is com.xiwei.sujian.core.interop.common.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    emitErrorEvent(
                        getApplication<Application>().getString(R.string.error_save_failed, result.message),
                    )
                    null
                }
                com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    null
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
            null
        }
    }

/** #595 一：加载/session 预准备失败回滚 — 恢复旧 EditorUiState 与输入冻结。 */
private fun EditorViewModel.rollbackAfterLoadFailure(
    oldSession: EditorSession?,
    oldUiState: EditorUiState,
) {
    currentSession = oldSession
    _uiState.value = oldUiState.copy(loading = false)
    setInputFrozen(false)
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

/** #595 一：阶段 1 — 保存旧章节（稳定 lease 事务，含回执/过期回滚）。null 表示继续。 */
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
    // #624 评论13 第2项：persistOldChapterForSwitch 循环签发权威 lease，只有最新
    // revision 真正落盘（Committed）才继续加载新章节。
    when (persistOldChapterForSwitch(ctx, oldSession)) {
        SwitchSaveOutcome.Committed -> {
            // #595 一：可见提交边界 1 — 保存完成后若已有更新请求，回滚并退出。
            if (!ctx.isLatest()) {
                restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
                return ChapterSwitchResult.Stale
            }
            return null
        }
        SwitchSaveOutcome.Stale -> {
            restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
            return ChapterSwitchResult.Stale
        }
        SwitchSaveOutcome.Failed -> {
            // #595 一：保存失败返回明确失败结果，且完整恢复旧 EditorUiState。
            _uiState.value = ctx.oldUiState.copy(loading = false, saveStatus = SaveStatus.SaveFailed)
            saveCommandChannel = Channel<SaveCommand>(Channel.UNLIMITED)
            startSaveActor()
            setInputFrozen(false)
            return ChapterSwitchResult.SaveFailed(oldSession)
        }
    }
}

/**
 * #624 评论14 第2项：章节切换加载结果 — 纯读取数据，不携带副作用。
 * [switchLoadAndPrepare] 只读 Repository 构造此对象，不写 currentSession/_uiState/不 emit fact；
 * [switchCommit] commit 成功后才用此对象发布 B 的可见状态。
 */
private data class LoadedChapterForSwitch(
    val session: EditorSession,
    val text: String,
    val meta: com.xiwei.sujian.feature.project.data.model.ChapterMeta,
    val wordCount: Int,
    val fact: TargetDocumentFact,
)

private sealed interface SwitchPrepareOutcome {
    data class Ready(
        val session: EditorSession,
        val handle: com.xiwei.sujian.feature.editor.session.PreparedSessionHandle,
        val loaded: LoadedChapterForSwitch,
    ) : SwitchPrepareOutcome

    data class Aborted(val result: ChapterSwitchResult) : SwitchPrepareOutcome
}

/**
 * #624 评论14 第2项：纯读取加载 — 只读 Repository、算字数、构造 loaded data。
 * 不写 currentSession、不写 live _uiState、不 emit document fact。
 * A 保持 live current session（inputFrozen=true），B 在 commit 前不可见。
 */
private suspend fun EditorViewModel.loadChapterForSwitch(session: EditorSession): LoadedChapterForSwitch? {
    return try {
        val (content, meta) =
            withContext(Dispatchers.IO) {
                chapterRepository.getChapterContentWithMeta(
                    session.projectId,
                    session.volumeId,
                    session.chapterId,
                )
            }
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.chapterLoad(
            session.projectId,
            session.chapterId,
            content.toByteArray(Charsets.UTF_8).size,
            "ok",
        )
        val wordCount = calculateWordCount(content)
        val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
        val fact =
            TargetDocumentFact(
                targetId = targetId,
                text = content,
                sourceVersion = DocumentVersion(contentHash = meta.hash),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            )
        LoadedChapterForSwitch(session, content, meta, wordCount, fact)
    } catch (e: kotlinx.coroutines.CancellationException) {
        throw e
    } catch (e: Throwable) {
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.chapterLoad(
            session.projectId,
            session.chapterId,
            0,
            "error",
        )
        null
    }
}

/**
 * #595 一：阶段 2 — 加载新章节 + 无副作用预准备 Rust session。
 * #624 评论14 第2项：不提前发布 B — 不写 currentSession、不写 _uiState、不 emit fact。
 * A 保持 live current session（inputFrozen=true），B 只在 [switchCommit] commit 成功后才可见。
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
    // #624 评论14 第2项：不写 currentSession、不写 _uiState — A 保持 live current session。
    val loaded = loadChapterForSwitch(newSession)
    if (loaded == null) {
        // #624 评论15 问题1：完整回滚 — 恢复 currentSession、重建 channel、
        // 启动 save actor、恢复 autosave、解除 inputFrozen。旧实现只恢复 _uiState，
        // 导致 inputFrozen 保持 true、新 channel 无 actor 消费（输入被丢弃、自动保存停止）。
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
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
    val handle = prepareTargetSession(chapterTargetId(ctx.projectId, ctx.volumeId, ctx.chapterId), loaded.text)
    if (handle == null) {
        // #624 评论15 问题1：完整回滚（同 loadChapterForSwitch 失败路径）。
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
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
    return SwitchPrepareOutcome.Ready(newSession, handle, loaded)
}

/**
 * #595 一：最终提交 — 一次性执行 A→B 切换；失败回滚到旧章节。
 * #624 评论14 第2项：commit 成功后才发布 B（写 currentSession/_uiState/emit fact）—
 * B 在 commit 前对 WritingPane 不可见，避免提前 beginEdit(B)/消费 REPOSITORY_LOAD fact。
 */
private suspend fun EditorViewModel.switchCommit(
    ctx: SwitchContext,
    newSession: EditorSession,
    handle: com.xiwei.sujian.feature.editor.session.PreparedSessionHandle,
    loaded: LoadedChapterForSwitch,
): ChapterSwitchResult {
    val coordinator = _sessionCoordinator
    if (coordinator == null || !coordinator.commitPreparedSession(handle)) {
        // #624 评论15 问题1：commit 失败先回滚预准备 session，再完整回滚旧章节状态
        // （恢复 currentSession、重建 channel、启动 save actor、恢复 autosave、解除 inputFrozen）。
        rollbackPreparedSession(handle)
        restoreAfterSwitch(ctx.oldSession, ctx.oldUiState)
        return ChapterSwitchResult.LoadFailed(ChapterKey(ctx.projectId, ctx.volumeId, ctx.chapterId))
    }

    // #624 评论14 第2项：commit 成功后才发布 B — 写 currentSession/_uiState/emit fact。
    currentSession = newSession
    _uiState.value =
        _uiState.value.copy(
            loading = false,
            content = loaded.text,
            chapterHash = loaded.meta.hash,
            chapterNote = loaded.meta.note,
            chapterTitle = ctx.chapterTitle,
            editorEnabled = true,
            saveStatus = SaveStatus.Idle,
            wordCount = loaded.wordCount,
        )
    initialWordCount = loaded.wordCount
    sessionStartTime = System.currentTimeMillis()
    startSaveActor()
    reloadSettings()
    // 提交后把 loaded fact 交给 applyExternalContentFact 建立 committedVersion，再 emit 到 replay bus。
    // #624 评论15 问题2：只在提交后 Core snapshot 与 loaded.text 是同一个文档事实时才
    // applyExternalContentFact — 防御性检查：prepare 已做正文比较（复用时 snapshot==loaded.text，
    // candidate swap 时 candidate 装入 loaded.text），commit 不修改 session 正文，所以正常路径
    // 总是一致。若异常路径导致不一致，不得把 committedVersion 标成新版本而 Rust 正文还是旧版本
    // （三份状态分裂）。emitDocumentFact 仍无条件发，让 replay bus 消费者走 shouldApplyExternalContent
    // → Apply → resetPersistentSession 修正。
    val committedTargetId = chapterTargetId(ctx.projectId, ctx.volumeId, ctx.chapterId)
    val committedSnapshot = coordinator.queryTargetSnapshot(committedTargetId)
    if (committedSnapshot != null && committedSnapshot.text == loaded.text) {
        coordinator.applyExternalContentFact(loaded.fact)
    }
    emitDocumentFact(loaded.fact)
    updateStats()

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

    setInputFrozen(true)
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
                return switchCommit(ctx, outcome.session, outcome.handle, outcome.loaded)
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
    setInputFrozen(false)
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

fun EditorViewModel.initErrorState(errorMessage: String) {
    _uiState.value =
        _uiState.value.copy(
            loading = false,
            content = errorMessage,
            editorEnabled = false,
            saveStatus = SaveStatus.Idle,
        )
}

suspend fun EditorViewModel.applyExternalContentToUi(
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
    // #624 评论12 第2项：dirty 唯一真值在 session store — 外部 reset 由
    // applyExternalContentFact/resetPersistentSession 清 localDirty，不再有 ViewModel 第二份。
    // #624 评论13 第3项：同步合并正文已经是磁盘事实 — 不记录回执（旧 buildSaveToken
    // 会拼出 targetId + "此刻 currentInputLease" 的假身份）。回执跟踪器只记录真实
    // Save/Clear 操作使用的 lease.toSaveToken。
    // #624 评论9：previousText 已删除 — 统计改增量 recordWritingEvent。
    // 冷路径 external-apply：用整章 text 重算 wordCount 并设入 _uiState，再 updateStats() 算 speed。
    // #624 评论13 第4项：calculateWordCount 是 suspend（Repository 自己 main-safe）。
    _uiState.value = _uiState.value.copy(wordCount = calculateWordCount(text))
    updateStats()
}

fun EditorViewModel.notifySyncMergeConflict() {
    editorScope.launch {
        emitErrorEvent(getApplication<Application>().getString(R.string.error_sync_document_conflict))
    }
}
