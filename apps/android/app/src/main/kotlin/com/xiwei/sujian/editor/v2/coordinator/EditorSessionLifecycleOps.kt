package com.xiwei.sujian.editor.v2.coordinator

//! # 编辑器会话生命周期操作（从 EditorSessionCoordinator 拆分）

import android.util.Log

fun EditorSessionCoordinator.prepareTargetSessionForCommit(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
): PreparedSessionHandle? {
    val record = store.record(targetId) ?: return null
    val isPersistent = record.persistent
    val existingId = record.sessionId
    var newlyCreated = false
    val sessionId: ULong? = if (existingId != null && existingId != 0UL && validateSession(existingId)) {
        existingId
    } else {
        // 记录中的 ID 无效/缺失：清理失效 ID（Core 侧已不存在）并新建临时 session。
        if (existingId != null && existingId != 0UL) {
            closeSession(existingId)
        }
        newlyCreated = true
        val sel = initialSelection ?: initialText.toByteArray(Charsets.UTF_8).size
        createSession(targetId, initialText, sel, isPersistent)
    }
    if (sessionId == null || sessionId == 0UL) return null
    val snapshot = querySnapshotForSession(sessionId)
    if (snapshot == null) {
        // 新建的 session 读不到 snapshot — 预准备失败，关闭临时 session 不留下孤儿。
        if (newlyCreated) closeSession(sessionId)
        return null
    }
    return PreparedSessionHandle(
        targetId = targetId,
        sessionId = sessionId,
        snapshot = snapshot,
        newlyCreated = newlyCreated,
        previousRecord = record,
    )
}

fun EditorSessionCoordinator.commitPreparedSession(handle: PreparedSessionHandle, windowId: String = "prepared"): Boolean {
    val record = store.record(handle.targetId) ?: return false
    // #595 一：句柄仍有效 — 新建事务要求记录 sessionId 仍是 prepare 前的值
    // （prepare 不修改 store；previousRecord.sessionId 是事务前值，默认 0UL）；
    // 复用事务要求记录仍指向同一 session。不能要求"记录已存在 handle.sessionId"，
    // 否则新建 session（prepare 不写 store）永远无法提交。
    val expectedSessionId = if (handle.newlyCreated) {
        handle.previousRecord?.sessionId ?: 0UL
    } else {
        handle.sessionId
    }
    if (record.sessionId != expectedSessionId) return false
    // 1. 冻结并撤销 A 的输入 lease。
    invalidateInputLease()
    // 2. 一次性提交旧活动目标（若仍是活动状态）。
    val oldActive = activeTargetId
    // 合并嵌套 if：commitActiveSession 短路求值，仅在前置条件成立时调用。
    if (oldActive != null && oldActive != handle.targetId && !commitActiveSession(null)) {
        cancelActiveSession()
    }
    // 3. 激活 B — 通过唯一 reducer 原子推进。
    val snapshot = handle.snapshot
    val attaching = WindowBindingState.Attaching(windowId, handle.targetId, handle.sessionId)
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { _ ->
        // #595 一：提交时把 handle.sessionId + snapshot 写入 B 的正式记录 —
        // prepare 不修改 store，commit 是唯一写入点，保证 store 与 SessionState 一致。
        val rec = store.record(handle.targetId)
        val doc = rec?.documentState ?: DocumentState()
        pendingRecord = (rec ?: EditorSessionRecord(
            targetId = handle.targetId,
            persistent = handle.previousRecord?.persistent ?: false,
        )).copy(sessionId = handle.sessionId)
            .withDocumentState {
                it.copy(
                    text = snapshot.text,
                    revision = snapshot.revision,
                    selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                    selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                )
            }
        EditorSessionState(
            targetId = handle.targetId,
            sessionId = handle.sessionId,
            text = snapshot.text,
            revision = snapshot.revision,
            selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
            selectionHeadUtf8 = snapshot.selectionHeadUtf8,
            lastAppliedTransactionId = doc.lastAppliedTransactionId,
            origin = EditorSessionOrigin.INITIAL_LOAD,
            bindingState = attaching,
            editingState = EditingState.BINDING,
            activeTargetId = handle.targetId,
            committedVersion = doc.committedVersion,
            sessionBaseVersion = doc.sessionBaseVersion,
            localDirty = doc.localDirty,
        )
    }
    pendingRecord?.let { store.put(it) }
    com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
        handle.sessionId.toString(), "commit_prepared"
    )
    return true
}

