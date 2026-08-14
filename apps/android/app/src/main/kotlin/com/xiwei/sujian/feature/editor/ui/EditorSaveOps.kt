package com.xiwei.sujian.feature.editor.ui

// ! # 编辑器保存操作（从 EditorViewModel 拆分）
// !
// ! 自动保存调度、保存命令 actor、flush 屏障、清空文档、保存回执记录。

import android.app.Application
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.session.DocumentSaveReceiptTracker
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

/**
 * #624 评论10 第2项：ViewModel 销毁（onCleared）兜底保存 — 从真实 Rust
 * snapshot/lease 按 target 取 text + revision，不回退 `_uiState.content`
 * （评论9 后本地输入不再更新它 — 保存旧正文会把刚输入的内容覆盖回磁盘）。
 *
 * snapshot 不可得（session 未激活/已关闭/快照缺失/错版）时跳过保存：正文
 * 要么已由对应关闭路径落盘，要么 session 保留可再保存；绝不伪造空正文。
 * 保存端口为 suspend 契约，此处用 runBlocking 保持原有生命周期语义
 * （不延迟进程退出，不依赖仍会被取消的 viewModelScope）。
 */
internal fun EditorViewModel.saveFallbackOnClear(session: EditorSession) {
    val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
    val lease = _sessionCoordinator?.issueDocumentOperationLease(targetId) ?: return
    kotlinx.coroutines.runBlocking {
        // #624 评论11 第2项：统一按 contentDirty + 有效 lease.text 决定 —
        // dirty + 空正文 → Clear（用户删空了，磁盘旧正文必须清掉）；
        // 未 dirty → 无事可做。
        if (lease.text.isNotEmpty()) {
            effectiveChapterSavePort.saveChapterContent(
                session.projectId,
                session.volumeId,
                session.chapterId,
                lease.text,
            )
        } else if (contentDirty) {
            chapterRepository.clearChapterContent(session.projectId, session.volumeId, session.chapterId)
        }
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

/**
 * #624 评论11 第2项：自动保存统一按 `contentDirty + 有效 lease.text` 决定：
 * `!contentDirty` 无事可做；dirty 且空正文发 Clear；dirty 且非空发 Save。
 * 不再维护 contentExplicitlyCleared 布尔侧信道。
 */
private fun EditorViewModel.dispatchAutoSaveCommand(
    lease: com.xiwei.sujian.feature.editor.session.DocumentOperationLease,
    session: EditorSession,
) {
    if (!contentDirty) return
    if (lease.text.isEmpty()) {
        saveCommandChannel.trySend(SaveCommand.Clear(session, lease.rustRevision))
    } else {
        saveCommandChannel.trySend(SaveCommand.Save(lease.text, session, lease.rustRevision))
    }
}

// #597 保存请求需校验 lease/session/revision 多重前置条件后签发，拆分会破坏 lease 语义 — 待后续重构
fun EditorViewModel.requestSave(): kotlinx.coroutines.Deferred<Boolean> {
    val session = currentSession
    val deferred = kotlinx.coroutines.CompletableDeferred<Boolean>()
    // #595 二：签发文档操作租约 — 一次性取得完整不可变文档快照，
    // 不再拼接 currentSession + sessionState 两个独立状态源。
    val lease = _sessionCoordinator?.issueDocumentOperationLease()
    // #624 评论10 第1项：lease null（snapshot 缺失/错版）时不回退到 _uiState.value.content —
    // UI 冷路径正文不是真值，用它保存会导致数据丢失。返回失败，保持 Unsaved。
    if (lease == null) {
        deferred.complete(false)
        return deferred
    }
    val content = lease.text
    val requiredRevision = lease.rustRevision
    if (!validateSaveLease(session, lease, deferred)) {
        return deferred
    }
    // #624 评论11 第2项：未编辑（!contentDirty）→ 无事可做，直接成功 —
    // 不发保存/清空命令，不触碰 saveStatus。
    if (!contentDirty) {
        deferred.complete(true)
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
    // #624 评论11 第2项：dirty+空正文 → Clear（用户删空后必须真正清掉磁盘旧正文，
    // 不得停留在假失败）；dirty+非空 → Save。
    if (content.isEmpty()) {
        if (saveCommandChannel.trySend(SaveCommand.Clear(session, requiredRevision)).isFailure) {
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
    // #624 评论11 第2项：显式清空本身就是 Clear 语义 — 直接发命令，不再写布尔侧信道。
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
            val result = chapterRepository.clearChapterContent(session.projectId, session.volumeId, session.chapterId)
            when (result) {
                is com.xiwei.sujian.core.interop.common.BridgeResult.Success -> {
                    val savedHash = result.data?.contentHash ?: ""
                    _uiState.value =
                        _uiState.value.copy(
                            content = "",
                            chapterHash = savedHash,
                            saveStatus = SaveStatus.Saved,
                        )
                    // #624 评论9：previousText 已删除 — 不再维护整章 String 缓存。
                    // #595 三：清空落盘成功后统一清理 dirty — 否则下次同步
                    // requestSave 仍见 dirty=true 重复发送 Clear。
                    contentDirty = false
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

// #597 / #624 评论9：保存循环与单次落盘分离 — 单次尝试（lease 校验→内容保存→
// 回执记录→状态更新）收敛到 saveChapterAttempt。pendingSaveContent 已删除：
// 保存期间继续输入只标记 contentDirty=true，下次 autoSave/requestSave 重新签发 lease。
private sealed interface SaveAttemptResult {
    data class Finished(val success: Boolean) : SaveAttemptResult
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
        // #624 评论9：pendingSaveContent 已删除 — 保存期间继续输入只标记 contentDirty=true，
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
    result: com.xiwei.sujian.core.interop.common.BridgeResult.Success<
        com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt,
        >,
    contentToSave: String,
    session: EditorSession,
    currentRevision: Long,
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
    // #624 评论9：pendingSaveContent 已删除 — 保存期间继续输入由 contentDirty 标记，
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
    isAutoSave: Boolean,
    revisionAtEnqueue: Long = 0L,
): Boolean {
    // #624 评论1："\n"/连续空行/纯空白段落是用户正文，原样保存；
    // 只有真正的空字符串才触发清空语义。
    if (content.isEmpty()) {
        // #624 评论11 第2项：dirty+空正文 → Clear（用户删空）；未 dirty 的空正文不动作。
        if (contentDirty) {
            return clearChapterContentInternal(session, revisionAtEnqueue)
        }
        return false
    }
    // #624 评论9：pendingSaveContent 已删除 — 不再重试循环，单次尝试即返回。
    val saveStartedAt = System.currentTimeMillis()
    val attempt =
        saveMutex.withLock {
            saveChapterAttempt(content, session, isAutoSave, revisionAtEnqueue, saveStartedAt)
        }
    return (attempt as SaveAttemptResult.Finished).success
}
