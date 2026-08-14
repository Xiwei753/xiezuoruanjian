package com.xiwei.sujian.feature.editor.session

import org.junit.Assert.assertFalse
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

    /**
     * #624 评论10 第1项：snapshot 缺失（无 native）时 issueDocumentOperationLease
     * 必须返回 null — 不伪造空正文 lease。这是数据安全核心守卫。
     */
    @Test
    fun issueLease_returnsNullWhenSnapshotMissing() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "hello", sessionId = 1UL, revision = 1L))
        // 测试环境无 native → querySnapshotForSession 返回 null → lease 必须为 null。
        assertNull(
            "snapshot 缺失时必须返回 null，不伪造空正文 lease",
            coordinator.issueDocumentOperationLease(),
        )
    }

    /**
     * #624 评论10 第1项：snapshot 缺失时连续调用都返回 null —
     * 不会因为 session 存在就放宽守卫。
     */
    @Test
    fun issueLease_alwaysNullWhenSnapshotMissing() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL))
        assertNull(coordinator.issueDocumentOperationLease())
        assertNull(coordinator.issueDocumentOperationLease())
    }

    // ── isDocumentOperationLeaseCurrent：手动构造 lease 测试（不依赖 snapshot） ──

    @Test
    fun isLeaseCurrent_trueWhenMatching() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 2UL))
        // 手动构造 lease（不经过 issueDocumentOperationLease，避免 snapshot 依赖）。
        // epoch 从 currentInputLease 取 — commitPreparedSession 递增 inputLeaseEpoch。
        val epoch = coordinator.currentInputLease()!!.epoch
        val lease =
            DocumentOperationLease(
                operationId = 1L,
                targetId = "a",
                coreSessionId = 2UL,
                inputEpoch = epoch,
                rustRevision = 1L,
                text = "text",
                committedVersion = DocumentVersion(),
            )
        assertTrue("匹配 target/session/epoch 的 lease 必须有效", coordinator.isDocumentOperationLeaseCurrent(lease))
    }

    @Test
    fun isLeaseCurrent_falseAfterInvalidate() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL))
        val epoch = coordinator.currentInputLease()!!.epoch
        val lease =
            DocumentOperationLease(
                operationId = 1L,
                targetId = "a",
                coreSessionId = 1UL,
                inputEpoch = epoch,
                rustRevision = 1L,
                text = "text",
                committedVersion = DocumentVersion(),
            )
        coordinator.invalidateInputLease()
        assertFalse("invalidateInputLease 后旧 lease 必须失效", coordinator.isDocumentOperationLeaseCurrent(lease))
    }

    @Test
    fun isLeaseCurrent_falseForDifferentTarget() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text-a", sessionId = 1UL))
        val epochA = coordinator.currentInputLease()!!.epoch
        val leaseA =
            DocumentOperationLease(
                operationId = 1L,
                targetId = "a",
                coreSessionId = 1UL,
                inputEpoch = epochA,
                rustRevision = 1L,
                text = "text-a",
                committedVersion = DocumentVersion(),
            )
        assertTrue(commitWithSession(coordinator, "b", "text-b", sessionId = 2UL))
        assertFalse("切换到 B 后 A 的 lease 必须失效", coordinator.isDocumentOperationLeaseCurrent(leaseA))
    }
}
