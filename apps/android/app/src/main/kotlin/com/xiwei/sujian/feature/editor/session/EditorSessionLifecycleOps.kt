package com.xiwei.sujian.feature.editor.session

// ! # 编辑器会话生命周期操作（从 EditorSessionCoordinator 拆分）
// ! #624 评论17 问题1：走 mutateSession 单一临界区，删除 pendingRecord 外置副作用。
// ! #624 评论17 问题2：删除 prepared 假窗口 — commitPreparedSession 后 Detached/IDLE/activeTargetId=null。

import android.util.Log
import com.xiwei.sujian.feature.editor.window.EditingState

fun EditorSessionCoordinator.prepareTargetSessionForCommit(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
): PreparedSessionHandle? {
    // #624 评论16 问题1：prepare 是无副作用事务 — 不关闭任何 prepare 之前就存在的 session，
    // 也不写 EditorSessionStore。所有权由 [PreparedSessionMode] 枚举明确表达。
    val record = store.record(targetId) ?: return null
    if (record.documentState.localDirty) return null
    val existingId = record.sessionId
    val existingValid = existingId != 0UL && validateSession(existingId)
    if (existingValid) {
        return prepareFromValidExistingSession(targetId, initialText, initialSelection, record, existingId!!)
    }
    return prepareNewCandidateSession(targetId, initialText, initialSelection, record)
}

private fun EditorSessionCoordinator.prepareFromValidExistingSession(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
    record: EditorSessionRecord,
    existingId: ULong,
): PreparedSessionHandle? {
    val existingSnapshot = querySnapshotForSession(existingId)
    if (existingSnapshot == null) return null
    if (existingSnapshot.text == initialText) {
        return PreparedSessionHandle(
            targetId = targetId,
            sessionId = existingId,
            snapshot = existingSnapshot,
            mode = PreparedSessionMode.Borrowed,
            previousRecord = record,
        )
    }
    val sel = initialSelection ?: initialText.toByteArray(Charsets.UTF_8).size
    val candidateId = createSession(targetId, initialText, sel, record.persistent)
    if (candidateId == null || candidateId == 0UL) return null
    val candidateSnapshot = querySnapshotForSession(candidateId)
    if (candidateSnapshot == null) {
        closeSession(candidateId)
        return null
    }
    return PreparedSessionHandle(
        targetId = targetId,
        sessionId = candidateId,
        snapshot = candidateSnapshot,
        mode = PreparedSessionMode.Replacement(existingId),
        previousRecord = record,
    )
}

private fun EditorSessionCoordinator.prepareNewCandidateSession(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
    record: EditorSessionRecord,
): PreparedSessionHandle? {
    val sel = initialSelection ?: initialText.toByteArray(Charsets.UTF_8).size
    val sessionId = createSession(targetId, initialText, sel, record.persistent)
    if (sessionId == null || sessionId == 0UL) return null
    val snapshot = querySnapshotForSession(sessionId)
    if (snapshot == null) {
        closeSession(sessionId)
        return null
    }
    return PreparedSessionHandle(
        targetId = targetId,
        sessionId = sessionId,
        snapshot = snapshot,
        mode = PreparedSessionMode.Created,
        previousRecord = record,
    )
}

/**
 * #624 评论17 问题2：提交预准备 session — 删除 "prepared" 假窗口身份。
 *
 * prepared commit 成功后状态为：
 * - bindingState = WindowBindingState.Detached(targetId, sessionId, snapshot)
 * - editingState = EditingState.IDLE
 * - activeTargetId = null
 *
 * targetId/sessionId/revision 仍记录 B 的正式 session（store 记录写入 handle.sessionId）。
 * 真正窗口出现后 WritingPaneEditorAttach() 从 Detached 调 beginEdit()，由
 * prepareSessionForEdit(..., realWindowId) 进入：
 * Detached → Attaching(realWindowId, B, sessionId) → AndroidView factory/attachView
 * → Attached(realWindowId, B, sessionId) → confirmEditorAttached → inputFrozen=false。
 *
 * #624 评论17 问题1：在 [mutateSession] 单一临界区内原子完成 lease 失效、
 * 旧活动目标提交、B 的 store/sessionState 写入，不再用 update lambda + pendingRecord。
 */
@Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod")
fun EditorSessionCoordinator.commitPreparedSession(handle: PreparedSessionHandle): Boolean {
    val record = store.record(handle.targetId) ?: return false
    val expectedSessionId =
        when (handle.mode) {
            PreparedSessionMode.Borrowed -> handle.sessionId
            PreparedSessionMode.Created -> handle.previousRecord?.sessionId ?: 0UL
            is PreparedSessionMode.Replacement -> handle.mode.oldSessionId
        }
    if (record.sessionId != expectedSessionId) return false

    val snapshot = handle.snapshot
    var committedProfile: TextEditorProfile? = null
    mutateSession {
        // 1. 冻结并撤销 A 的输入 lease。
        invalidateLease()
        // 2. 一次性提交旧活动目标 A（若仍是活动状态且不是 B）。
        val oldActive = sessionState.activeTargetId
        if (oldActive != null && oldActive != handle.targetId) {
            val oldRec = record(oldActive)
            val oldSessionId = oldRec?.sessionId
            if (oldSessionId != null && oldSessionId != 0UL) {
                val oldPersistent = oldRec.persistent
                val oldWindowBound =
                    sessionState.bindingState is WindowBindingState.Attached ||
                        sessionState.bindingState is WindowBindingState.Attaching
                if (!oldPersistent || !oldWindowBound) {
                    coordinator.closeSession(oldSessionId)
                    removeRecord(oldActive)
                }
                committedProfile = oldRec.profile
            }
        }
        // 3. 激活 B 的正式 session 记录 — store 写入 handle.sessionId + snapshot。
        val rec = record(handle.targetId)
        val doc = rec?.documentState ?: DocumentState()
        putRecord(
            (
                rec ?: EditorSessionRecord(
                    targetId = handle.targetId,
                    persistent = handle.previousRecord?.persistent ?: false,
                )
            ).copy(sessionId = handle.sessionId)
                .withDocumentState {
                    it.copy(
                        revision = snapshot.revision,
                        selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                        selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    )
                },
        )
        // #624 评论17 问题2：prepared commit 后 Detached/IDLE/activeTargetId=null —
        // 不再用 Attaching("prepared", ...)。真实窗口出现后从 Detached 调 beginEdit。
        sessionState =
            EditorSessionState(
                targetId = handle.targetId,
                sessionId = handle.sessionId,
                revision = snapshot.revision,
                selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                lastAppliedTransactionId = doc.lastAppliedTransactionId,
                origin = EditorSessionOrigin.INITIAL_LOAD,
                bindingState = WindowBindingState.Detached(handle.targetId, handle.sessionId, snapshot),
                editingState = EditingState.IDLE,
                activeTargetId = null,
                committedVersion = doc.committedVersion,
                sessionBaseVersion = doc.sessionBaseVersion,
                localDirty = doc.localDirty,
            )
    }
    // #624 评论15 问题2：candidate swap commit 成功后关闭被替换的旧 session。
    closeReplacedSessionAfterCommit(handle)
    if (committedProfile != null) {
        _lastCommittedTextFlow.value = null
    }
    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
        handle.sessionId.toString(),
        "commit_prepared",
    )
    return true
}

private fun EditorSessionCoordinator.closeReplacedSessionAfterCommit(handle: PreparedSessionHandle) {
    val replaced = (handle.mode as? PreparedSessionMode.Replacement)?.oldSessionId
    if (replaced != null && replaced != 0UL && replaced != handle.sessionId) {
        closeSession(replaced)
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
            replaced.toString(),
            "commit_prepared_replaced",
        )
    }
}

