package com.xiwei.sujian.feature.editor.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #624 评论17 问题1：SessionMutationGate 并发契约测试。
 *
 * 复现旧实现缺陷：commitSavedLease 等写操作用 MutableStateFlow.update {} lambda
 * 内写 pendingRecord，lambda 外再 store.put。Kotlin 官方文档明确 update lambda
 * 并发时可能被多次求值（CAS 重试），导致 StateFlow 与 Store 分裂：
 *
 * - 保存线程 lambda 读 rev=N 写 committed=true/pendingRecord=N；
 * - 输入线程推进到 rev=N+1/localDirty=true；
 * - 保存线程 CAS 失败 lambda 以 N+1 重跑返回原 state，但外面 committed 仍 true、
 *   pendingRecord 仍 N，store.put(N 的 saved record) 把 Store 回退到旧 revision/dirty=false。
 *
 * 新实现用 [mutateSession] 单一临界区统一保护 _sessionStateFlow.value、
 * [EditorSessionStore]、inputLeaseEpoch，state 与 store 原子一致。
 */
class SessionMutationGateConcurrencyTest {
    private fun baseBridge(): com.xiwei.sujian.core.interop.app.AppServiceBridge =
        com.xiwei.sujian.core.interop.app.AppServiceBridge(
            com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_gate",
                "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_gate",
            ),
        )

    /**
     * 复现：commitSavedLease 的 update lambda 第一次求值时另一线程改了
     * _sessionStateFlow.value，CAS 失败重试。旧实现 pendingRecord 保留第一次
     * 的旧 record，store.put 把 store 回退；新实现 mutateSession 在锁内一次性
     * 写 state+store，不分裂。
     */
    @Test
    fun commitSavedLease_casRetryDuringTransform_keepsStateAndStoreConsistent() {
        val coordinator = CasRetryHookCoordinator(baseBridge())
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession("t1", 5UL, "text", 10L, localDirty = true)
        coordinator.activateTarget("t1", 5UL, 10L)

        val lease = coordinator.issueDocumentOperationLease("t1")
        assertNotNull("必须能签发 lease", lease)
        assertEquals(10L, lease!!.rustRevision)
        assertTrue("初始 localDirty 必须为 true", lease.localDirty)

        val savedVersion = DocumentVersion(contentHash = "hash-saved")
        coordinator.commitSavedLease(lease, savedVersion)

        val state = coordinator.sessionState
        val record = coordinator.store.record("t1")
        assertNotNull("store 记录必须存在", record)
        record!!
        assertEquals(
            "state.localDirty 必须与 store.localDirty 一致（不得分裂）",
            state.localDirty,
            record.documentState.localDirty,
        )
        assertEquals(
            "state.revision 必须与 store.revision 一致（不得分裂）",
            state.revision,
            record.documentState.revision,
        )
        assertEquals(
            "state.committedVersion 必须与 store.committedVersion 一致（不得分裂）",
            state.committedVersion,
            record.documentState.committedVersion,
        )
    }

    /**
     * 复现：markSaved 的 update lambda + store.put 在 CAS 重试时分裂。
     */
    @Test
    fun markSaved_casRetryDuringTransform_keepsStateAndStoreConsistent() {
        val coordinator = CasRetryHookCoordinator(baseBridge())
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession("t1", 5UL, "text", 10L, localDirty = true)
        coordinator.activateTarget("t1", 5UL, 10L)

        coordinator.markSaved("t1", DocumentVersion(contentHash = "hash-saved"))

        val state = coordinator.sessionState
        val record = coordinator.store.record("t1")!!
        assertEquals(
            "markSaved 后 state.localDirty 必须与 store 一致",
            state.localDirty,
            record.documentState.localDirty,
        )
        assertEquals(
            "markSaved 后 state.committedVersion 必须与 store 一致",
            state.committedVersion,
            record.documentState.committedVersion,
        )
    }

    /**
     * 复现：applyExternalContentFact 的 update lambda + store.put 在 CAS 重试时分裂。
     */
    @Test
    fun applyExternalContentFact_casRetry_keepsStateAndStoreConsistent() {
        val coordinator = CasRetryHookCoordinator(baseBridge())
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession("t1", 5UL, "text", 10L, localDirty = false)
        coordinator.activateTarget("t1", 5UL, 10L)

        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                targetId = "t1",
                text = "newText",
                sourceVersion = DocumentVersion(contentHash = "hash-new"),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )

        val state = coordinator.sessionState
        val record = coordinator.store.record("t1")!!
        assertEquals(
            "applyExternalContentFact 后 state.committedVersion 必须与 store 一致",
            state.committedVersion,
            record.documentState.committedVersion,
        )
        assertEquals(
            "applyExternalContentFact 后 state.localDirty 必须与 store 一致",
            state.localDirty,
            record.documentState.localDirty,
        )
    }
}

