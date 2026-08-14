package com.xiwei.sujian.feature.editor.presentation

// ! # 编辑器保存操作（从 EditorViewModel 拆分）
// !
// ! 自动保存调度、保存命令 actor、flush 屏障、清空文档、保存回执记录。
// !
// ! #624 评论12 第2项：dirty 唯一真值在 session store（applyLocalEdit 写入），
// ! issueDocumentOperationLease 从记录填入 lease.localDirty。所有保存入口
// ! （autosave / requestSave / 显式清空 / 切章保存）只消费 lease 决策：
// !
// ! ```text
// ! when {
// !     !lease.localDirty -> NoOp        // 未编辑 — 无事可做，不重写磁盘
// !     lease.text.isEmpty() -> Clear    // 用户删空 — 必须清掉磁盘旧正文
// !     else -> Save(lease.text)         // 有正文 — 原样保存
// ! }
// ! ```
// !
// ! 保存完成统一走 [EditorViewModel.commitSaveSuccess]（回执 + markSaved 原子
// ! 提交），不再有第二套保存语义。

import android.app.Application
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.session.DocumentOperationLease
import com.xiwei.sujian.feature.editor.session.DocumentVersion
import com.xiwei.sujian.feature.editor.session.commitSavedLease
import com.xiwei.sujian.feature.editor.session.documentCommittedVersionFor
import com.xiwei.sujian.feature.editor.session.toSaveToken
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.withLock

/**
 * #624 评论12 第2项：保存完成统一提交 — 输入必须是本次 [DocumentOperationLease]
 * + Repository 返回的 contentHash：
 *
 * 1. 用 lease 字段直接构造 SaveToken 记录回执（不重读\"此刻的 currentInputLease\"）；
 * 2. 活动文档仍是同一 target/session/epoch/revision 时才 markSaved 原子推进
 *    committedVersion/sessionBaseVersion/lastSavedVersion 并清 localDirty
 *    （保存期间继续输入导致 revision 前进时不得把新输入误标为已落盘）；
 * 3. 返回是否完成提交 — 调用方据此设置 UI saveStatus（Saved / Unsaved）。
 */
private fun EditorViewModel.commitSaveSuccess(
    lease: DocumentOperationLease,
    hash: String,
): Boolean {
    saveReceipts.record(lease.toSaveToken(hash))
    val coordinator = _sessionCoordinator ?: return false
    // #624 评论16 问题2：原子提交 — commitSavedLease 一次完成校验 + markSaved，
    // 不再先 isDocumentOperationLeaseCurrent() 再 markSaved()（两步操作有竞态窗口）。
    return coordinator.commitSavedLease(lease, DocumentVersion(contentHash = hash))
}

/**
 * #624 评论12 第2项：自动保存统一按 `lease.localDirty + lease.text` 决定：
 * `!localDirty` 无事可做；dirty 且空正文发 Clear；dirty 且非空发 Save。
 */
private fun EditorViewModel.dispatchAutoSaveCommand(
    lease: DocumentOperationLease,
    session: EditorSession,
) {
    when {
        !lease.localDirty -> Unit
        lease.text.isEmpty() -> saveCommandChannel.trySend(SaveCommand.Clear(session, lease))
        else -> saveCommandChannel.trySend(SaveCommand.Save(lease.text, session, lease))
    }
}

fun EditorViewModel.scheduleAutoSave() {
    val session = currentSession ?: return
    autoSaveJob?.cancel()
    autoSaveJob =
        editorScope.launch {
            val delayMs = _uiState.value.settings.autoSaveDelayMs
            if (!_uiState.value.settings.autoSaveEnabled) return@launch
            delay(delayMs)
            if (_uiState.value.saveStatus != SaveStatus.Unsaved) return@launch
            // #624 评论10 第1项：正文从 Core snapshot lease 取（冷路径）。
            // lease null（snapshot 缺失/错版）时不回退到 "" — 保持 Unsaved，
            // 不发任何保存命令，绝不误触发 Clear。
            val lease = _sessionCoordinator?.issueDocumentOperationLease() ?: return@launch
            dispatchAutoSaveCommand(lease, session)
        }
}