fun EditorSessionCoordinator.releasePreparedTarget(handle: PreparedSessionHandle) {
    when (handle.mode) {
        PreparedSessionMode.Borrowed -> {
            // 借用既有 session 的 abort 是 no-op — 旧 session 与 store 记录原样保留。
        }
        PreparedSessionMode.Created -> {
            closeSession(handle.sessionId)
            if (handle.sessionId != 0UL) {
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
                    handle.sessionId.toString(),
                    "release_prepared_new",
                )
            }
            mutateSession {
                val rec = record(handle.targetId)
                if (rec != null && rec.sessionId == handle.sessionId) {
                    removeRecord(handle.targetId)
                }
            }
        }
        is PreparedSessionMode.Replacement -> {
            closeSession(handle.sessionId)
            if (handle.sessionId != 0UL) {
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
                    handle.sessionId.toString(),
                    "release_prepared_candidate",
                )
            }
        }
    }
}

private fun EditorSessionCoordinator.isBindingForDifferentWindow(
    windowId: String,
    targetId: String,
): Boolean {
    val currentBinding = _sessionStateFlow.value.bindingState
    return when (currentBinding) {
        is WindowBindingState.Attaching ->
            currentBinding.windowId != windowId || currentBinding.targetId != targetId
        is WindowBindingState.Attached ->
            currentBinding.windowId != windowId || currentBinding.targetId != targetId
        else -> false
    }
}

@Suppress("CognitiveComplexMethod")
fun EditorSessionCoordinator.detachWindowBinding(
    windowId: String,
    targetId: String,
) {
    val record = store.record(targetId)
    val isPersistent = record?.persistent ?: false
    val sessionId = record?.sessionId
    if (isBindingForDifferentWindow(windowId, targetId)) return
    val currentBinding = _sessionStateFlow.value.bindingState
    if (currentBinding is WindowBindingState.Detached && currentBinding.targetId == targetId) return
    if (!isPersistent || sessionId == null || sessionId == 0UL) {
        mutateSession {
            invalidateLease()
            if (sessionId != null && sessionId != 0UL) {
                coordinator.closeSession(sessionId)
            }
            removeRecord(targetId)
            if (sessionState.targetId == targetId) {
                sessionState =
                    sessionState.copy(
                        editingState = EditingState.IDLE,
                        bindingState = WindowBindingState.Idle,
                        activeTargetId = null,
                        targetId = null,
                        sessionId = null,
                    )
            }
        }
        return
    }
    val snapshot = if (validateSession(sessionId)) queryTargetSnapshot(targetId) else null
    val detached = WindowBindingState.Detached(targetId, sessionId, snapshot)
    mutateSession {
        invalidateLease()
        if (sessionState.targetId == targetId) {
            sessionState =
                sessionState.copy(
                    bindingState = detached,
                    editingState = EditingState.IDLE,
                    activeTargetId = null,
                )
        }
    }
    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
        sessionId.toString(),
        "window_detached",
    )
}

fun EditorSessionCoordinator.completeWindowAttach(
    windowId: String,
    targetId: String,
    sessionId: ULong,
) {
    val current = _sessionStateFlow.value.bindingState
    if (current.isExactAttached(windowId, targetId, sessionId)) {
        return
    }
    if (!current.isExactAttaching(windowId, targetId, sessionId)) {
        Log.w(
            EditorSessionCoordinator.TAG,
            "completeWindowAttach($windowId,$targetId,$sessionId): current state $current is not the " +
                "exact Attaching for this window/target/session — ignoring (Attached requires a bound View)",
        )
        return
    }
    val attached = WindowBindingState.Attached(windowId, targetId, sessionId)
    mutateSession {
        sessionState =
            sessionState.copy(
                bindingState = attached,
                editingState = EditingState.EDITING,
            )
    }
}

private fun WindowBindingState.isExactAttached(
    windowId: String,
    targetId: String,
    sessionId: ULong,
): Boolean =
    this is WindowBindingState.Attached &&
        this.windowId == windowId &&
        this.targetId == targetId &&
        this.sessionId == sessionId

private fun WindowBindingState.isExactAttaching(
    windowId: String,
    targetId: String,
    sessionId: ULong,
): Boolean =
    this is WindowBindingState.Attaching &&
        this.windowId == windowId &&
        this.targetId == targetId &&
        this.sessionId == sessionId

