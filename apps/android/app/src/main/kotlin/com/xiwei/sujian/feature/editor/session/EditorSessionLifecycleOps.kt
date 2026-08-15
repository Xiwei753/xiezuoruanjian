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
        // #624 评论5294575627 要求1：先认领，再关闭 — mutateSession 内校验 binding+record.sessionId
        // 仍属本次操作 → removeRecord → 重置 sessionState → 返回独占的 SessionCloseClaim；
        // 解锁后才 closeSession(claim.sessionId)。认领失败不调 closeSession，避免锁外 closeSession
        // 期间同 target 被新窗口重新绑定后旧操作先把 Rust session 关掉。
        // #624 评论5294575627 验证修复3：走到非持久分支说明已通过前面的守卫（不是同 targetId
        // 的 Detached 幂等 no-op —— 那在行 270 已 return；也不是不同窗口的 Attaching/Attached ——
        // 那在 isBindingForDifferentWindow 已 return），所以这是一个有效的 detach 事件，先无条件
        // invalidateLease 让旧 View lease 失效；removeRecord 仍由 binding/record CAS 守护，
        // 认领失败不 removeRecord、不 closeSession。
        val claim =
            mutateSession {
                invalidateLease()
                // token 仍完全一致才允许认领 — 旧窗口晚到不能清新绑定的 session。
                if (sessionState.bindingState != token.binding) return@mutateSession null
                val currentRec = record(targetId)
                if (currentRec?.sessionId != token.sessionId) return@mutateSession null
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
                SessionCloseClaim(targetId = targetId, sessionId = token.sessionId)
            }
        // 解锁后关闭本次独占的 Core session — 认领失败时 claim 为 null，不调 closeSession。
        if (claim != null && claim.sessionId != 0UL) {
            closeSession(claim.sessionId)
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
 * #624 评论5294575627 要求1：closeTarget — 先认领，再关闭。
 *
 * wasActive 时先 commitActiveSession(null)（它内部也是"先认领再 close"），再由
 * mutateSession 内校验 binding+record.sessionId 仍属本次操作 → invalidateLease →
 * removeRecord → 重置 sessionState → 返回独占的 SessionCloseClaim；解锁后才
 * closeSession(claim.sessionId)。认领失败不调 closeSession，避免锁外 closeSession
 * 期间同 target 被新窗口重新绑定后旧操作先把 Rust session 关掉。
 */
fun EditorSessionCoordinator.closeTarget(
    targetId: String,
    reason: SessionCloseReason,
) {
    val wasActive = activeTargetId == targetId
    if (wasActive) {
        commitActiveSession(null)
    }

    data class CloseTargetToken(val sessionId: ULong?, val binding: WindowBindingState)
    val token = readSession { CloseTargetToken(record(targetId)?.sessionId, sessionState.bindingState) }
    val claim =
        mutateSession {
            // token 仍完全一致才允许认领 — 锁外期间新窗口 attach 不能被清掉。
            if (sessionState.bindingState != token.binding) return@mutateSession null
            val currentRec = record(targetId)
            if (currentRec?.sessionId != token.sessionId) return@mutateSession null
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
            SessionCloseClaim(targetId = targetId, sessionId = token.sessionId ?: 0UL)
        }
    // 解锁后关闭本次独占的 Core session — 认领失败时 claim 为 null，不调 closeSession。
    if (claim != null && claim.sessionId != 0UL) {
        closeSession(claim.sessionId)
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.sessionLifecycle(
            claim.sessionId.toString(),
            "close_target:${reason.name.lowercase()}",
        )
    }
}

/**
 * #624 评论17 问题2：准备会话绑定 — 删除 "prepared" 假窗口默认参数。
 * 真实窗口层必须传入真实 windowId。
 *
 * #624 评论5294575627 要求2：host 已释放（editingState==RELEASED）时直接拒绝，不允许重新塞记录。
 * #624 评论5294575627 要求3：readSession 捕获 [SessionBindPrecondition] → 锁外 resolve/query →
 * [commitPreparedBindingState] 带 precondition CAS 才写 Attaching。stale 时 Created 关闭 candidate、
 * Borrowed 不关闭，返回 null 让上层重新按当前状态发起 bind。
 */
@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.prepareSessionForEdit(
    targetId: String,
    initialText: String,
    initialSelection: Int?,
    windowId: String,
): SessionBindInfo? {
    // #624 评论17 问题1：record 从 readSession 取 — 不在锁外读 store。
    // #624 评论5294575627 要求2：同时检查 RELEASED — host 已释放后拒绝。
    data class PrepareInitialRead(val record: EditorSessionRecord?, val isReleased: Boolean)
    val initial =
        readSession {
            PrepareInitialRead(
                record = record(targetId),
                isReleased = sessionState.editingState == EditingState.RELEASED,
            )
        }
    if (initial.isReleased) return null
    val record = initial.record ?: return null
    val isPersistent = record.persistent
    val profile = record.profile

    rebindFromOtherActiveIfNeeded(targetId)

    // #624 评论5294575627 要求3：rebind 之后捕获 SessionBindPrecondition — 锁外 resolve/query 期间
    // 同 target 可能换了 session/revision/binding，commitPreparedBindingState 内重新校验
    // precondition 完全一致才 putRecord + 写 Attaching。
    val precondition =
        readSession {
            val rec = record(targetId)
            SessionBindPrecondition(
                targetId = targetId,
                oldSessionId = rec?.sessionId ?: 0UL,
                oldRevision = rec?.documentState?.revision ?: 0L,
                leaseEpoch = leaseEpoch,
                bindingState = sessionState.bindingState,
                stateTargetId = sessionState.targetId,
                stateSessionId = sessionState.sessionId,
                activeTargetId = sessionState.activeTargetId,
                editingState = sessionState.editingState,
            )
        }

    val textForSession = initialText
    val sel = initialSelection ?: textForSession.toByteArray(Charsets.UTF_8).size
    val prepared = resolveSessionForPrepare(targetId, textForSession, sel, isPersistent)

    if (prepared == null) return null

    val sessionId = prepared.sessionId
    val attaching = WindowBindingState.Attaching(windowId, targetId, sessionId)
    val snapshot = querySnapshotForSession(sessionId)
    if (snapshot == null) {
        // #624 评论5294575627 要求3：snapshot == null — Created 关闭本事务 candidate；
        // Borrowed 不关闭。不能拿 snapshot=null 继续进入 commitPreparedBindingState 写 revision=0。
        if (prepared is PreparedBindSession.Created) {
            closeSession(prepared.sessionId)
        }
        return null
    }
    val committed =
        commitPreparedBindingState(textForSession, sel, snapshot, attaching, precondition)
    if (!committed) {
        // #624 评论5294575627 要求3：precondition stale — 本次新建的 candidate 关闭；
        // 复用的既有 session 不关闭。返回 null 让上层重新按当前状态发起 bind。
        if (prepared is PreparedBindSession.Created) {
            closeSession(prepared.sessionId)
        }
        return null
    }
    return SessionBindInfo(sessionId, profile, isPersistent, snapshot = snapshot)
}

/**
 * #624 评论5294575627 要求3：commitPreparedBindingState 返回 Boolean — mutateSession 内重新校验
 * [SessionBindPrecondition] 完全一致才 putRecord + 写 Attaching 并返回 true；否则返回 false（不写）。
 * 删除旧的无条件提交。targetId/sessionId 从 precondition/attaching 派生（调用方保证一致），
 * 不重复传参（避免 detekt LongParameterList）。
 *
 * #624 评论5294575627 要求3（收口）：[snapshot] 由调用方保证非空 — null 在 prepareSessionForEdit
 * 已提前 return（Created 关闭 candidate、Borrowed 不关闭），不进入本函数写 revision=0。
 */
private fun EditorSessionCoordinator.commitPreparedBindingState(
    textForSession: String,
    sel: Int,
    snapshot: TargetSnapshot,
    attaching: WindowBindingState.Attaching,
    precondition: SessionBindPrecondition,
): Boolean {
    val targetId = precondition.targetId
    val sessionId = attaching.sessionId
    return mutateSession {
        // precondition CAS — 完全一致才 putRecord + 写 Attaching。
        if (isBindPreconditionStale(targetId, precondition)) return@mutateSession false
        // #624 评论5294575627 要求2：跨窗口 restamp 时使旧窗口 input lease 失效 —
        // 旧 fast path 的 restampAttachingToWindow 负责递增 epoch，删除后由本 CAS 路径承担。
        invalidateLease()
        val currentRec = record(targetId)
        putRecord(
            currentRec?.copy(
                sessionId = sessionId,
                documentState =
                    currentRec.documentState.copy(
                        revision = snapshot.revision,
                        selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                        selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    ),
            ) ?: EditorSessionRecord(
                targetId = targetId,
                sessionId = sessionId,
                documentState =
                    DocumentState(
                        revision = snapshot.revision,
                        selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                        selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    ),
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
        true
    }
}

/**
 * #624 评论5294575627 要求3：bind precondition stale 判定 — 与 [isResetPreconditionStale] 同构，
 * 额外校验 bindingState 仍一致（锁外 resolve/query 期间同 target 的 binding 可能被改换）。
 */
private fun SessionMutationScope.isBindPreconditionStale(
    targetId: String,
    precondition: SessionBindPrecondition,
): Boolean {
    val rec = record(targetId)
    val currentSessionId = rec?.sessionId ?: 0UL
    val currentRevision = rec?.documentState?.revision ?: 0L
    return currentSessionId != precondition.oldSessionId ||
        currentRevision != precondition.oldRevision ||
        leaseEpoch != precondition.leaseEpoch ||
        sessionState.bindingState != precondition.bindingState ||
        sessionState.targetId != precondition.stateTargetId ||
        sessionState.sessionId != precondition.stateSessionId ||
        sessionState.activeTargetId != precondition.activeTargetId ||
        sessionState.editingState != precondition.editingState
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

private fun EditorSessionCoordinator.rebindFromOtherActiveIfNeeded(targetId: String) {
    if (activeTargetId == null || activeTargetId == targetId) return
    mutateSession {
        sessionState = sessionState.copy(editingState = EditingState.REBINDING)
    }
    if (!commitActiveSession(null)) {
        cancelActiveSession()
    }
}

/**
 * #624 评论5294575627 要求3：resolveSessionForPrepare 返回 [PreparedBindSession] —
 * 既有有效 session 返回 [PreparedBindSession.Borrowed]（stale 时不关闭）；
 * 否则锁外 create 返回 [PreparedBindSession.Created]（stale 时关闭 candidate）。
 */
private fun EditorSessionCoordinator.resolveSessionForPrepare(
    targetId: String,
    textForSession: String,
    sel: Int,
    isPersistent: Boolean,
): PreparedBindSession? {
    // #624 评论17 问题1：sessionId 从 readSession 取 — 不在锁外读 store。
    val existingId = readSession { record(targetId)?.sessionId }
    if (existingId != null && existingId != 0UL && validateSession(existingId)) {
        return PreparedBindSession.Borrowed(existingId)
    }
    if (existingId != null && existingId != 0UL) {
        // existingId 已无效（validateSession 返回 false）— 关闭是 no-op 防御，不影响新 session。
        closeSession(existingId)
    }
    val newId = createSession(targetId, textForSession, sel, isPersistent)
    if (newId == null || newId == 0UL) return null
    return PreparedBindSession.Created(newId)
}

@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.commitActiveSession(finalText: String?): Boolean {
    // #624 评论5294575627 要求1：先认领，再 close — 一次 mutateSession 完成 Kotlin 所有权转移：
    // invalidateLease → 需要关闭则 removeRecord + 收集 closeSessionId → sessionState 直接落 Idle →
    // 解锁后只有 closeSessionId != 0 才 closeSession。不再写 COMMITTING → close → 第二次 mutation。
    var profile: TextEditorProfile? = null
    var pendingCloseSessionId: ULong = 0UL
    val committed =
        mutateSession {
            val targetId = sessionState.activeTargetId ?: return@mutateSession false
            val rec = record(targetId) ?: return@mutateSession false
            val sessionId = rec.sessionId
            if (sessionId == 0UL) return@mutateSession false
            val isPersistent = rec.persistent
            val windowBound =
                sessionState.bindingState is WindowBindingState.Attached ||
                    sessionState.bindingState is WindowBindingState.Attaching
            invalidateLease()
            if (!isPersistent || !windowBound) {
                removeRecord(targetId)
                pendingCloseSessionId = sessionId
            }
            profile = rec.profile
            sessionState = EditorSessionState()
            true
        }
    if (!committed) return false
    if (pendingCloseSessionId != 0UL) {
        closeSession(pendingCloseSessionId)
    }
    _lastCommittedTextFlow.value =
        if (profile?.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
    return true
}

@Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod")
fun EditorSessionCoordinator.cancelActiveSession(): Boolean {
    // #624 评论5294575627 要求1：先认领，再 close — 一次 mutateSession 完成 Kotlin 所有权转移：
    // invalidateLease → removeRecord + 收集 closeSessionId → sessionState 直接回 Idle →
    // 解锁后 closeSession。不再写 CANCELLING → close → 第二次 mutation。
    var pendingCloseSessionId: ULong = 0UL
    val cancelled =
        mutateSession {
            val targetId = sessionState.activeTargetId ?: return@mutateSession false
            val rec = record(targetId) ?: return@mutateSession false
            val sessionId = rec.sessionId
            if (sessionId == 0UL) return@mutateSession false
            invalidateLease()
            removeRecord(targetId)
            pendingCloseSessionId = sessionId
            sessionState = EditorSessionState()
            true
        }
    if (!cancelled) return false
    if (pendingCloseSessionId != 0UL) {
        closeSession(pendingCloseSessionId)
    }
    return true
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
        // #624 评论5294575627 要求4：旧 session 已失效 — 不再把 record 写成 0UL + 递归。
        // 直接锁外 create candidate → commitResetSnapshot(candidate, 同一个 precondition,
        // oldSessionIdToClose = oldSessionId)。CAS 决定能否 swap；stale 关闭 candidate；
        // 成功后再关闭旧失效 session。reset 只有一套提交语义（commitResetSnapshot）。
        Log.w(
            EditorSessionCoordinator.TAG,
            "resetPersistentSession($targetId): session $sessionId no longer valid, " +
                "recreating via commitResetSnapshot CAS",
        )
        val candidateSessionId = createSession(targetId, text, cursorUtf8, true)
        if (candidateSessionId == null || candidateSessionId == 0UL) {
            Log.e(
                EditorSessionCoordinator.TAG,
                "resetPersistentSession($targetId): failed to create candidate session for invalid old " +
                    "session — old session preserved",
            )
            return ExternalResetResult.Failed
        }
        return commitResetSnapshot(targetId, candidateSessionId, precondition, oldSessionIdToClose = sessionId)
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

/**
 * #624 评论5294575627 要求2：releaseHost — 先一次性拿走所有 session 所有权，再锁外逐个 close。
 *
 * 旧实现 readSession{allRecords()} → 锁外逐个 closeSession → mutateSession{clearRecords;RELEASED}
 * 在锁外 close 循环期间若又注册/创建了 session，最后 clearRecords() 会把新记录一起清掉，
 * 而新 session 又不在旧 recordsToClose 列表里，形成孤儿 Core session。
 *
 * 改成：mutateSession{收集当前所有非零 sessionId; invalidateLease; clearRecords;
 * sessionState=RELEASED; 返回 sessionId 列表} → 解锁后逐个 closeSession。
 *
 * #624 评论5294575627 要求1（收口）：不再先额外走一次 cancelActiveSession — 单次 mutateSession
 * 已收走全部 session（active 记录也在 allRecords() 内），避免两次进锁期间状态被改换的竞态。
 */
fun EditorSessionCoordinator.releaseHost() {
    // 单次 mutateSession 内一次性拿走所有 session 所有权 — invalidateLease + clearRecords + RELEASED。
    val sessionIdsToClose =
        mutateSession {
            val ids = allRecords().map { it.sessionId }.filter { it != 0UL }
            invalidateLease()
            clearRecords()
            sessionState = EditorSessionState(editingState = EditingState.RELEASED)
            ids
        }
    // 解锁后逐个 closeSession — Core 调用不得持 mutationLock。
    sessionIdsToClose.forEach { sessionId ->
        closeSession(sessionId)
    }
}