fun EditorSessionCoordinator.releasePreparedTarget(handle: PreparedSessionHandle) {
    val record = store.record(handle.targetId)
    if (handle.newlyCreated) {
        closeSession(handle.sessionId)
        if (handle.sessionId != 0UL) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
                handle.sessionId.toString(), "release_prepared_new"
            )
        }
        // 只移除仍指向该 session 的记录 — 若事务期间记录已被其他路径替换，
        // 不覆盖其状态。
        if (record != null && record.sessionId == handle.sessionId) {
            store.remove(handle.targetId)
        }
    } else {
        // 借用的既有 session：恢复事务前记录，保留 Undo/Redo 与文档事实。
        val previous = handle.previousRecord
        if (previous != null) {
            store.put(previous)
        }
        if (handle.sessionId != 0UL) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
                handle.sessionId.toString(), "release_prepared_borrowed"
            )
        }
    }
}

fun EditorSessionCoordinator.detachWindowBinding(windowId: String, targetId: String) {
    val record = store.record(targetId)
    val isPersistent = record?.persistent ?: false
    val sessionId = record?.sessionId
    // #595 二：窗口解绑使该窗口持有的输入 lease 失效 — 解绑后晚到的回调
    // （回调清除窗口期内的竞态）不能再进入会话层。
    invalidateInputLease()
    if (!isPersistent || sessionId == null || sessionId == 0UL) {
        // 草稿会话或已无会话：直接关闭/清理窗口引用
        if (sessionId != null && sessionId != 0UL) {
            closeSession(sessionId)
        }
        store.remove(targetId)
        clearWindowAttach(targetId)
        return
    }
    val snapshot = if (validateSession(sessionId)) queryTargetSnapshot(targetId) else null
    val detached = WindowBindingState.Detached(targetId, sessionId, snapshot)
    // #595 三/四：通过唯一 reducer 原子推进 bindingState/editingState/activeTargetId。
    // #595 一：只清理本 target 的窗口状态 — 章节切换事务已把 B 设为活动目标时，
    // 旧 pane 的 onDispose（detachWindowBinding(A)）不得把 B 的 Attaching/Attached
    // 状态清成 Detached(A)。
    updateSessionState { state ->
        if (state.targetId != targetId) {
            state
        } else {
            state.copy(
                bindingState = detached,
                editingState = EditingState.IDLE,
                activeTargetId = null,
            )
        }
    }
    com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
        sessionId.toString(), "window_detached"
    )
}

fun EditorSessionCoordinator.completeWindowAttach(windowId: String, targetId: String, sessionId: ULong) {
    val current = _sessionStateFlow.value.bindingState
    // 幂等重入：已经是同一 target/session 的 Attached（如 beginEdit 重复调用）保持现状。
    if (current is WindowBindingState.Attached &&
        current.targetId == targetId && current.sessionId == sessionId
    ) {
        return
    }
    if (current !is WindowBindingState.Attaching || current.targetId != targetId) {
        Log.w(EditorSessionCoordinator.TAG, "completeWindowAttach($targetId): current state $current is not Attaching for target — ignoring (Attached requires a bound View)")
        return
    }
    val attached = WindowBindingState.Attached(windowId, targetId, sessionId)
    updateSessionState { it.copy(
        bindingState = attached,
        editingState = EditingState.EDITING,
    ) }
}

