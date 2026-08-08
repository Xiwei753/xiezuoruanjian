package com.xiwei.sujian.feature.editor

// ! # 编辑器保存操作（从 EditorViewModel 拆分）
// !
// ! 自动保存调度、保存命令 actor、flush 屏障、清空文档、保存回执记录。

import android.app.Application
import com.xiwei.sujian.R
import com.xiwei.sujian.core.interop.project.DocumentSaveReceiptTracker
import com.xiwei.sujian.feature.editor.session.DocumentVersion
import com.xiwei.sujian.feature.editor.session.documentCommittedVersionFor
import com.xiwei.sujian.feature.editor.session.markSaved
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.withLock

fun EditorViewModel.buildSaveToken(
    targetId: String,
    revision: Long,
    hash: String,
): DocumentSaveReceiptTracker.SaveToken {
    val lease = _sessionCoordinator?.currentInputLease()
    return DocumentSaveReceiptTracker.SaveToken(
        operationId = 0L,
        targetId = targetId,
        coreSessionId = lease?.sessionId ?: 0UL,
        inputEpoch = lease?.epoch ?: 0L,
        rustRevision = revision,
        textHash = hash,
    )
}

fun EditorViewModel.scheduleAutoSave(content: String) {
    val session = currentSession ?: return
    autoSaveJob?.cancel()
    autoSaveJob =
        editorScope.launch {
            val delayMs = _uiState.value.settings.autoSaveDelayMs
            if (!_uiState.value.settings.autoSaveEnabled) return@launch
            delay(delayMs)
            if (_uiState.value.saveStatus == SaveStatus.Unsaved) {
                // #595 七：保存命令携带入队时的 Rust session revision — 回执按
                // (target, revision) 记录，Flush 屏障据此验证 revision 对应正文已落盘。
                val revision = _sessionCoordinator?.sessionState?.revision ?: 0L
                if (content.trim().isEmpty() && contentExplicitlyCleared) {
                    saveCommandChannel.trySend(SaveCommand.Clear(session, revision))
                } else if (content.trim().isNotEmpty()) {
                    saveCommandChannel.trySend(SaveCommand.Save(content, session, revision))
                }
            }
        }
}

// #597 保存请求需校验 lease/session/revision 多重前置条件后签发，拆分会破坏 lease 语义 — 待后续重构
fun EditorViewModel.requestSave(): kotlinx.coroutines.Deferred<Boolean> {
    val session = currentSession
    val deferred = kotlinx.coroutines.CompletableDeferred<Boolean>()
    // #595 二：签发文档操作租约 — 一次性取得完整不可变文档快照，
    // 不再拼接 currentSession + sessionState 两个独立状态源。
    val lease = _sessionCoordinator?.issueDocumentOperationLease()
    val content = lease?.text ?: _uiState.value.content
    val requiredRevision = lease?.rustRevision ?: 0L
    if (!validateSaveLease(session, lease, deferred)) {
        return deferred
    }
    editorScope.launch {
        deferred.complete(dispatchSaveCommand(session, content, requiredRevision))
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
    lease: com.xiwei.sujian.feature.editor.session.DocumentOperationLease?,
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
    content: String,
    requiredRevision: Long,
): Boolean {
    if (session == null) {
        // 无活动章节：没有本地输入需要保护。
        return true
    }
    val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
    if (content.trim().isEmpty()) {
        if (contentExplicitlyCleared) {
            if (saveCommandChannel.trySend(SaveCommand.Clear(session, requiredRevision)).isFailure) {
                return false
            }
        } else if (contentDirty) {
            // 编辑过但未确认清空 — 磁盘与屏幕不一致，不得报告假成功。
            return false
        }
    } else {
        if (saveCommandChannel.trySend(SaveCommand.Save(content, session, requiredRevision)).isFailure) {
            return false
        }
    }
    val flushReply = CompletableDeferred<Boolean>()
    if (saveCommandChannel.trySend(
            SaveCommand.Flush(targetId, session.sessionId, requiredRevision, flushReply),
        ).isFailure
    ) {
        return false
    }
    return flushReply.await()
}

fun EditorViewModel.clearChapterContent() {
    val session = currentSession ?: return
    contentExplicitlyCleared = true
    val revision = _sessionCoordinator?.sessionState?.revision ?: 0L
    saveCommandChannel.trySend(SaveCommand.Clear(session, revision))
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
                                cmd.content, cmd.session, isAutoSave = true,
                                revisionAtEnqueue = cmd.revisionAtEnqueue,
                            )
                        }
                    }
                    is SaveCommand.Clear -> {
                        if (cmd.session.sessionId == currentSession?.sessionId) {
                            clearChapterContentInternal(cmd.session, cmd.revisionAtEnqueue)
                        }
                    }
                    is SaveCommand.Flush -> {
                        // #595 二/七：Flush 是指定 target 和 revision 的持久化屏障 —
                        // 只有确认该 revision 对应正文已经得到保存回执（且与
                        // committedVersion 一致）才返回成功。删除跨章节全局 lastSaveResult。
                        // #595 二：再次确认活动文档仍基于该 snapshot — 保存后又输入会让
                        // sessionState.revision 前进，不再等于 requiredRustRevision，
                        // flush 失败，同步中止（旧实现只比较回执 revision，不确认当前活动 revision）。
                        val committed = _sessionCoordinator?.documentCommittedVersionFor(cmd.targetId)
                        val receiptOk =
                            saveReceipts.canFlush(
                                buildSaveToken(cmd.targetId, cmd.requiredRustRevision, committed?.contentHash ?: ""),
                                committed?.contentHash,
                            )
                        val currentRevision = _sessionCoordinator?.sessionState?.revision ?: 0L
                        cmd.reply.complete(receiptOk && currentRevision == cmd.requiredRustRevision)
                    }
                }
            }
        }
}

