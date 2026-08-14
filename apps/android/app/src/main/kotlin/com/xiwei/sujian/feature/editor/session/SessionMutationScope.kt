package com.xiwei.sujian.feature.editor.session

/**
 * #624 评论17 问题1：会话变更临界区 — 在同一把锁内原子推进
 * `_sessionStateFlow.value` / [EditorSessionStore] / `inputLeaseEpoch`，
 * 消除 `MutableStateFlow.update` lambda CAS 重试导致的 state/store 分裂。
 *
 * 旧缺陷：`commitSavedLease` 等写操作用 `MutableStateFlow.update {}` lambda
 * 内写 `pendingRecord`，lambda 外再 `store.put`。update lambda 并发时可能被
 * 多次求值（CAS 重试）：保存线程 lambda 读 rev=N 写 committed=true/pendingRecord=N，
 * 输入线程推进到 rev=N+1/localDirty=true，保存线程 CAS 失败 lambda 以 N+1 重跑
 * 返回原 state，但外面 committed 仍 true、pendingRecord 仍 N，store.put(N 的 saved
 * record) 把 Store 回退到旧 revision/dirty=false。
 *
 * 新实现：所有写路径走 [EditorSessionCoordinator.mutateSession]，在
 * [mutationLock] 内一次性读当前 state/epoch、执行 block、写回 state/epoch。
 * [EditorSessionStore] 的 put/update/remove 只在 [SessionMutationScope] 内暴露，
 * 不再允许 gateway 外直接修改。
 *
 * 会话层不持有 Compose 状态；ReentrantLock 与现有同步语义一致（会话层方法
 * 非 suspend，调用方在 Dispatchers.IO 或主线程调用）。
 */
internal class SessionMutationScope(
    /** 持有 coordinator 引用 — block 内可调 closeSession/createSession 等外部 Core 操作。 */
    internal val coordinator: EditorSessionCoordinator,
    /** 受锁保护的 store — block 内通过 putRecord/updateRecord/removeRecord 修改。 */
    val store: EditorSessionStore,
    /** 当前 SessionState 快照（block 内可读写，block 结束后写回 StateFlow）。 */
    var sessionState: EditorSessionState,
    /** 当前 inputLeaseEpoch（block 内可读写，block 结束后写回）。 */
    var leaseEpoch: Long,
) {
    fun record(targetId: String): EditorSessionRecord? = store.record(targetId)

    fun allRecords(): List<EditorSessionRecord> = store.allRecords()

    fun isRegistered(targetId: String): Boolean = store.isRegistered(targetId)

    fun putRecord(record: EditorSessionRecord) {
        store.put(record)
    }

    fun updateRecord(
        targetId: String,
        transform: (EditorSessionRecord) -> EditorSessionRecord,
    ) {
        store.update(targetId, transform)
    }

    fun removeRecord(targetId: String): EditorSessionRecord? = store.remove(targetId)

    fun clearRecords() {
        store.clear()
    }

    /** 在临界区内使输入 lease 失效（等价于旧 invalidateInputLease）。 */
    fun invalidateLease() {
        leaseEpoch++
    }

    /**
     * 在临界区内校验事件 lease 是否仍匹配当前活动 target/session/epoch。
     * 与 [EditorSessionCoordinator.isInputLeaseCurrent] 同语义，但读 scope 快照。
     */
    fun isInputLeaseCurrent(
        lease: EditorInputLease?,
        eventTargetId: String? = null,
    ): Boolean {
        if (lease == null) return false
        if (lease.epoch != leaseEpoch) return false
        if (eventTargetId != null && lease.targetId != eventTargetId) return false
        val active = sessionState.activeTargetId
        if (active == null) return true
        if (active != lease.targetId) return false
        val expectedSession = sessionState.sessionId ?: 0UL
        return lease.sessionId == expectedSession
    }
}
