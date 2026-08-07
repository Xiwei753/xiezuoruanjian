package com.xiwei.sujian.ui

//! # 编辑器保存操作（从 EditorViewModel 拆分）
//!
//! 自动保存调度、保存命令 actor、flush 屏障、清空文档、保存回执记录。

import android.app.Application
import com.xiwei.sujian.R
import com.xiwei.sujian.data.DocumentSaveReceiptTracker
import com.xiwei.sujian.editor.v2.coordinator.DocumentVersion
import com.xiwei.sujian.editor.v2.coordinator.documentCommittedVersionFor
import com.xiwei.sujian.editor.v2.coordinator.markSaved
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.withLock

fun EditorViewModel.buildSaveToken(targetId: String, revision: Long, hash: String): DocumentSaveReceiptTracker.SaveToken {
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
    autoSaveJob = editorScope.launch {
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
@Suppress("CognitiveComplexMethod")
fun EditorViewModel.requestSave(): kotlinx.coroutines.Deferred<Boolean> {
    val session = currentSession
    val deferred = kotlinx.coroutines.CompletableDeferred<Boolean>()
    // #595 二：签发文档操作租约 — 一次性取得完整不可变文档快照，
    // 不再拼接 currentSession + sessionState 两个独立状态源。
    val lease = _sessionCoordinator?.issueDocumentOperationLease()
    val content = lease?.text ?: _uiState.value.content
    val requiredRevision = lease?.rustRevision ?: 0L
    // #595 二：lease 校验 — currentSession 的 target 必须与 lease 的 target 一致，
    // 且 lease 的 session/epoch 仍有效。任一不匹配返回失败，不拼接字段。
    // 旧实现组合 currentSession（ViewModel 字段）与全局 sessionState（Coordinator
    // StateFlow）两个独立状态源，交错时形成 A 正文 → B 章节的错误保存。
    if (session != null && lease != null) {
        val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
        if (lease.targetId != targetId ||
            !_sessionCoordinator!!.isDocumentOperationLeaseCurrent(lease)) {
            deferred.complete(false)
            return deferred
        }
    }
    editorScope.launch {
        if (session == null) {
            // 无活动章节：没有本地输入需要保护。
            deferred.complete(true)
            return@launch
        }
        val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
        if (content.trim().isEmpty()) {
            if (contentExplicitlyCleared) {
                val sendResult = saveCommandChannel.trySend(SaveCommand.Clear(session, requiredRevision))
                if (sendResult.isFailure) {
                    deferred.complete(false)
                    return@launch
                }
            } else if (contentDirty) {
                // 编辑过但未确认清空 — 磁盘与屏幕不一致，不得报告假成功。
                deferred.complete(false)
                return@launch
            }
        } else {
            val sendResult = saveCommandChannel.trySend(SaveCommand.Save(content, session, requiredRevision))
            if (sendResult.isFailure) {
                deferred.complete(false)
                return@launch
            }
        }
        val flushReply = CompletableDeferred<Boolean>()
        val flushResult = saveCommandChannel.trySend(SaveCommand.Flush(targetId, session.sessionId, requiredRevision, flushReply))
        if (flushResult.isFailure) {
            deferred.complete(false)
            return@launch
        }
        val result = flushReply.await()
        deferred.complete(result)
    }
    return deferred
}

fun EditorViewModel.clearChapterContent() {
    val session = currentSession ?: return
    contentExplicitlyCleared = true
    val revision = _sessionCoordinator?.sessionState?.revision ?: 0L
    saveCommandChannel.trySend(SaveCommand.Clear(session, revision))
}

fun EditorViewModel.startSaveActor() {
    saveActorJob?.cancel()
    saveActorJob = editorScope.launch(kotlinx.coroutines.Dispatchers.IO) {
        for (cmd in saveCommandChannel) {
            when (cmd) {
                is SaveCommand.Save -> {
                    if (cmd.session.sessionId == currentSession?.sessionId) {
                        performSave(cmd.content, cmd.session, isAutoSave = true, revisionAtEnqueue = cmd.revisionAtEnqueue)
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
                    val receiptOk = saveReceipts.canFlush(buildSaveToken(cmd.targetId, cmd.requiredRustRevision, committed?.contentHash ?: ""), committed?.contentHash)
                    val currentRevision = _sessionCoordinator?.sessionState?.revision ?: 0L
                    cmd.reply.complete(receiptOk && currentRevision == cmd.requiredRustRevision)
                }
            }
        }
    }
}

suspend fun EditorViewModel.clearChapterContentInternal(session: EditorSession, revisionAtEnqueue: Long = 0L): Boolean {
    return saveMutex.withLock {
        try {
            val result = workspaceRepository.clearChapterContent(session.projectId, session.volumeId, session.chapterId)
            when (result) {
                is com.xiwei.sujian.data.BridgeResult.Success -> {
                    val savedHash = result.data?.contentHash ?: ""
                    _uiState.value = _uiState.value.copy(
                        content = "",
                        chapterHash = savedHash,
                        saveStatus = SaveStatus.Saved
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
                is com.xiwei.sujian.data.BridgeResult.Error -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    if (result.code == "EMPTY_OVERWRITE_BLOCKED") {
                        _events.send(EditorEvent.ShowSaveFailedDialog(
                            getApplication<Application>().getString(R.string.error_empty_overwrite_dialog)))
                    } else {
                        _events.send(EditorEvent.ShowSaveFailedDialog(
                            getApplication<Application>().getString(R.string.error_save_failed, result.message)))
                    }
                    false
                }
                com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                    _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                    false
                }
            }
        } catch (e: Throwable) {
            _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
            _events.send(EditorEvent.ShowSaveFailedDialog(
                getApplication<Application>().getString(R.string.error_save_exception, e.message ?: "")))
            false
        }
    }
}

// #597 保存事务需原子执行（lease 校验→内容保存→回执记录→状态更新），含多种 BridgeResult 分支；
// 拆分会破坏保存回滚一致性 — 待后续重构
@Suppress("LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
suspend fun EditorViewModel.performSave(content: String, session: EditorSession, isAutoSave: Boolean, revisionAtEnqueue: Long = 0L): Boolean {
    if (content.trim().isEmpty()) {
        if (contentExplicitlyCleared) {
            return clearChapterContentInternal(session, revisionAtEnqueue)
        }
        return false
    }

    var currentContent = content
    var currentIsAutoSave = isAutoSave
    var lastSaveSuccess = false
    var currentRevision = revisionAtEnqueue
    val saveStartedAt = System.currentTimeMillis()

    while (true) {
        val contentToSave = currentContent
        saveMutex.withLock {
            val currentState = _uiState.value
            if (currentState.saveStatus == SaveStatus.Saving) {
                pendingSaveContent = contentToSave
                return false
            }

            _uiState.value = currentState.copy(saveStatus = SaveStatus.Saving)

            try {
                val result = workspaceRepository.saveChapterContent(session.projectId, session.volumeId, session.chapterId, contentToSave)
                when (result) {
                    is com.xiwei.sujian.data.BridgeResult.Success -> {
                        val savedHash = result.data?.contentHash ?: ""
                        com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                            session.projectId, session.chapterId,
                            contentToSave.toByteArray(Charsets.UTF_8).size, "ok",
                            System.currentTimeMillis() - saveStartedAt
                        )
                        val targetId = chapterTargetId(session.projectId, session.volumeId, session.chapterId)
                        // #595 七：保存落盘后记录回执（revision 精确锚定）。
                        saveReceipts.record(buildSaveToken(targetId, currentRevision, savedHash))
                        // #595 三：保存回执按 revision 条件提交 — 只有当前活动
                        // revision 仍等于保存时的 revision 才标记 Saved、清 dirty、
                        // markSaved。用户在保存 IO 期间继续输入（revision 前进）时，
                        // 只记录回执，不覆盖新输入产生的 UI 状态/dirty/chapterHash
                        // （旧实现无条件设 Saved，页面错误显示"已保存"，B 未落盘）。
                        val activeRevision = _sessionCoordinator?.sessionState?.revision ?: currentRevision
                        if (activeRevision == currentRevision) {
                            _uiState.value = _uiState.value.copy(
                                saveStatus = SaveStatus.Saved,
                                chapterHash = savedHash,
                            )
                            _sessionCoordinator?.markSaved(
                                targetId,
                                DocumentVersion(contentHash = savedHash),
                            )
                            contentDirty = false
                        } else {
                            _uiState.value = _uiState.value.copy(
                                saveStatus = SaveStatus.Unsaved,
                            )
                        }
                        val pending = pendingSaveContent
                        pendingSaveContent = null
                        if (pending != null && pending != contentToSave) {
                            currentContent = pending
                            currentIsAutoSave = true
                            lastSaveSuccess = true
                            currentRevision = _sessionCoordinator?.sessionState?.revision ?: currentRevision
                        } else {
                            lastSaveSuccess = true
                            return true
                        }
                    }
                    is com.xiwei.sujian.data.BridgeResult.Error -> {
                        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                        com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                            session.projectId, session.chapterId,
                            contentToSave.toByteArray(Charsets.UTF_8).size, "error",
                            System.currentTimeMillis() - saveStartedAt
                        )
                        if (result.code == "EMPTY_OVERWRITE_BLOCKED") {
                            if (!currentIsAutoSave) {
                                _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_empty_overwrite_dialog)))
                            } else {
                                emitErrorEvent(getApplication<Application>().getString(R.string.error_empty_overwrite_save_blocked))
                            }
                        } else {
                            if (!currentIsAutoSave) {
                                _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_save_failed, result.message)))
                            } else {
                                emitErrorEvent(getApplication<Application>().getString(R.string.error_auto_save_failed, result.message))
                            }
                        }
                        return false
                    }
                    com.xiwei.sujian.data.BridgeResult.NotLoaded -> {
                        _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                        com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                            session.projectId, session.chapterId,
                            contentToSave.toByteArray(Charsets.UTF_8).size, "not_loaded",
                            System.currentTimeMillis() - saveStartedAt
                        )
                        if (!currentIsAutoSave) {
                            _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_save_native_not_loaded)))
                        }
                        return false
                    }
                }
            } catch (e: Throwable) {
                _uiState.value = _uiState.value.copy(saveStatus = SaveStatus.SaveFailed)
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.chapterSave(
                    session.projectId, session.chapterId,
                    contentToSave.toByteArray(Charsets.UTF_8).size, "exception",
                    System.currentTimeMillis() - saveStartedAt
                )
                if (!currentIsAutoSave) {
                    _events.send(EditorEvent.ShowSaveFailedDialog(getApplication<Application>().getString(R.string.error_save_exception, e.message ?: "")))
                } else {
                    emitErrorEvent(getApplication<Application>().getString(R.string.error_auto_save_exception, e.message ?: ""))
                }
                return false
            }
        }
        if (!lastSaveSuccess) return false
    }
}