suspend fun EditorViewModel.clearChapterContentInternal(
    session: EditorSession,
    revisionAtEnqueue: Long = 0L,
): Boolean {
    return saveMutex.withLock {
        try {
            val result = projectRepository.clearChapterContent(session.projectId, session.volumeId, session.chapterId)
            when (result) {
                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> {
                    val savedHash = result.data?.contentHash ?: ""
                    _uiState.value =
                        _uiState.value.copy(
                            content = "",
                            chapterHash = savedHash,
                            saveStatus = SaveStatus.Saved,
                        )
                    previousText = ""
                    // #595 三：清空落盘成功后统一清理 dirty/cleared — 否则下次同步
                    // requestSave 仍见 contentExplicitlyCleared=true 重复发送 Clear，
                    // 再次触发空覆盖保护（旧实现分散在多套可写状态未统一清理）。
                    contentDirty = false
                    contentExplicitlyCleared = false
                    val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
                    // #595 七：清空落盘后记录回执（revision 精确锚定）。
                    saveReceipts.record(buildSaveToken(targetId, revisionAtEnqueue, savedHash))
                    // #595 二/六：保存成功上报 — 保存回执作为文档提交原子推进
                    // committed/sessionBase/lastSaved + 清除 localDirty，
                    // 同步合并以磁盘版本为基础可安全应用。
                    _sessionCoordinator?.markSaved(
                        targetId,
                        DocumentVersion(contentHash = savedHash),
                    )
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

// #597：保存循环与单次落盘分离 — 单次尝试（lease 校验→内容保存→回执记录→
// 状态更新）收敛到 saveChapterAttempt，performSave 只负责 pending 重试循环；
// 保存期间继续输入（pending）时按新 revision 重试，晚到回执不覆盖新输入。
private sealed interface SaveAttemptResult {
    data class Finished(val success: Boolean) : SaveAttemptResult

    data class RetryPending(val content: String) : SaveAttemptResult
}

private suspend fun EditorViewModel.saveChapterAttempt(
    contentToSave: String,
    session: EditorSession,
    isAutoSave: Boolean,
    currentRevision: Long,
    saveStartedAt: Long,
): SaveAttemptResult {
    val currentState = _uiState.value
    if (currentState.saveStatus == SaveStatus.Saving) {
        pendingSaveContent = contentToSave
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
                handleSaveSuccess(result, contentToSave, session, currentRevision, saveStartedAt)
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
    result: com.xiwei.sujian.core.interop.common.BridgeResult.Success<com.xiwei.sujian.core.model.ChapterSaveReceipt>,
    contentToSave: String,
    session: EditorSession,
    currentRevision: Long,
    saveStartedAt: Long,
): SaveAttemptResult {
    val savedHash = result.data?.contentHash ?: ""
    com.xiwei.sujian.app.diagnostics.DiagnosticsEvents.chapterSave(
        session.projectId,
        session.chapterId,
        contentToSave.toByteArray(Charsets.UTF_8).size,
        "ok",
        System.currentTimeMillis() - saveStartedAt,
    )
    val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
    // #595 七：保存落盘后记录回执（revision 精确锚定）。
    saveReceipts.record(buildSaveToken(targetId, currentRevision, savedHash))
    // #595 三：保存回执按 revision 条件提交 — 只有当前活动 revision 仍等于
    // 保存时的 revision 才标记 Saved、清 dirty、markSaved。用户在保存 IO 期间
    // 继续输入（revision 前进）时，只记录回执，不覆盖新输入产生的 UI 状态/
    // dirty/chapterHash（旧实现无条件设 Saved，页面错误显示"已保存"，B 未落盘）。
    val activeRevision = _sessionCoordinator?.sessionState?.revision ?: currentRevision
    if (activeRevision == currentRevision) {
        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.Saved, chapterHash = savedHash)
        _sessionCoordinator?.markSaved(targetId, DocumentVersion(contentHash = savedHash))
        contentDirty = false
    } else {
        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.Unsaved)
    }
    val pending = pendingSaveContent
    pendingSaveContent = null
    return if (pending != null && pending != contentToSave) {
        SaveAttemptResult.RetryPending(pending)
    } else {
        SaveAttemptResult.Finished(true)
    }
}

private suspend fun EditorViewModel.handleSaveError(
    result: com.xiwei.sujian.core.interop.common.BridgeResult.Error,
    contentToSave: String,
    session: EditorSession,
    isAutoSave: Boolean,
    saveStartedAt: Long,
): SaveAttemptResult {
    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
    com.xiwei.sujian.app.diagnostics.DiagnosticsEvents.chapterSave(
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
    com.xiwei.sujian.app.diagnostics.DiagnosticsEvents.chapterSave(
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
    isAutoSave: Boolean,
    revisionAtEnqueue: Long = 0L,
): Boolean {
    if (content.trim().isEmpty()) {
        if (contentExplicitlyCleared) {
            return clearChapterContentInternal(session, revisionAtEnqueue)
        }
        return false
    }
    var currentContent = content
    var currentIsAutoSave = isAutoSave
    var currentRevision = revisionAtEnqueue
    val saveStartedAt = System.currentTimeMillis()
    while (true) {
        val contentToSave = currentContent
        val attempt =
            saveMutex.withLock {
                saveChapterAttempt(contentToSave, session, currentIsAutoSave, currentRevision, saveStartedAt)
            }
        when (attempt) {
            is SaveAttemptResult.Finished -> return attempt.success
            is SaveAttemptResult.RetryPending -> {
                currentContent = attempt.content
                currentIsAutoSave = true
                currentRevision = _sessionCoordinator?.sessionState?.revision ?: currentRevision
            }
        }
    }
}
