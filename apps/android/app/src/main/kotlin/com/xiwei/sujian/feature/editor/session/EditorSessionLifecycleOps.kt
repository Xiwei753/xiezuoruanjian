package com.xiwei.sujian.feature.editor.session

// ! # 编辑器会话生命周期操作（从 EditorSessionCoordinator 拆分）
// ! #624 评论17 问题1：走 mutateSession/readSession 单一临界区，删除 pendingRecord 外置副作用。
// ! #624 评论17 问题2：删除 prepared 假窗口 — commitPreparedSession 后 Detached/IDLE/activeTargetId=null。
// ! #624 评论17 问题2：窗口绑定判断+提交同一次 mutation，旧窗口晚到回调不能覆盖新绑定。
// ! #624 评论17 问题3：commitPreparedSession 单次 mutateSession CAS，删除 finalizePreparedSessionCommit 两段提交。
// ! #624 评论17 问题4：resetPersistentSession/commitResetSnapshot 带 SessionResetPrecondition CAS。

import android.util.Log
import com.xiwei.sujian.feature.editor.window.EditingState

fun EditorSessionCoordinator.prepareTargetSessionForCommit(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
): PreparedSessionHandle? {
    // #624 评论16 问题1：prepare 是无副作用事务 — 不关闭任何 prepare 之前就存在的 session，
    // 也不写 EditorSessionStore。所有权由 [PreparedSessionMode] 枚举明确表达。
    // #624 评论17 问题1：record 从 readSession 取 — 不在锁外读 store。
    val record = readSession { record(targetId) } ?: return null
    if (record.documentState.localDirty) return null
    val existingId = record.sessionId
    val existingValid = existingId != 0UL && validateSession(existingId)
    if (existingValid) {
        return prepareFromValidExistingSession(targetId, initialText, initialSelection, record, existingId)
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
 * #624 评论17 问题3：prepared commit 改成一次真正的 compare-and-swap。
 *
 * Core candidate 已在 prepare 阶段准备好 → 进入一次 mutateSession → 锁内重新校验
 * handle 对应 B 前置条件（target 仍是 handle.targetId；当前 record.sessionId 仍等于
 * prepare 时预期值；Replacement 时 oldSessionId 仍一致）→ 校验成功后在同一 mutation 内
 * 完成 invalidateLease、A 的 Kotlin store/state 退出、B record 写入 handle.sessionId/snapshot、
 * SessionState 切成 B 的 Detached、收集需锁外关闭的旧 Core sessionId → 解锁 →
 * closeSession(old A / replaced B old session)。
 *
 * 校验失败：不改 A、不改 B、不关闭 A、返回 false。
 *
 * 删除旧 finalizePreparedSessionCommit 两段提交 — 成功与否由这一次 mutation 返回值决定，
 * 不能无条件 return true。
 */
@Suppress("CyclomaticComplexMethod", "CognitiveComplexMethod")
fun EditorSessionCoordinator.commitPreparedSession(handle: PreparedSessionHandle): Boolean {
    // expectedSessionId 只读 handle，不读 store — 可在锁外计算。
    val expectedSessionId =
        when (handle.mode) {
            PreparedSessionMode.Borrowed -> handle.sessionId
            PreparedSessionMode.Created -> handle.previousRecord?.sessionId ?: 0UL
            is PreparedSessionMode.Replacement -> handle.mode.oldSessionId
        }

    val snapshot = handle.snapshot
    var committedProfile: TextEditorProfile? = null
    // 锁外关闭的旧 Core sessionId（A 的非持久/未绑定 session）。
    var pendingCloseSessionId: ULong = 0UL

    // 单次 mutateSession CAS：锁内校验 B 前置 + 完成 A 退出 + B 写入 + 收集锁外关闭 id。
    val committed =
        mutateSession {
            // 3. 锁内重新校验 handle 对应 B 前置条件。
            val entryRec = record(handle.targetId) ?: return@mutateSession false
            if (entryRec.sessionId != expectedSessionId) return@mutateSession false
            // 4. 校验成功 → 同一 mutation 内完成：
            invalidateLease()
            // A 的 Kotlin store/state 退出（若旧活动目标 A 不是 B）。
            val eviction = evictOldActiveForPreparedCommit(handle.targetId)
            pendingCloseSessionId = eviction.closeSessionId
            committedProfile = eviction.profile
            // B record 写入 handle.sessionId/snapshot。
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
            // SessionState 切成 B 的 Detached/IDLE/activeTargetId=null。
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
            true
        }
    if (!committed) return false
    // 5. 解锁 → 6. closeSession(old A / replaced B old session)
    if (pendingCloseSessionId != 0UL) {
        closeSession(pendingCloseSessionId)
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
): Boolean =
    // #624 评论17 问题1：从 readSession 取 binding — 不在锁外读 _sessionStateFlow.value。
    readSession {
        val currentBinding = sessionState.bindingState
        when (currentBinding) {
            is WindowBindingState.Attaching ->
                currentBinding.windowId != windowId || currentBinding.targetId != targetId
            is WindowBindingState.Attached ->
                currentBinding.windowId != windowId || currentBinding.targetId != targetId
            else -> false
        }
    }

/**
 * #624 评论17 问题2：detachWindowBinding 取准确 token（windowId+targetId+sessionId+bindingState），
 * Core snapshot/close 锁外执行，重新进 mutation 后 token 仍完全一致才允许
 * Detached/Idle/removeRecord。旧窗口的 onRelease 不能把刚刚重新绑定的同 target 新 session 清掉。
 */
@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.detachWindowBinding(
    windowId: String,
    targetId: String,
) {
    // readSession 取 token（sessionId + persistent + binding）。
    data class DetachToken(
        val sessionId: ULong,
        val isPersistent: Boolean,
        val binding: WindowBindingState,
    )
    val token =
        readSession {
            val rec = record(targetId)
            DetachToken(
                sessionId = rec?.sessionId ?: 0UL,
                isPersistent = rec?.persistent ?: false,
                binding = sessionState.bindingState,
            )
        }
    if (isBindingForDifferentWindow(windowId, targetId)) return
    if (token.binding is WindowBindingState.Detached && token.binding.targetId == targetId) return

    if (!token.isPersistent || token.sessionId == 0UL) {
        // 非持久 / 无 session：锁外 closeSession → 再进锁校验 token 仍完全一致才 removeRecord/Idle。
        if (token.sessionId != 0UL) {
            closeSession(token.sessionId)
        }
        mutateSession {
            // token 仍完全一致才允许 removeRecord/Idle — 旧窗口晚到不能清新绑定的 session。
            if (sessionState.bindingState != token.binding) return@mutateSession
            val currentRec = record(targetId)
            if (currentRec?.sessionId != token.sessionId) return@mutateSession
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
    // 持久：锁外 snapshot → 再进锁校验 token 仍完全一致才 Detached。
    val snapshot = if (validateSession(token.sessionId)) querySnapshotForSession(token.sessionId) else null
    val detached = WindowBindingState.Detached(targetId, token.sessionId, snapshot)
    mutateSession {
        if (sessionState.bindingState != token.binding) return@mutateSession
        val currentRec = record(targetId)
        if (currentRec?.sessionId != token.sessionId) return@mutateSession
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
        token.sessionId.toString(),
        "window_detached",
    )
}

/**
 * #624 评论17 问题2：completeWindowAttach 返回 Boolean — 单个 mutateSession 内校验
 * current.isExactAttaching(windowId,targetId,sessionId) 才写 Attached+EDITING 并返回 true，
 * 否则返回 false。旧窗口晚到回调不能覆盖新绑定。
 */
fun EditorSessionCoordinator.completeWindowAttach(
    windowId: String,
    targetId: String,
    sessionId: ULong,
): Boolean =
    mutateSession {
        val current = sessionState.bindingState
        if (current.isExactAttached(windowId, targetId, sessionId)) {
            return@mutateSession true
        }
        if (!current.isExactAttaching(windowId, targetId, sessionId)) {
            Log.w(
                EditorSessionCoordinator.TAG,
                "completeWindowAttach($windowId,$targetId,$sessionId): current state $current is not the " +
                    "exact Attaching for this window/target/session — ignoring (Attached requires a bound View)",
            )
            return@mutateSession false
        }
        sessionState =
            sessionState.copy(
                bindingState = WindowBindingState.Attached(windowId, targetId, sessionId),
                editingState = EditingState.EDITING,
            )
        true
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

/**
 * #624 评论17 问题2：closeTarget — readSession 取 token（sessionId + binding），锁外 closeSession，
 * 重新进 mutation 校验 record.sessionId 仍是 token.sessionId 且 binding 仍一致才 removeRecord/重置。
 * 锁外 closeSession 期间新窗口可能 attach 同 target，binding 校验防止清掉刚建立的新绑定。
 */
fun EditorSessionCoordinator.closeTarget(
    targetId: String,
    reason: SessionCloseReason,
) {
    val wasActive = activeTargetId == targetId
    if (wasActive) {
        commitActiveSession(null)
    }

    // #624 评论17 问题2：token 含 sessionId + binding — 与 detachWindowBinding 一致。
    data class CloseTargetToken(val sessionId: ULong?, val binding: WindowBindingState)
    val token = readSession { CloseTargetToken(record(targetId)?.sessionId, sessionState.bindingState) }
    if (token.sessionId != null && token.sessionId != 0UL) {
        closeSession(token.sessionId)
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
            token.sessionId.toString(),
            "close_target:${reason.name.lowercase()}",
        )
    }
    mutateSession {
        // token 仍完全一致才 removeRecord — 锁外 closeSession 期间新窗口 attach 不能被清掉。
        if (sessionState.bindingState != token.binding) return@mutateSession
        val currentRec = record(targetId)
        if (currentRec?.sessionId != token.sessionId) return@mutateSession
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
@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.prepareSessionForEdit(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
    windowId: String,
): SessionBindInfo? {
    // #624 评论17 问题1：record 从 readSession 取 — 不在锁外读 store。
    val record = readSession { record(targetId) } ?: return null
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

/**
 * #624 评论17 问题2：restampAttachingToWindow — 单个 mutation 内确认
 * record.sessionId + sessionState.targetId/sessionId + 当前 binding 仍属预期 session 后才 restamp。
 */
private fun EditorSessionCoordinator.restampAttachingToWindow(
    windowId: String,
    targetId: String,
) {
    mutateSession {
        val sid = record(targetId)?.sessionId ?: return@mutateSession
        if (sid == 0UL) return@mutateSession
        if (sessionState.targetId != targetId) return@mutateSession
        if (sessionState.sessionId != sid) return@mutateSession
        val currentWindowId = bindingWindowIdFor(sessionState.bindingState, targetId, sid)
        if (currentWindowId == null || currentWindowId == windowId) return@mutateSession
        invalidateLease()
        sessionState =
            sessionState.copy(
                bindingState = WindowBindingState.Attaching(windowId, targetId, sid),
                editingState = EditingState.BINDING,
            )
    }
}

private fun bindingWindowIdFor(
    binding: WindowBindingState,
    targetId: String,
    sessionId: ULong,
): String? =
    when (binding) {
        is WindowBindingState.Attaching ->
            binding.windowId.takeIf { binding.targetId == targetId && binding.sessionId == sessionId }
        is WindowBindingState.Attached ->
            binding.windowId.takeIf { binding.targetId == targetId && binding.sessionId == sessionId }
        else -> null
    }

private fun SessionMutationScope.isResetPreconditionStale(
    targetId: String,
    precondition: SessionResetPrecondition,
): Boolean {
    val rec = record(targetId)
    val currentSessionId = rec?.sessionId ?: 0UL
    val currentRevision = rec?.documentState?.revision ?: 0L
    return currentSessionId != precondition.oldSessionId ||
        currentRevision != precondition.oldRevision ||
        leaseEpoch != precondition.leaseEpoch
}

private data class OldActiveEvictionResult(
    val closeSessionId: ULong,
    val profile: TextEditorProfile?,
)

private fun SessionMutationScope.evictOldActiveForPreparedCommit(newTargetId: String): OldActiveEvictionResult {
    val oldActive = sessionState.activeTargetId
    if (oldActive == null || oldActive == newTargetId) return OldActiveEvictionResult(0UL, null)
    val oldRec = record(oldActive)
    val oldSessionId = oldRec?.sessionId
    if (oldSessionId == null || oldSessionId == 0UL) return OldActiveEvictionResult(0UL, null)
    val oldPersistent = oldRec.persistent
    val oldWindowBound =
        sessionState.bindingState is WindowBindingState.Attached ||
            sessionState.bindingState is WindowBindingState.Attaching
    val closeId =
        if (!oldPersistent || !oldWindowBound) {
            removeRecord(oldActive)
            oldSessionId
        } else {
            0UL
        }
    return OldActiveEvictionResult(closeId, oldRec.profile)
}

private fun EditorSessionCoordinator.prepareActiveSessionIfCurrent(targetId: String): SessionBindInfo? {
    // #624 评论17 问题1：从 readSession 取一致快照 — 不在锁外多处读 store。
    val snap =
        readSession {
            if (sessionState.activeTargetId != targetId ||
                (sessionState.editingState != EditingState.EDITING && sessionState.editingState != EditingState.BINDING)
            ) {
                return@readSession null
            }
            val rec = record(targetId) ?: return@readSession null
            Triple(rec.sessionId, rec.profile, rec.persistent)
        } ?: return null
    val sid = snap.first
    if (sid == 0UL) return null
    return SessionBindInfo(sid, snap.second, snap.third, snapshot = querySnapshotForSession(sid))
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
    // #624 评论17 问题1：sessionId 从 readSession 取 — 不在锁外读 store。
    val existingId = readSession { record(targetId)?.sessionId }
    if (existingId != null && existingId != 0UL && validateSession(existingId)) {
        return existingId
    }
    if (existingId != null && existingId != 0UL) {
        closeSession(existingId)
    }
    return createSession(targetId, textForSession, sel, isPersistent)
}

@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.commitActiveSession(finalText: String?): Boolean {
    var committed = false
    var profile: TextEditorProfile? = null
    // #624 评论17 问题3：锁内读取待关闭的 id/状态 → 锁外 closeSession →
    // 再进锁校验前提仍成立并提交。Core 调用不得持 mutationLock。
    var pendingTargetId: String? = null
    var pendingCloseSessionId: ULong = 0UL
    var needClose = false
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
        pendingTargetId = targetId
        if (!isPersistent || !windowBound) {
            pendingCloseSessionId = sessionId
            needClose = true
        }
        profile = rec.profile
        committed = true
    }
    // 锁外关闭 Core session — Core 调用不得持 mutationLock。
    if (committed && needClose && pendingCloseSessionId != 0UL) {
        closeSession(pendingCloseSessionId)
    }
    // 再进锁校验前提仍成立并提交 — 锁外 closeSession 期间活动 target 可能被其他线程
    // 改换，只有仍匹配才 removeRecord 并重置 sessionState，避免误删/覆盖新状态。
    if (committed) {
        mutateSession {
            if (sessionState.activeTargetId == pendingTargetId) {
                if (needClose && pendingTargetId != null) {
                    val currentRec = record(pendingTargetId)
                    if (currentRec?.sessionId == pendingCloseSessionId) {
                        removeRecord(pendingTargetId)
                    }
                }
                sessionState = EditorSessionState()
            } else {
                // #624 评论17 问题4：锁外 closeSession 期间活动 target 被改换，
                // 复位自己设的 COMMITTING + Committing(pendingTargetId) 中间态，
                // 避免 editorAttachDecision 对 Committing 返回 Hold 导致新 target
                // 附着 LaunchedEffect 持续不触发 beginEdit → 永久卡死。
                if (sessionState.editingState == EditingState.COMMITTING) {
                    sessionState = sessionState.copy(editingState = EditingState.IDLE)
                }
                val binding = sessionState.bindingState
                if (binding is WindowBindingState.Committing && binding.targetId == pendingTargetId) {
                    sessionState = sessionState.copy(bindingState = WindowBindingState.Idle)
                }
            }
        }
        _lastCommittedTextFlow.value =
            if (profile?.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
    }
    return committed
}

@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.cancelActiveSession(): Boolean {
    var cancelled = false
    // #624 评论17 问题3：锁内读取待关闭的 id/状态 → 锁外 closeSession →
    // 再进锁校验前提仍成立并提交。Core 调用不得持 mutationLock。
    var pendingCloseTargetId: String? = null
    var pendingCloseSessionId: ULong = 0UL
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
        pendingCloseTargetId = targetId
        pendingCloseSessionId = sessionId
        cancelled = true
    }
    // 锁外关闭 Core session — Core 调用不得持 mutationLock。
    if (cancelled && pendingCloseSessionId != 0UL) {
        closeSession(pendingCloseSessionId)
    }
    // 再进锁校验前提仍成立并提交 — 锁外 closeSession 期间活动 target 可能被其他线程
    // 改换，只有仍匹配才 removeRecord 并重置 sessionState，避免误删/覆盖新状态。
    if (cancelled) {
        mutateSession {
            if (pendingCloseTargetId != null && sessionState.activeTargetId == pendingCloseTargetId) {
                val currentRec = record(pendingCloseTargetId)
                if (currentRec?.sessionId == pendingCloseSessionId) {
                    removeRecord(pendingCloseTargetId)
                }
                sessionState = EditorSessionState()
            } else {
                // #624 评论17 问题4：锁外 closeSession 期间活动 target 被改换，
                // 复位自己设的 CANCELLING + Cancelling(pendingCloseTargetId) 中间态，
                // 避免 editorAttachDecision 对 Cancelling 返回 Hold 导致新 target
                // 附着 LaunchedEffect 持续不触发 beginEdit → 永久卡死。
                if (sessionState.editingState == EditingState.CANCELLING) {
                    sessionState = sessionState.copy(editingState = EditingState.IDLE)
                }
                val binding = sessionState.bindingState
                if (binding is WindowBindingState.Cancelling && binding.targetId == pendingCloseTargetId) {
                    sessionState = sessionState.copy(bindingState = WindowBindingState.Idle)
                }
            }
        }
    }
    return cancelled
}

/**
 * #624 评论17 问题4：persistent reset 带 SessionResetPrecondition — readSession 捕获 precondition →
 * 锁外 createSession/querySnapshotForSession → mutateSession 内重新校验 precondition
 * 完全一致才 swap candidate。已 stale：关闭本次 candidate，保留当前新 session，不得覆盖。
 */
@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.resetPersistentSession(
    targetId: String,
    text: String,
    cursorUtf8: Int,
    source: SessionResetSource = SessionResetSource.EXTERNAL,
): ExternalResetResult {
    if (source == SessionResetSource.LOCAL_CONTENT_CHANGED) return ExternalResetResult.Failed

    // #624 评论17 问题4：readSession 捕获 precondition + persistent 检查。
    data class ResetPreRead(val persistent: Boolean, val precondition: SessionResetPrecondition)
    val preRead =
        readSession {
            val rec = record(targetId)
            ResetPreRead(
                persistent = rec?.persistent ?: false,
                precondition =
                    SessionResetPrecondition(
                        targetId = targetId,
                        oldSessionId = rec?.sessionId ?: 0UL,
                        oldRevision = rec?.documentState?.revision ?: 0L,
                        leaseEpoch = leaseEpoch,
                    ),
            )
        }
    if (!preRead.persistent) return ExternalResetResult.Failed

    val precondition = preRead.precondition
    val sessionId = precondition.oldSessionId
    if (sessionId == 0UL) {
        val newSessionId = createSession(targetId, text, cursorUtf8, true)
        if (newSessionId == null || newSessionId == 0UL) {
            Log.e(
                EditorSessionCoordinator.TAG,
                "resetPersistentSession($targetId): failed to create session for empty/missing persistent session",
            )
            return ExternalResetResult.Failed
        }
        return commitResetSnapshot(targetId, newSessionId, precondition, oldSessionIdToClose = null)
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
    return commitResetSnapshot(targetId, candidateSessionId, precondition, oldSessionIdToClose = sessionId)
}

/**
 * #624 评论17 问题4：commitResetSnapshot — 带 precondition CAS — mutateSession 内重新校验
 * precondition 完全一致才 swap candidate。已 stale：关闭本次 candidate，保留当前新 session，
 * 返回 [ExternalResetResult.Stale]。
 */
fun EditorSessionCoordinator.commitResetSnapshot(
    targetId: String,
    sessionId: ULong,
    precondition: SessionResetPrecondition,
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
    // mutateSession 内重新校验 precondition 完全一致才 swap candidate。
    val stale =
        mutateSession {
            if (isResetPreconditionStale(targetId, precondition)) {
                // 已 stale — 不 swap，返回 true 表示 stale。
                return@mutateSession true
            }
            // CAS 通过后读取当前 record — 反映 precondition 校验通过后的状态。
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
                    sessionState.copy(bindingState = WindowBindingState.Detached(targetId, sessionId, snapshot))
            }
            false
        }
    if (stale) {
        // #624 评论17 问题4：已 stale — 关闭本次 candidate，保留当前新 session，不覆盖。
        closeSession(sessionId)
        return ExternalResetResult.Stale
    }
    if (oldSessionIdToClose != null && oldSessionIdToClose != 0UL && oldSessionIdToClose != sessionId) {
        closeSession(oldSessionIdToClose)
    }
    return ExternalResetResult.Success(snapshot)
}

/**
 * #624 评论17 问题2：refreshDetachedSnapshot — readSession 取 token（sessionId + binding），
 * 锁外 snapshot，重新进 mutation 校验 token 仍一致才更新 binding。
 */
fun EditorSessionCoordinator.refreshDetachedSnapshot(targetId: String): TargetSnapshot? {
    data class RefreshToken(val sessionId: ULong, val binding: WindowBindingState)
    val token =
        readSession {
            val sid = record(targetId)?.sessionId ?: 0UL
            RefreshToken(sid, sessionState.bindingState)
        }
    val snapshot = if (token.sessionId != 0UL) querySnapshotForSession(token.sessionId) else null
    if (token.binding is WindowBindingState.Detached && token.binding.targetId == targetId) {
        mutateSession {
            // token 仍一致才更新 — 锁外 snapshot 期间 binding 可能被改换。
            if (sessionState.bindingState != token.binding) return@mutateSession
            val currentRec = record(targetId)
            if (currentRec?.sessionId != token.sessionId) return@mutateSession
            sessionState =
                sessionState.copy(bindingState = WindowBindingState.Detached(targetId, token.sessionId, snapshot))
        }
    }
    return snapshot
}

fun EditorSessionCoordinator.releaseHost() {
    if (activeTargetId != null) {
        cancelActiveSession()
    }
    // #624 评论17 问题1/3：records snapshot 从 readSession 取 — 不在锁外读 store。
    val recordsToClose = readSession { allRecords().filter { it.sessionId != 0UL } }
    // 锁外关闭所有 Core session — Core 调用不得持 mutationLock。
    recordsToClose.forEach { record ->
        closeSession(record.sessionId)
    }
    // 再进锁清理记录并设 RELEASED 状态。
    mutateSession {
        clearRecords()
        sessionState = EditorSessionState(editingState = EditingState.RELEASED)
    }
}