fun EditorSessionCoordinator.closeTarget(targetId: String, reason: SessionCloseReason) {
    val wasActive = activeTargetId == targetId
    if (wasActive) {
        commitActiveSession(null)
    }
    val record = store.record(targetId)
    val sessionId = record?.sessionId
    if (sessionId != null && sessionId != 0UL) {
        closeSession(sessionId)
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
            sessionId.toString(), "close_target:${reason.name.lowercase()}"
        )
    }
    store.remove(targetId)
    invalidateInputLease()
    // #595 三/四：被关闭的 target 是当前 SessionState target 时整体回到 Idle；
    // 否则保留活动 target 的状态（旧实现会把新章节的 Attached 清成 Idle）。
    // 记录已删除，残留的 Detached(targetId) 状态没有意义 — 统一回 Idle。
    if (_sessionStateFlow.value.targetId == targetId) {
        updateSessionState {
            if (it.targetId == targetId) {
                EditorSessionState(
                    editingState = EditingState.IDLE,
                    bindingState = WindowBindingState.Idle,
                    activeTargetId = null,
                )
            } else {
                it
            }
        }
    }
}

fun EditorSessionCoordinator.clearWindowAttach(targetId: String) {
    store.remove(targetId)
    updateSessionState {
        if (it.targetId == targetId) {
            it.copy(
                editingState = EditingState.IDLE,
                bindingState = WindowBindingState.Idle,
                activeTargetId = null,
                targetId = null,
                sessionId = null,
            )
        } else {
            it
        }
    }
}