// #597 保存请求需校验 lease/session/revision 多重前置条件后签发，拆分会破坏 lease 语义 — 待后续重构
fun EditorViewModel.requestSave(): kotlinx.coroutines.Deferred<Boolean> {
    val session = currentSession
    val deferred = kotlinx.coroutines.CompletableDeferred<Boolean>()
    // 无活动章节：没有本地输入需要保护（ActiveDocumentGate flush 的 no-op 路径）。
    if (session == null) {
        deferred.complete(true)
        return deferred
    }
    // #595 二：签发文档操作租约 — 一次性取得完整不可变文档快照，
    // 不再拼接 currentSession + sessionState 两个独立状态源。
    val lease = _sessionCoordinator?.issueDocumentOperationLease()
    // #624 评论10 第1项：lease null（snapshot 缺失/错版）时不回退到 _uiState.value.content —
    // UI 冷路径正文不是真值，用它保存会导致数据丢失。返回失败，保持 Unsaved。
    if (lease == null) {
        deferred.complete(false)
        return deferred
    }
    if (!validateSaveLease(session, lease, deferred)) {
        return deferred
    }
    // #624 评论12 第2项：未编辑（!lease.localDirty）→ 无事可做，直接成功 —
    // 不发保存/清空命令，不触碰 saveStatus。
    if (!lease.localDirty) {
        deferred.complete(true)
        return deferred
    }
    editorScope.launch {
        deferred.complete(dispatchSaveCommand(session, lease))
    }
    return deferred
}

/**
 * #595 二：lease 校验 — currentSession 的 target 必须与 lease 的 target 一致，
 * 且 lease 的 session/epoch 仍有效。任一不匹配返回失败，不拼接字段。
 * 旧实现组合 currentSession（ViewModel 字段）与全局 sessionState（Coordinator
 * StateFlow）两个独立状态源，交错时形成 A 正文 → B 章节的错误保存。
 */
private fun EditorViewModel.validateSaveLease(
    session: EditorSession?,
    lease: DocumentOperationLease?,
    deferred: kotlinx.coroutines.CompletableDeferred<Boolean>,
): Boolean {
    if (session == null || lease == null) return true
    val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
    if (lease.targetId != targetId || !_sessionCoordinator!!.isDocumentOperationLeaseCurrent(lease)) {
        deferred.complete(false)
        return false
    }
    return true
}

/** 派发保存命令（Save/Clear/Flush）并等待 flush 屏障返回最终结果。 */
private suspend fun EditorViewModel.dispatchSaveCommand(
    session: EditorSession?,
    lease: DocumentOperationLease,
): Boolean {
    if (session == null) {
        // 无活动章节：没有本地输入需要保护。
        return true
    }
    // #624 评论12 第2项：Save/Clear 决策统一按 lease.localDirty + lease.text —
    // dirty+空正文 → Clear（用户删空后必须真正清掉磁盘旧正文）；
    // dirty+非空 → Save；未 dirty → 无事可做（不重写磁盘）。
    when {
        !lease.localDirty -> return true
        lease.text.isEmpty() -> {
            if (saveCommandChannel.trySend(SaveCommand.Clear(session, lease)).isFailure) {
                return false
            }
        }
        else -> {
            if (saveCommandChannel.trySend(SaveCommand.Save(lease.text, session, lease)).isFailure) {
                return false
            }
        }
    }
    val flushReply = CompletableDeferred<Boolean>()
    if (saveCommandChannel.trySend(SaveCommand.Flush(lease, flushReply)).isFailure) {
        return false
    }
    return flushReply.await()
}

fun EditorViewModel.clearChapterContent() {
    val session = currentSession ?: return
    // #624 评论12 第2项：显式清空本身就是 Clear 语义 — 直接发命令，不再写布尔侧信道；
    // 命令携带本次权威 lease（快照不可得时拒绝清空 — 不伪造空正文）。
    val lease = _sessionCoordinator?.issueDocumentOperationLease() ?: return
    saveCommandChannel.trySend(SaveCommand.Clear(session, lease))
}

fun EditorViewModel.startSaveActor() {
    saveActorJob?.cancel()
    saveActorJob =
        editorScope.launch(kotlinx.coroutines.Dispatchers.IO) {
            for (cmd in saveCommandChannel) {
                when (cmd) {
                    is SaveCommand.Save -> {
                        if (cmd.session.sessionId == currentSession?.sessionId) {
                            performSave(
                                cmd.content, cmd.session, cmd.lease, isAutoSave = true,
                            )
                        }
                    }
                    is SaveCommand.Clear -> {
                        if (cmd.session.sessionId == currentSession?.sessionId) {
                            clearChapterContentInternal(cmd.session, cmd.lease)
                        }
                    }
                    is SaveCommand.Flush -> {
                        // #595 二/七：Flush 是指定 target 和 revision 的持久化屏障 —
                        // 只有确认该 revision 对应正文已经得到保存回执（且与
                        // committedVersion 一致）才返回成功。删除跨章节全局 lastSaveResult。
                        // #595 二：再次确认活动文档仍基于该 snapshot — 保存后又输入会让
                        // sessionState.revision 前进，不再等于 lease.rustRevision，
                        // flush 失败，同步中止（旧实现只比较回执 revision，不确认当前活动 revision）。
                        // #624 评论12 第2项：请求 token 直接由本次 lease 构造，不重读当前输入 lease。
                        val committed = _sessionCoordinator?.documentCommittedVersionFor(cmd.lease.targetId)
                        val receiptOk =
                            saveReceipts.canFlush(
                                cmd.lease.toSaveToken(committed?.contentHash ?: ""),
                                committed?.contentHash,
                            )
                        val currentRevision = _sessionCoordinator?.sessionState?.revision ?: 0L
                        cmd.reply.complete(receiptOk && currentRevision == cmd.lease.rustRevision)
                    }
                }
            }
        }
}