fun EditorSessionCoordinator.closeTarget(
    targetId: String,
    reason: SessionCloseReason,
) {
    val wasActive = activeTargetId == targetId
    if (wasActive) {
        commitActiveSession(null)
    }
    val record = store.record(targetId)
    val sessionId = record?.sessionId
    if (sessionId != null && sessionId != 0UL) {
        closeSession(sessionId)
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
            sessionId.toString(),
            "close_target:${reason.name.lowercase()}",
        )
    }
    mutateSession {
        removeRecord(targetId)
        invalidateLease()
        if (sessionState.targetId == targetId) {
            sessionState =
                EditorSessionState(
                    editingState = EditingState.IDLE,
                    bindingState = WindowBindingState.Idle,
                    activeTargetId = null,
                )
        }
    }
}

fun EditorSessionCoordinator.clearWindowAttach(targetId: String) {
    mutateSession {
        removeRecord(targetId)
        if (sessionState.targetId == targetId) {
            sessionState =
                sessionState.copy(
                    editingState = EditingState.IDLE,
                    bindingState = WindowBindingState.Idle,
                    activeTargetId = null,
                    targetId = null,
                    sessionId = null,
                )
        }
    }
}

/**
 * #624 评论17 问题2：准备会话绑定 — 删除 "prepared" 假窗口默认参数。
 * 真实窗口层必须传入真实 windowId。
 */
fun EditorSessionCoordinator.prepareSessionForEdit(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
    windowId: String,
): SessionBindInfo? {
    val record = store.record(targetId) ?: return null
    val isPersistent = record.persistent
    val profile = record.profile

    prepareActiveSessionIfCurrent(targetId)?.let { bind ->
        restampAttachingToWindow(windowId, targetId)
        return bind
    }
    rebindFromOtherActiveIfNeeded(targetId)

    val textForSession = initialText
    val sel = initialSelection ?: textForSession.toByteArray(Charsets.UTF_8).size
    val sessionId = resolveSessionForPrepare(targetId, textForSession, sel, isPersistent)

    if (sessionId == null || sessionId == 0UL) {
        Log.e(
            EditorSessionCoordinator.TAG,
            "prepareSessionForEdit($targetId): session creation returned invalid id=$sessionId, aborting",
        )
        mutateSession {
            removeRecord(targetId)
            sessionState =
                sessionState.copy(
                    editingState = EditingState.IDLE,
                    bindingState = WindowBindingState.Idle,
                )
        }
        return null
    }

    val attaching = WindowBindingState.Attaching(windowId, targetId, sessionId)
    val snapshot = querySnapshotForSession(sessionId)
    commitPreparedBindingState(targetId, sessionId, textForSession, sel, snapshot, attaching)
    return SessionBindInfo(sessionId, profile, isPersistent, snapshot = snapshot)
}