/**
 * 测试用 Coordinator：override updateSessionState 在 transform 第一次求值后
 * 直接改 _sessionStateFlow.value，确定性触发 MutableStateFlow.update 的 CAS 重试。
 *
 * 同时提供 installExistingPersistentSession / activateTarget 便利方法设置初始状态。
 */
private class CasRetryHookCoordinator(
    bridge: com.xiwei.sujian.core.interop.app.AppServiceBridge,
) : EditorSessionCoordinator(bridge) {
    private val snapshots = mutableMapOf<ULong, TargetSnapshot>()
    private val validSessions = mutableSetOf<ULong>()

    fun installExistingPersistentSession(
        targetId: String,
        sessionId: ULong,
        text: String,
        revision: Long,
        localDirty: Boolean = false,
    ) {
        validSessions.add(sessionId)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        snapshots[sessionId] = TargetSnapshot(text, cursor, revision, 0, cursor)
        store.put(
            EditorSessionRecord(
                targetId = targetId,
                sessionId = sessionId,
                persistent = true,
                documentState =
                    DocumentState(
                        revision = revision,
                        selectionAnchorUtf8 = 0,
                        selectionHeadUtf8 = cursor,
                        localDirty = localDirty,
                    ),
            ),
        )
    }

    fun activateTarget(
        targetId: String,
        sessionId: ULong,
        revision: Long,
    ) {
        val record = store.record(targetId)!!
        _sessionStateFlow.value =
            EditorSessionState(
                targetId = targetId,
                sessionId = sessionId,
                revision = revision,
                activeTargetId = targetId,
                localDirty = record.documentState.localDirty,
                committedVersion = record.documentState.committedVersion,
                sessionBaseVersion = record.documentState.sessionBaseVersion,
                bindingState = WindowBindingState.Attached("w1", targetId, sessionId),
                editingState = com.xiwei.sujian.feature.editor.window.EditingState.EDITING,
            )
    }

    override fun updateSessionState(transform: (EditorSessionState) -> EditorSessionState) {
        var firstCall = true
        super.updateSessionState { state ->
            val result = transform(state)
            if (firstCall) {
                firstCall = false
                // 模拟并发：transform 第一次求值返回后、CAS 前，另一线程改了 value，
                // 迫使 MutableStateFlow.update CAS 失败并重试 transform。
                // 旧实现的 pendingRecord/committed var 保留第一次的旧值，store.put 分裂。
                _sessionStateFlow.value =
                    state.copy(
                        revision = state.revision + 1,
                        localDirty = true,
                    )
            }
            result
        }
    }

    internal override fun createSession(
        targetId: String,
        text: String,
        cursorByteOffset: Int,
        isPersistent: Boolean,
    ): ULong? = null

    internal override fun closeSession(sessionId: ULong) { }

    internal override fun validateSession(sessionId: ULong): Boolean = sessionId != 0UL && sessionId in validSessions

    internal override fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? = snapshots[sessionId]
}