// #597 session 预准备需校验记录/会话/绑定多态后原子推进，拆分会破坏 session 状态机 — 待后续重构
@Suppress("LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
fun EditorSessionCoordinator.prepareSessionForEdit(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
    windowId: String = "prepared",
): SessionBindInfo? {
    val record = store.record(targetId) ?: return null
    val isPersistent = record.persistent
    val profile = record.profile

    if (activeTargetId == targetId && (editingState == EditingState.EDITING || editingState == EditingState.BINDING)) {
        val sid = store.record(targetId)?.sessionId ?: return null
        if (sid == 0UL) return null
        return SessionBindInfo(sid, profile, isPersistent, snapshot = querySnapshotForSession(sid))
    }

    if (activeTargetId != null && activeTargetId != targetId) {
        updateSessionState { it.copy(editingState = EditingState.REBINDING) }
        if (!commitActiveSession(null)) {
            cancelActiveSession()
        }
    }

    updateSessionState { it.copy(editingState = EditingState.BINDING) }

    val textForSession = initialText
    val sel = initialSelection ?: textForSession.toByteArray(Charsets.UTF_8).size
    val existingId = store.record(targetId)?.sessionId
    val sessionId = if (existingId != null && existingId != 0UL && validateSession(existingId)) {
        existingId
    } else {
        if (existingId != null && existingId != 0UL) {
            closeSession(existingId)
        }
        createSession(targetId, textForSession, sel, isPersistent)
    }

    if (sessionId == null || sessionId == 0UL) {
        Log.e(EditorSessionCoordinator.TAG, "prepareSessionForEdit($targetId): session creation returned invalid id=$sessionId, aborting")
        store.remove(targetId)
        updateSessionState { it.copy(
            editingState = EditingState.IDLE,
            bindingState = WindowBindingState.Idle,
        ) }
        return null
    }

    val attaching = WindowBindingState.Attaching(windowId, targetId, sessionId)

    // #595 一/二/三/四：通过唯一 reducer 更新 SessionState — 无论新建还是复用、
    // 持久还是草稿，都用真实 snapshot（createSession 已把初始正文装入 kernel，
    // 是唯一一次 Core 命令；草稿 session 同样记录 sessionId）。
    val snapshot = querySnapshotForSession(sessionId)
    store.update(targetId) { r ->
        r.copy(
            sessionId = sessionId,
            documentState = if (snapshot != null) {
                r.documentState.copy(
                    text = snapshot.text,
                    revision = snapshot.revision,
                    selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                    selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                )
            } else {
                r.documentState.copy(
                    text = textForSession,
                    revision = 0L,
                    selectionAnchorUtf8 = sel,
                    selectionHeadUtf8 = sel,
                )
            },
        )
    }
    updateSessionState { _ ->
        val rec = store.record(targetId)
        val doc = rec?.documentState
        EditorSessionState(
            targetId = targetId,
            sessionId = sessionId,
            text = doc?.text ?: textForSession,
            revision = doc?.revision ?: 0L,
            selectionAnchorUtf8 = doc?.selectionAnchorUtf8 ?: sel,
            selectionHeadUtf8 = doc?.selectionHeadUtf8 ?: sel,
            lastAppliedTransactionId = doc?.lastAppliedTransactionId ?: 0L,
            origin = EditorSessionOrigin.INITIAL_LOAD,
            bindingState = attaching,
            editingState = EditingState.BINDING,
            activeTargetId = targetId,
            committedVersion = doc?.committedVersion ?: DocumentVersion(),
            sessionBaseVersion = doc?.sessionBaseVersion ?: DocumentVersion(),
            localDirty = doc?.localDirty ?: false,
        )
    }
    return SessionBindInfo(sessionId, profile, isPersistent, snapshot = snapshot)
}

fun EditorSessionCoordinator.commitActiveSession(finalText: String?): Boolean {
    val targetId = activeTargetId ?: return false
    val record = store.record(targetId) ?: return false
    val sessionId = record.sessionId
    if (sessionId == 0UL) return false
    val isPersistent = record.persistent
    val windowBound = windowBindingState is WindowBindingState.Attached ||
        windowBindingState is WindowBindingState.Attaching

    updateSessionState { it.copy(
        editingState = EditingState.COMMITTING,
        bindingState = if (windowBound) WindowBindingState.Committing(targetId, sessionId) else it.bindingState,
    ) }
    // #595 四：正文在 store 记录/SessionState 中（applyLocalEdit 已更新），
    // 不再维护第二份正文缓存。
    if (!isPersistent || !windowBound) {
        closeSession(sessionId)
        store.remove(targetId)
    }
    // #595 三/四：提交清除后 SessionState 必须回到 Idle。
    updateSessionState { EditorSessionState() }
    _lastCommittedTextFlow.value = if (record.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
    return true
}

fun EditorSessionCoordinator.cancelActiveSession(): Boolean {
    val targetId = activeTargetId ?: return false
    val record = store.record(targetId) ?: return false
    val sessionId = record.sessionId
    if (sessionId == 0UL) return false
    val windowBound = windowBindingState is WindowBindingState.Attached ||
        windowBindingState is WindowBindingState.Attaching

    updateSessionState { it.copy(
        editingState = EditingState.CANCELLING,
        bindingState = if (windowBound) WindowBindingState.Cancelling(targetId, sessionId) else it.bindingState,
    ) }
    closeSession(sessionId)
    store.remove(targetId)
    // #595 三/四：取消清除后 SessionState 必须回到 Idle。
    updateSessionState { EditorSessionState() }
    return true
}

fun EditorSessionCoordinator.resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL): ExternalResetResult {
    // #595 一/五：返回可提交事务结果 — reset 未执行或 Core 失败时返回 Failed，
    // 调用方不得推进 SessionStore/ViewModel 状态（旧实现返回 Unit，WritingPane
    // 无条件推进导致三份状态分裂）。
    // #595 一：禁止构造 revision=0 的兜底 snapshot — Core reset/create 成功后
    // 必须再次读取真实 snapshot，读取失败则整个操作失败（旧实现用输入参数补
    // revision=0 快照，调用方仍当成功，导致 Rust session/SessionStore/ViewModel
    // 三份状态分裂）。
    if (source == SessionResetSource.LOCAL_CONTENT_CHANGED) return ExternalResetResult.Failed
    val record = store.record(targetId)
    if (record?.persistent != true) return ExternalResetResult.Failed

    val sessionId = record.sessionId
    if (sessionId == 0UL) {
        val newSessionId = createSession(targetId, text, cursorUtf8, true)
        if (newSessionId == null || newSessionId == 0UL) {
            Log.e(EditorSessionCoordinator.TAG, "resetPersistentSession($targetId): failed to create session for empty/missing persistent session")
            return ExternalResetResult.Failed
        }
        return commitResetSnapshot(targetId, newSessionId)
    }

    if (!validateSession(sessionId)) {
        Log.w(EditorSessionCoordinator.TAG, "resetPersistentSession($targetId): session $sessionId no longer valid, deleting and recreating")
        store.update(targetId) { it.copy(sessionId = 0UL) }
        closeSession(sessionId)
        return resetPersistentSession(targetId, text, cursorUtf8, source)
    }

    // #595 一：候选 session 原子交换 — 不原地 reset 旧 session。原地 reset 成功后
    // snapshot 读取失败时旧 Undo/Redo/composition/正文已被 load_text 破坏，无法回滚。
    // 创建 candidate session 装入新正文，读取真实 snapshot：失败则关闭 candidate、
    // 旧 session/store/view 完全不动；成功则原子提交 candidate record 并关闭旧 session。
    val candidateSessionId = createSession(targetId, text, cursorUtf8, true)
    if (candidateSessionId == null || candidateSessionId == 0UL) {
        Log.e(EditorSessionCoordinator.TAG, "resetPersistentSession($targetId): failed to create candidate session — old session preserved")
        return ExternalResetResult.Failed
    }
    return commitResetSnapshot(targetId, candidateSessionId, oldSessionIdToClose = sessionId)
}

fun EditorSessionCoordinator.commitResetSnapshot(
    targetId: String,
    sessionId: ULong,
    oldSessionIdToClose: ULong? = null,
): ExternalResetResult {
    val snapshot = querySnapshotForSession(sessionId)
    if (snapshot == null) {
        Log.e(EditorSessionCoordinator.TAG, "commitResetSnapshot($targetId): snapshot read failed — closing candidate $sessionId, old session preserved")
        closeSession(sessionId)
        return ExternalResetResult.Failed
    }
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { previous ->
        val rec = store.record(targetId)
        pendingRecord = (rec ?: EditorSessionRecord(targetId = targetId, persistent = true)).copy(
            sessionId = sessionId,
            documentState = (rec?.documentState ?: DocumentState()).copy(
                text = snapshot.text,
                revision = snapshot.revision,
                selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                selectionHeadUtf8 = snapshot.selectionHeadUtf8,
            ),
        )
        if (previous.targetId != targetId) return@updateSessionState previous
        previous.copy(
            sessionId = sessionId,
            text = snapshot.text,
            revision = snapshot.revision,
            selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
            selectionHeadUtf8 = snapshot.selectionHeadUtf8,
            origin = EditorSessionOrigin.EXTERNAL_REPLACE,
        )
    }
    pendingRecord?.let { store.put(it) }
    if (oldSessionIdToClose != null && oldSessionIdToClose != 0UL && oldSessionIdToClose != sessionId) {
        closeSession(oldSessionIdToClose)
    }
    val state = windowBindingState
    if (state is WindowBindingState.Detached && state.targetId == targetId) {
        updateSessionState { it.copy(bindingState = WindowBindingState.Detached(targetId, sessionId, snapshot)) }
    }
    return ExternalResetResult.Success(snapshot)
}

fun EditorSessionCoordinator.refreshDetachedSnapshot(targetId: String): TargetSnapshot? {
    val snapshot = queryTargetSnapshot(targetId)
    val state = windowBindingState
    if (state is WindowBindingState.Detached && state.targetId == targetId) {
        val sid = store.record(targetId)?.sessionId ?: return snapshot
        updateSessionState { it.copy(bindingState = WindowBindingState.Detached(targetId, sid, snapshot)) }
    }
    return snapshot
}

fun EditorSessionCoordinator.releaseHost() {
    if (activeTargetId != null) {
        cancelActiveSession()
    }
    store.allRecords().forEach { record ->
        if (record.sessionId != 0UL) {
            closeSession(record.sessionId)
        }
    }
    store.clear()
    updateSessionState { EditorSessionState(editingState = EditingState.RELEASED) }
}

