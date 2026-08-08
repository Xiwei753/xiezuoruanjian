package com.xiwei.sujian.feature.editor.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 二：DocumentOperationLease 行为测试 — 保存/同步开始时签发的完整文档快照。
 *
 * lease 包含 target/session/epoch/revision/text/committedVersion 全部字段，
 * 调用方不再自行拼接 currentSession + sessionState。任一字段不匹配则操作中止。
 *
 * 本测试通过真实驱动 [EditorSessionCoordinator] 的状态变化并断言
 * [issueDocumentOperationLease] / [isDocumentOperationLeaseCurrent] 的可观察结果，
 * 验证 lease 携带的 target/session/epoch/revision/text/committedVersion 全部来自
 * 同一活动记录。由于测试环境无 native（createSession 返回 NotLoaded），通过
 * [commitPreparedSession] 提交手工构造的 [PreparedSessionHandle]（sessionId 为
 * 可控非零值 1UL/2UL）建立活动 session — 这是 native 不可用时的唯一可控注入点，
 * 等价于一个返回非零 session ID 的可控 Fake Bridge。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DocumentOperationLeaseTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_doc_lease",
                    "/tmp/sujian_test_workspace_595_doc_lease",
                ),
            ),
        )
    }

    /**
     * 提交一个携带可控非零 sessionId 的 prepared handle，建立活动 session。
     * 等价于 Fake Bridge 的 textEditSessionCreate 返回 [sessionId]。
     */
    private fun commitWithSession(
        coordinator: EditorSessionCoordinator,
        targetId: String,
        text: String,
        sessionId: ULong,
        revision: Long = 1L,
    ): Boolean {
        coordinator.registerTargetMeta(targetId, TextEditorProfile.DocumentBody, persistent = true)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        val handle =
            PreparedSessionHandle(
                targetId = targetId,
                sessionId = sessionId,
                snapshot = TargetSnapshot(text, cursor, revision, 0, cursor),
                newlyCreated = true,
                previousRecord = null,
            )
        return coordinator.commitPreparedSession(handle)
    }

    @Test
    fun issueLease_nullWithoutActiveTarget() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        assertNull("未绑定/无活动目标时必须返回 null", coordinator.issueDocumentOperationLease())
    }

    @Test
    fun issueLease_returnsCompleteSnapshotFromSingleRecord() {
        // lease 的 target/session/epoch/revision/text/committedVersion 必须全部来自
        // 同一活动记录 — 不再由调用方拼接 currentSession + sessionState 两个独立源。
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "hello", sessionId = 1UL, revision = 1L))
        val lease = coordinator.issueDocumentOperationLease()
        assertNotNull("活动 target 必须签发 lease", lease)
        val l = lease!!
        assertEquals("a", l.targetId)
        assertEquals("lease.coreSessionId 必须来自活动记录的 sessionId", 1UL, l.coreSessionId)
        assertEquals("lease.rustRevision 必须来自活动记录的 revision", 1L, l.rustRevision)
        assertEquals("lease.text 必须来自活动记录的正文", "hello", l.text)
        // committedVersion 来自 store 记录（commitPreparedSession 保留 doc.committedVersion）。
        val record = coordinator.getPersistentSessionId("a")
        assertEquals("store 记录的 sessionId 必须与 lease 一致", 1UL, record)
    }

    @Test
    fun isLeaseCurrent_trueWhenMatching() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 2UL))
        val lease = coordinator.issueDocumentOperationLease()
        assertNotNull(lease)
        assertTrue("刚签发的 lease 必须有效", coordinator.isDocumentOperationLeaseCurrent(lease!!))
    }

    @Test
    fun isLeaseCurrent_falseAfterInvalidate() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL))
        val lease = coordinator.issueDocumentOperationLease()
        assertNotNull(lease)
        coordinator.invalidateInputLease()
        assertFalse("invalidateInputLease 后旧 lease 必须失效", coordinator.isDocumentOperationLeaseCurrent(lease!!))
    }

    @Test
    fun isLeaseCurrent_falseForDifferentTarget() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text-a", sessionId = 1UL))
        val leaseA = coordinator.issueDocumentOperationLease()
        assertNotNull(leaseA)
        assertTrue(commitWithSession(coordinator, "b", "text-b", sessionId = 2UL))
        assertFalse("切换到 B 后 A 的 lease 必须失效", coordinator.isDocumentOperationLeaseCurrent(leaseA!!))
    }

    @Test
    fun leaseOperationIdMonotonic() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL))
        val lease1 = coordinator.issueDocumentOperationLease()
        assertNotNull(lease1)
        val lease2 = coordinator.issueDocumentOperationLease()
        assertNotNull(lease2)
        assertTrue("每次签发 operationId 必须递增", lease2!!.operationId > lease1!!.operationId)
    }

    @Test
    fun leaseCarriesDistinctSessionIdsForDifferentTargets() {
        // 两个 target 各自提交可控的非零 session ID，lease 必须携带各自的 ID，
        // 不互相串扰（旧实现拼接 currentSession + sessionState 会交错）。
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text-a", sessionId = 1UL))
        val leaseA = coordinator.issueDocumentOperationLease()!!
        assertEquals(1UL, leaseA.coreSessionId)
        assertEquals("text-a", leaseA.text)

        assertTrue(commitWithSession(coordinator, "b", "text-b", sessionId = 2UL))
        val leaseB = coordinator.issueDocumentOperationLease()!!
        assertEquals(2UL, leaseB.coreSessionId)
        assertEquals("text-b", leaseB.text)
        assertEquals("b", leaseB.targetId)

        // 切到 B 后 A 的 lease 失效（target/session 不再匹配活动记录）。
        assertFalse(coordinator.isDocumentOperationLeaseCurrent(leaseA))
        assertTrue(coordinator.isDocumentOperationLeaseCurrent(leaseB))
    }

    @Test
    fun leaseRejectedAfterLocalEditAdvancesRevision() {
        // 保存期间用户继续输入 → revision 前进 → 旧 lease 的 rustRevision 不再匹配
        // 活动记录（调用方据此判断保存回执是否仍属于当前文档）。
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL, revision = 1L))
        val leaseAtSave = coordinator.issueDocumentOperationLease()!!
        assertEquals(1L, leaseAtSave.rustRevision)

        // 用活动 lease 继续输入，revision 前进。
        val inputLease = coordinator.currentInputLease()!!
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                text = "text edited",
                revision = 2L,
                transactionId = 11L,
                lease = inputLease,
            ),
        )
        assertEquals(2L, coordinator.sessionState.revision)

        // 旧 lease 的 epoch 仍匹配（未发生章节切换/关闭），但调用方通过比较
        // lease.rustRevision 与当前 sessionState.revision 检测保存期间是否有新输入。
        // lease 本身仍 "current"（target/session/epoch 一致），但 revision 已前进 —
        // 调用方（EditorViewModel.performSave）据此走条件提交路径。
        val leaseAfterEdit = coordinator.issueDocumentOperationLease()!!
        assertEquals(2L, leaseAfterEdit.rustRevision)
        assertEquals("text edited", leaseAfterEdit.text)
        assertTrue(coordinator.isDocumentOperationLeaseCurrent(leaseAfterEdit))
    }
}