suspend fun EditorViewModel.clearChapterContentInternal(
    session: EditorSession,
    lease: DocumentOperationLease,
): Boolean {
    return saveMutex.withLock {
        try {
            val result = chapterRepository.clearChapterContent(session.projectId, session.volumeId, session.chapterId)
            when (result) {
                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> {
                    val savedHash = result.data?.contentHash ?: ""
                    // #624 评论12 第2项：回执 + markSaved 统一提交（revision 匹配才清 dirty）。
                    val committed = commitSaveSuccess(lease, savedHash)
                    _uiState.value =
                        _uiState.value.copy(
                            content = "",
                            chapterHash = savedHash,
                            saveStatus = if (committed) SaveStatus.Saved else SaveStatus.Unsaved,
                        )
                    // #624 评论17 问题5：清空保存提交后同样检查 pendingExternal 重读。
                    if (committed) {
                        editorScope.launch(Dispatchers.IO) { checkSyncMergedChapter() }
                    }
                    true
                }
                is com.xiwei.sujian.core.interop.common.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    if (result.code == "EMPTY_OVERWRITE_BLOCKED") {
                        _events.send(
                            EditorEvent.ShowSaveFailedDialog(
                                getApplication<Application>().getString(R.string.error_empty_overwrite_dialog),
                            ),
                        )
                    } else {
                        _events.send(
                            EditorEvent.ShowSaveFailedDialog(
                                getApplication<Application>().getString(R.string.error_save_failed, result.message),
                            ),
                        )
                    }
                    false
                }
                com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    false
                }
            }
        } catch (e: Throwable) {
            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
            _events.send(
                EditorEvent.ShowSaveFailedDialog(
                    getApplication<Application>().getString(R.string.error_save_exception, e.message ?: ""),
                ),
            )
            false
        }
    }
}

// #597 / #624 评论9：保存循环与单次落盘分离 — 单次尝试（lease 校验→内容保存→
// 回执记录→状态更新）收敛到 saveChapterAttempt。pendingSaveContent 已删除：
// 保存期间继续输入只标记 dirty（session store），下次 autoSave/requestSave 重新签发 lease。
private sealed interface SaveAttemptResult {
    data class Finished(val success: Boolean) : SaveAttemptResult
}

private suspend fun EditorViewModel.saveChapterAttempt(
    contentToSave: String,
    session: EditorSession,
    lease: DocumentOperationLease,
    isAutoSave: Boolean,
    saveStartedAt: Long,
): SaveAttemptResult {
    val currentState = _uiState.value
    if (currentState.saveStatus == SaveStatus.Saving) {
        // #624 评论9：pendingSaveContent 已删除 — 保存期间继续输入只标记 dirty，
        // 下次 autoSave/requestSave 重新签发 lease（saveCommandChannel actor 已处理队列）。
        return SaveAttemptResult.Finished(false)
    }
    _uiState.value = currentState.copy(saveStatus = SaveStatus.Saving)
    return try {
        val result =
            effectiveChapterSavePort.saveChapterContent(
                session.projectId,
                session.volumeId,
                session.chapterId,
                contentToSave,
            )
        when (result) {
            is com.xiwei.sujian.core.interop.common.BridgeResult.Success ->
                handleSaveSuccess(result, contentToSave, session, lease, saveStartedAt)
            is com.xiwei.sujian.core.interop.common.BridgeResult.Error ->
                handleSaveError(result, contentToSave, session, isAutoSave, saveStartedAt)
            com.xiwei.sujian.core.interop.common.BridgeResult.NotLoaded ->
                handleSaveNotLoaded(contentToSave, session, isAutoSave, saveStartedAt)
        }
    } catch (e: Throwable) {
        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
        _events.send(
            EditorEvent.ShowSaveFailedDialog(
                getApplication<Application>().getString(R.string.error_save_exception, e.message ?: ""),
            ),
        )
        SaveAttemptResult.Finished(false)
    }
}

