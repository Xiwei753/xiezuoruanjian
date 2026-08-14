package com.xiwei.sujian.feature.editor.session

// ! # 编辑器会话外部文档操作（从 EditorSessionCoordinator 拆分）

fun EditorSessionCoordinator.shouldApplyExternalContent(fact: TargetDocumentFact): ExternalContentDecision {
    if (fact.sourceVersion.isEmpty) return ExternalContentDecision.IgnoreEmptyVersion
    // #595 二/四：比较基于该 target 的 store 记录文档事实（committedVersion /
    // localDirty）— 与可观察 SessionState 是否恰好指向活动 target 无关，
    // 新 collector 读到的是当前文档事实，重放旧事实幂等忽略。
    val doc = store.record(fact.targetId)?.documentState ?: DocumentState()
    if (doc.committedVersion == fact.sourceVersion) return ExternalContentDecision.IgnoreReplay
    if (doc.localDirty) return ExternalContentDecision.IgnoreDirtyConflict
    if (isVersionOlder(doc.committedVersion, fact.sourceVersion)) return ExternalContentDecision.IgnoreOlder
    // #624 评论9：DocumentState.text 已删除 — 需要比较正文时低频调用
    // [queryTargetSnapshot] 取 Core snapshot.text（冷路径，非每键热路径）。
    // snapshot.text == fact.text 时无需 reset；否则继续 Apply 判定。
    val snapshot = queryTargetSnapshot(fact.targetId)
    if (snapshot != null && snapshot.text == fact.text) return ExternalContentDecision.IgnoreSameContent
    // 空 committed（从未建立版本事实）→ 首次应用（章节首次加载）。
    if (doc.committedVersion.isEmpty) return ExternalContentDecision.Apply
    // #595 四：正文不同且版本不可比较 — Repository 加载（用户主动打开章节）
    // 信任磁盘内容直接 Apply；同步合并不得盲目覆盖（Git 回退/外部修改/迟到 IO），
    // 进入 IgnoreUncomparableConflict 由调用方走重新读取/三方合并/冲突路径。
    if (!isComparable(doc.committedVersion, fact.sourceVersion)) {
        return if (fact.origin == DocumentFactOrigin.REPOSITORY_LOAD) {
            ExternalContentDecision.Apply
        } else {
            ExternalContentDecision.IgnoreUncomparableConflict
        }
    }
    return ExternalContentDecision.Apply
}

fun EditorSessionCoordinator.isVersionOlder(
    committed: DocumentVersion,
    incoming: DocumentVersion,
): Boolean {
    if (committed.repositoryRevision != 0L && incoming.repositoryRevision != 0L) {
        return incoming.repositoryRevision < committed.repositoryRevision
    }
    return false
}

fun EditorSessionCoordinator.isComparable(
    committed: DocumentVersion,
    incoming: DocumentVersion,
): Boolean {
    if (committed.contentHash.isNotEmpty() && committed.contentHash == incoming.contentHash) return true
    if (committed.repositoryRevision != 0L && incoming.repositoryRevision != 0L) return true
    var parent = incoming.parentVersion
    while (parent != null) {
        if (parent == committed) return true
        if (committed.contentHash.isNotEmpty() && parent.contentHash == committed.contentHash) return true
        parent = parent.parentVersion
    }
    return false
}

fun EditorSessionCoordinator.applyExternalContentFact(fact: TargetDocumentFact) {
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { previous ->
        val record = store.record(fact.targetId)
        val previousDoc = record?.documentState ?: DocumentState()
        val newDoc =
            previousDoc.copy(
                committedVersion = fact.sourceVersion,
                sessionBaseVersion = fact.sourceVersion,
                lastSavedVersion = fact.sourceVersion,
                localDirty = false,
            )
        pendingRecord = record?.copy(documentState = newDoc)
            ?: EditorSessionRecord(targetId = fact.targetId, documentState = newDoc)
        // 无活动 target（state.targetId == null）时同样把文档事实反映到可观察状态；
        // 活动 target 属于其他章节时只更新 store 记录，不清掉活动状态。
        if (previous.targetId != fact.targetId && previous.targetId != null) return@updateSessionState previous
        previous.copy(
            committedVersion = fact.sourceVersion,
            sessionBaseVersion = fact.sourceVersion,
            localDirty = false,
            origin =
                if (fact.origin == DocumentFactOrigin.SYNC_MERGED) {
                    EditorSessionOrigin.SYNC_MERGED
                } else {
                    EditorSessionOrigin.EXTERNAL_REPLACE
                },
        )
    }
    pendingRecord?.let { store.put(it) }
}