private fun EditorSessionCoordinator.commitPreparedBindingState(
    targetId: String,
    sessionId: ULong,
    textForSession: String,
    sel: Int,
    snapshot: TargetSnapshot?,
    attaching: WindowBindingState.Attaching,
) {
    mutateSession {
        val currentRec = record(targetId)
        putRecord(
            currentRec?.copy(
                sessionId = sessionId,
                documentState =
                    if (snapshot != null) {
                        currentRec.documentState.copy(
                            revision = snapshot.revision,
                            selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                            selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                        )
                    } else {
                        currentRec.documentState.copy(
                            revision = 0L,
                            selectionAnchorUtf8 = sel,
                            selectionHeadUtf8 = sel,
                        )
                    },
            ) ?: EditorSessionRecord(
                targetId = targetId,
                sessionId = sessionId,
                documentState =
                    if (snapshot != null) {
                        DocumentState(
                            revision = snapshot.revision,
                            selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                            selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                        )
                    } else {
                        DocumentState(
                            revision = 0L,
                            selectionAnchorUtf8 = sel,
                            selectionHeadUtf8 = sel,
                        )
                    },
            ),
        )
        val rec = record(targetId)
        val doc = rec?.documentState
        sessionState =
            EditorSessionState(
                targetId = targetId,
                sessionId = sessionId,
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
}

private fun EditorSessionCoordinator.restampAttachingToWindow(
    windowId: String,
    targetId: String,
) {
    val sessionId = store.record(targetId)?.sessionId ?: return
    if (sessionId == 0UL) return
    val current = _sessionStateFlow.value.bindingState
    val currentWindowId: String? =
        when (current) {
            is WindowBindingState.Attaching ->
                current.windowId.takeIf { current.targetId == targetId && current.sessionId == sessionId }
            is WindowBindingState.Attached ->
                current.windowId.takeIf { current.targetId == targetId && current.sessionId == sessionId }
            else -> null
        }
    if (currentWindowId == null || currentWindowId == windowId) return
    mutateSession {
        invalidateLease()
        if (sessionState.targetId == targetId) {
            sessionState =
                sessionState.copy(
                    bindingState = WindowBindingState.Attaching(windowId, targetId, sessionId),
                    editingState = EditingState.BINDING,
                )
        }
    }
}

private fun EditorSessionCoordinator.prepareActiveSessionIfCurrent(targetId: String): SessionBindInfo? {
    if (activeTargetId != targetId || (editingState != EditingState.EDITING && editingState != EditingState.BINDING)) {
        return null
    }
    val sid = store.record(targetId)?.sessionId ?: return null
    if (sid == 0UL) return null
    val profile = store.record(targetId)?.profile ?: return null
    return SessionBindInfo(
        sid,
        profile,
        store.record(targetId)?.persistent ?: false,
        snapshot = querySnapshotForSession(sid),
    )
}

private fun EditorSessionCoordinator.rebindFromOtherActiveIfNeeded(targetId: String) {
    if (activeTargetId == null || activeTargetId == targetId) return
    mutateSession {
        sessionState = sessionState.copy(editingState = EditingState.REBINDING)
    }
    if (!commitActiveSession(null)) {
        cancelActiveSession()
    }
}

private fun EditorSessionCoordinator.resolveSessionForPrepare(
    targetId: String,
    textForSession: String,
    sel: Int,
    isPersistent: Boolean,
): ULong? {
    val existingId = store.record(targetId)?.sessionId
    if (existingId != null && existingId != 0UL && validateSession(existingId)) {
        return existingId
    }
    if (existingId != null && existingId != 0UL) {
        closeSession(existingId)
    }
    return createSession(targetId, textForSession, sel, isPersistent)
}

@Suppress("CognitiveComplexMethod")
fun EditorSessionCoordinator.commitActiveSession(finalText: String?): Boolean {
    var committed = false
    var profile: TextEditorProfile? = null
    mutateSession {
        val targetId = sessionState.activeTargetId ?: return@mutateSession
        val rec = record(targetId) ?: return@mutateSession
        val sessionId = rec.sessionId
        if (sessionId == 0UL) return@mutateSession
        val isPersistent = rec.persistent
        val windowBound =
            sessionState.bindingState is WindowBindingState.Attached ||
                sessionState.bindingState is WindowBindingState.Attaching
        sessionState =
            sessionState.copy(
                editingState = EditingState.COMMITTING,
                bindingState =
                    if (windowBound) {
                        WindowBindingState.Committing(targetId, sessionId)
                    } else {
                        sessionState.bindingState
                    },
            )
        if (!isPersistent || !windowBound) {
            coordinator.closeSession(sessionId)
            removeRecord(targetId)
        }
        sessionState = EditorSessionState()
        profile = rec.profile
        committed = true
    }
    if (committed) {
        _lastCommittedTextFlow.value =
            if (profile?.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
    }
    return committed
}

fun EditorSessionCoordinator.cancelActiveSession(): Boolean {
    var cancelled = false
    mutateSession {
        val targetId = sessionState.activeTargetId ?: return@mutateSession
        val rec = record(targetId) ?: return@mutateSession
        val sessionId = rec.sessionId
        if (sessionId == 0UL) return@mutateSession
        val windowBound =
            sessionState.bindingState is WindowBindingState.Attached ||
                sessionState.bindingState is WindowBindingState.Attaching
        sessionState =
            sessionState.copy(
                editingState = EditingState.CANCELLING,
                bindingState =
                    if (windowBound) {
                        WindowBindingState.Cancelling(targetId, sessionId)
                    } else {
                        sessionState.bindingState
                    },
            )
        coordinator.closeSession(sessionId)
        removeRecord(targetId)
        sessionState = EditorSessionState()
        cancelled = true
    }
    return cancelled
}

fun EditorSessionCoordinator.resetPersistentSession(
    targetId: String,
    text: String,
    cursorUtf8: Int,
    source: SessionResetSource = SessionResetSource.EXTERNAL,
): ExternalResetResult {
    if (source == SessionResetSource.LOCAL_CONTENT_CHANGED) return ExternalResetResult.Failed
    val record = store.record(targetId)
    if (record?.persistent != true) return ExternalResetResult.Failed

    val sessionId = record.sessionId
    if (sessionId == 0UL) {
        val newSessionId = createSession(targetId, text, cursorUtf8, true)
        if (newSessionId == null || newSessionId == 0UL) {
            Log.e(
                EditorSessionCoordinator.TAG,
                "resetPersistentSession($targetId): failed to create session for empty/missing persistent session",
            )
            return ExternalResetResult.Failed
        }
        return commitResetSnapshot(targetId, newSessionId)
    }

    if (!validateSession(sessionId)) {
        Log.w(
            EditorSessionCoordinator.TAG,
            "resetPersistentSession($targetId): session $sessionId no longer valid, deleting and recreating",
        )
        mutateSession { updateRecord(targetId) { it.copy(sessionId = 0UL) } }
        closeSession(sessionId)
        return resetPersistentSession(targetId, text, cursorUtf8, source)
    }

    val candidateSessionId = createSession(targetId, text, cursorUtf8, true)
    if (candidateSessionId == null || candidateSessionId == 0UL) {
        Log.e(
            EditorSessionCoordinator.TAG,
            "resetPersistentSession($targetId): failed to create candidate session — old session preserved",
        )
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
        Log.e(
            EditorSessionCoordinator.TAG,
            "commitResetSnapshot($targetId): snapshot read failed — closing candidate $sessionId, " +
                "old session preserved",
        )
        closeSession(sessionId)
        return ExternalResetResult.Failed
    }
    mutateSession {
        val rec = record(targetId)
        putRecord(
            (rec ?: EditorSessionRecord(targetId = targetId, persistent = true)).copy(
                sessionId = sessionId,
                documentState =
                    (rec?.documentState ?: DocumentState()).copy(
                        revision = snapshot.revision,
                        selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                        selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    ),
            ),
        )
        if (sessionState.targetId == targetId) {
            sessionState =
                sessionState.copy(
                    sessionId = sessionId,
                    revision = snapshot.revision,
                    selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                    selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    origin = EditorSessionOrigin.EXTERNAL_REPLACE,
                )
        }
        val currentBinding = sessionState.bindingState
        if (currentBinding is WindowBindingState.Detached && currentBinding.targetId == targetId) {
            sessionState =
                sessionState.copy(
                    bindingState = WindowBindingState.Detached(targetId, sessionId, snapshot),
                )
        }
    }
    if (oldSessionIdToClose != null && oldSessionIdToClose != 0UL && oldSessionIdToClose != sessionId) {
        closeSession(oldSessionIdToClose)
    }
    return ExternalResetResult.Success(snapshot)
}

fun EditorSessionCoordinator.refreshDetachedSnapshot(targetId: String): TargetSnapshot? {
    val snapshot = queryTargetSnapshot(targetId)
    val state = windowBindingState
    if (state is WindowBindingState.Detached && state.targetId == targetId) {
        val sid = store.record(targetId)?.sessionId ?: return snapshot
        mutateSession {
            sessionState =
                sessionState.copy(bindingState = WindowBindingState.Detached(targetId, sid, snapshot))
        }
    }
    return snapshot
}

fun EditorSessionCoordinator.releaseHost() {
    if (activeTargetId != null) {
        cancelActiveSession()
    }
    val recordsToClose = store.allRecords().filter { it.sessionId != 0UL }
    mutateSession {
        recordsToClose.forEach { record ->
            coordinator.closeSession(record.sessionId)
        }
        clearRecords()
        sessionState = EditorSessionState(editingState = EditingState.RELEASED)
    }
}