private fun EditorViewModel.handleSaveSuccess(
    result: com.xiwei.sujian.core.interop.common.BridgeResult.Success<
        com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt,
        >,
    contentToSave: String,
    session: EditorSession,
    lease: DocumentOperationLease,
    saveStartedAt: Long,
): SaveAttemptResult {
    val savedHash = result.data?.contentHash ?: ""
    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.chapterSave(
        session.projectId,
        session.chapterId,
        contentToSave.toByteArray(Charsets.UTF_8).size,
        "ok",
        System.currentTimeMillis() - saveStartedAt,
    )
    // #624 评论12 第2项：保存完成统一提交 — 回执总是记录（该 revision 确实已
    // 落盘）；只有活动文档仍是同一 lease/revision 才 markSaved + UI Saved。
    val committed = commitSaveSuccess(lease, savedHash)
    if (committed) {
        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.Saved, chapterHash = savedHash)
        // #624 评论17 问题5：commitSavedLease 真正提交后，如果 target 有
        // pendingExternal，重新从 Repository 读最新正文/hash 走
        // shouldApplyExternalContent（不拿缓存的旧正文覆盖刚保存的本地正文）。
        editorScope.launch(Dispatchers.IO) { checkSyncMergedChapter() }
    } else {
        // 用户在保存 IO 期间继续输入（revision 前进）— 只记录回执，不覆盖新输入
        // 产生的 UI 状态/chapterHash（旧实现无条件设 Saved，页面错误显示"已保存"）。
        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.Unsaved)
    }
    // #624 评论9：pendingSaveContent 已删除 — 保存期间继续输入由 dirty 标记，
    // 下次 autoSave/requestSave 重新签发 lease。不再返回 RetryPending。
    return SaveAttemptResult.Finished(true)
}

private suspend fun EditorViewModel.handleSaveError(
    result: com.xiwei.sujian.core.interop.common.BridgeResult.Error,
    contentToSave: String,
    session: EditorSession,
    isAutoSave: Boolean,
    saveStartedAt: Long,
): SaveAttemptResult {
    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.chapterSave(
        session.projectId,
        session.chapterId,
        contentToSave.toByteArray(Charsets.UTF_8).size,
        "error",
        System.currentTimeMillis() - saveStartedAt,
    )
    val blocked = result.code == "EMPTY_OVERWRITE_BLOCKED"
    val dialogMessage =
        when {
            blocked && !isAutoSave ->
                getApplication<Application>().getString(R.string.error_empty_overwrite_dialog)
            !blocked && !isAutoSave ->
                getApplication<Application>().getString(R.string.error_save_failed, result.message)
            else -> null
        }
    val toastMessage =
        when {
            blocked && isAutoSave ->
                getApplication<Application>().getString(R.string.error_empty_overwrite_save_blocked)
            !blocked && isAutoSave ->
                getApplication<Application>().getString(R.string.error_auto_save_failed, result.message)
            else -> null
        }
    reportSaveFailure(dialogMessage, toastMessage)
    return SaveAttemptResult.Finished(false)
}

private suspend fun EditorViewModel.handleSaveNotLoaded(
    contentToSave: String,
    session: EditorSession,
    isAutoSave: Boolean,
    saveStartedAt: Long,
): SaveAttemptResult {
    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.chapterSave(
        session.projectId,
        session.chapterId,
        contentToSave.toByteArray(Charsets.UTF_8).size,
        "not_loaded",
        System.currentTimeMillis() - saveStartedAt,
    )
    if (!isAutoSave) {
        _events.send(
            EditorEvent.ShowSaveFailedDialog(
                getApplication<Application>().getString(R.string.error_save_native_not_loaded),
            ),
        )
    }
    return SaveAttemptResult.Finished(false)
}

private suspend fun EditorViewModel.reportSaveFailure(
    dialogMessage: String?,
    toastMessage: String?,
) {
    if (dialogMessage != null) {
        _events.send(EditorEvent.ShowSaveFailedDialog(dialogMessage))
    } else if (toastMessage != null) {
        emitErrorEvent(toastMessage)
    }
}

suspend fun EditorViewModel.performSave(
    content: String,
    session: EditorSession,
    lease: DocumentOperationLease,
    isAutoSave: Boolean,
): Boolean {
    // #624 评论1："\n"/连续空行/纯空白段落是用户正文，原样保存；
    // 只有真正的空字符串才触发清空语义。
    if (content.isEmpty()) {
        // #624 评论12 第2项：dirty+空正文 → Clear（用户删空）；未 dirty 的空正文不动作。
        if (lease.localDirty) {
            return clearChapterContentInternal(session, lease)
        }
        return false
    }
    // #624 评论9：pendingSaveContent 已删除 — 不再重试循环，单次尝试即返回。
    val saveStartedAt = System.currentTimeMillis()
    val attempt =
        saveMutex.withLock {
            saveChapterAttempt(content, session, lease, isAutoSave, saveStartedAt)
        }
    return (attempt as SaveAttemptResult.Finished).success
}