fun EditorSessionCoordinator.markSaved(
    targetId: String,
    savedVersion: DocumentVersion,
) {
    if (savedVersion.isEmpty) return
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { state ->
        val record = store.record(targetId)
        if (record != null) {
            pendingRecord =
                record.withDocumentState {
                    it.copy(
                        committedVersion = savedVersion,
                        sessionBaseVersion = savedVersion,
                        lastSavedVersion = savedVersion,
                        localDirty = false,
                    )
                }
        }
        if (state.targetId == targetId) {
            state.copy(
                committedVersion = savedVersion,
                sessionBaseVersion = savedVersion,
                localDirty = false,
            )
        } else {
            state
        }
    }
    pendingRecord?.let { store.put(it) }
}

fun EditorSessionCoordinator.documentCommittedVersionFor(targetId: String): DocumentVersion =
    store.record(targetId)?.documentState?.committedVersion ?: DocumentVersion()

/**
 * #624 评论16 问题2：保存回执原子提交 — 一次完成 target/session/epoch/revision 校验
 * + store/sessionState 的 markSaved，不存在"先检查 lease/revision，再单独调用 markSaved"
 * 的竞态窗口。
 *
 * ViewModel 不再先 [isDocumentOperationLeaseCurrent] 再 [markSaved]（两步操作有竞态：
 * 保存线程检查 rev=N 仍有效 → 主线程输入推进到 N+1/localDirty=true → 保存线程再
 * markSaved()，把 N+1 的 localDirty 清成 false）。
 *
 * 校验全通过（target/session/epoch/revision 都匹配）时原子推进
 * committedVersion/sessionBaseVersion/lastSavedVersion 并清 localDirty；
 * 任一不匹配时返回 false，不清 dirty（新输入没落盘不得标成已保存）。
 */
fun EditorSessionCoordinator.commitSavedLease(
    lease: DocumentOperationLease,
    savedVersion: DocumentVersion,
): Boolean {
    if (savedVersion.isEmpty) return false
    var committed = false
    var pendingRecord: EditorSessionRecord? = null
    updateSessionState { state ->
        if (!isSavedLeaseMatchingState(state, lease)) return@updateSessionState state
        committed = true
        pendingRecord = savedRecordFor(lease.targetId, savedVersion)
        state.copy(
            committedVersion = savedVersion,
            sessionBaseVersion = savedVersion,
            localDirty = false,
        )
    }
    pendingRecord?.let { store.put(it) }
    return committed
}

/** #624 评论16 问题2：原子校验 target/session/epoch/revision 是否全匹配。 */
private fun EditorSessionCoordinator.isSavedLeaseMatchingState(
    state: EditorSessionState,
    lease: DocumentOperationLease,
): Boolean =
    state.activeTargetId == lease.targetId &&
        state.sessionId == lease.coreSessionId &&
        inputLeaseEpoch == lease.inputEpoch &&
        state.revision == lease.rustRevision

/** #624 评论16 问题2：构造保存后的 store 记录（committedVersion/baseVersion/dirty 推进）。 */
private fun EditorSessionCoordinator.savedRecordFor(
    targetId: String,
    savedVersion: DocumentVersion,
): EditorSessionRecord? {
    val record = store.record(targetId) ?: return null
    return record.withDocumentState {
        it.copy(
            committedVersion = savedVersion,
            sessionBaseVersion = savedVersion,
            lastSavedVersion = savedVersion,
            localDirty = false,
        )
    }
}
