package com.xiwei.sujian.editor.v2.coordinator

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
 * #595 二：DocumentOperationLease 契约测试 — 保存/同步开始时签发的完整文档快照。
 *
 * lease 包含 target/session/epoch/revision/text/committedVersion 全部字段，
 * 调用方不再自行拼接 currentSession + sessionState。任一字段不匹配则操作中止。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DocumentOperationLeaseContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_doc_lease")
        ))
    }

    private fun commit(coordinator: EditorSessionCoordinator, targetId: String, text: String, sessionId: ULong = 0UL): Boolean {
        coordinator.registerTargetMeta(targetId, TextEditorProfile.DocumentBody, persistent = true)
        val handle = PreparedSessionHandle(
            targetId = targetId,
            sessionId = sessionId,
            snapshot = TargetSnapshot(text, text.toByteArray(Charsets.UTF_8).size, 1L, 0, text.toByteArray(Charsets.UTF_8).size),
            newlyCreated = true,
            previousRecord = null,
        )
        return coordinator.commitPreparedSession(handle, "window-test")
    }

    @Test
    fun issueLease_nullWithoutActiveTarget() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        assertNull("未绑定/无活动目标时必须返回 null", coordinator.issueDocumentOperationLease())
    }

    @Test
    fun issueLease_returnsCompleteSnapshotAfterCommit() {
        val coordinator = createCoordinator()
        assertTrue(commit(coordinator, "a", "hello"))
        val lease = coordinator.issueDocumentOperationLease()
        assertNotNull("活动 target 必须签发 lease", lease)
        assertEquals("a", lease!!.targetId)
        assertEquals("hello", lease.text)
    }

    @Test
    fun isLeaseCurrent_trueWhenMatching() {
        val coordinator = createCoordinator()
        assertTrue(commit(coordinator, "a", "text"))
        val lease = coordinator.issueDocumentOperationLease()
        assertNotNull(lease)
        assertTrue("刚签发的 lease 必须有效", coordinator.isDocumentOperationLeaseCurrent(lease!!))
    }

    @Test
    fun isLeaseCurrent_falseAfterInvalidate() {
        val coordinator = createCoordinator()
        assertTrue(commit(coordinator, "a", "text"))
        val lease = coordinator.issueDocumentOperationLease()
        assertNotNull(lease)
        coordinator.invalidateInputLease()
        assertFalse("invalidateInputLease 后旧 lease 必须失效", coordinator.isDocumentOperationLeaseCurrent(lease!!))
    }

    @Test
    fun isLeaseCurrent_falseForDifferentTarget() {
        val coordinator = createCoordinator()
        assertTrue(commit(coordinator, "a", "text-a"))
        val leaseA = coordinator.issueDocumentOperationLease()
        assertNotNull(leaseA)
        assertTrue(commit(coordinator, "b", "text-b"))
        assertFalse("切换到 B 后 A 的 lease 必须失效", coordinator.isDocumentOperationLeaseCurrent(leaseA!!))
    }

    @Test
    fun leaseOperationIdMonotonic() {
        val coordinator = createCoordinator()
        assertTrue(commit(coordinator, "a", "text"))
        val lease1 = coordinator.issueDocumentOperationLease()
        assertNotNull(lease1)
        val lease2 = coordinator.issueDocumentOperationLease()
        assertNotNull(lease2)
        assertTrue("每次签发 operationId 必须递增", lease2!!.operationId > lease1!!.operationId)
    }
}
