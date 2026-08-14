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

// ── #624 评论10 第2项：按 target/session 取得 lease ──
// 活动窗口、Detached 持久 session、切章中的旧 target 走同一入口；
// snapshot 缺失/错版返回 null，不伪造空正文。

/** 可控 snapshot 注入 fake — querySnapshotForSession 由测试安装。 */
private class FakeSnapshotCoordinator(bridge: com.xiwei.sujian.core.interop.app.AppServiceBridge) :
    EditorSessionCoordinator(bridge) {
    private val snapshots = mutableMapOf<ULong, TargetSnapshot>()

    fun installSnapshot(
        sessionId: ULong,
        text: String,
        revision: Long,
    ) {
        val cursor = text.toByteArray(Charsets.UTF_8).size
        snapshots[sessionId] = TargetSnapshot(text, cursor, revision, 0, cursor)
    }

    internal override fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? = snapshots[sessionId]
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DocumentOperationLeaseByTargetTest {
    private fun createCoordinator(): FakeSnapshotCoordinator {
        return FakeSnapshotCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_624_lease_by_target",
                    "/tmp/sujian_test_workspace_624_lease_by_target",
                ),
            ),
        )
    }

    private fun commitWithSession(
        coordinator: FakeSnapshotCoordinator,
        targetId: String,
        text: String,
        sessionId: ULong,
        revision: Long,
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

    /** 离开正文后 session 进入 Detached：activeTargetId=null，无活动 lease。 */
    private fun detachWindow(
        coordinator: FakeSnapshotCoordinator,
        targetId: String,
    ) {
        // commitPreparedSession 产生 Attaching(windowId="prepared") — 用同一 windowId 解绑。
        coordinator.detachWindowBinding("prepared", targetId)
    }

    /**
     * Detached 持久 session 必须仍能按 target 取得 lease（text 从真实 snapshot 取，
     * revision 与 store 记录一致）。无活动 target 时无参 issueDocumentOperationLease
     * 必须仍返回 null — 二者行为分开。
     */
    @Test
    fun issueLease_byTarget_worksForDetachedPersistentSession() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "旧正文", sessionId = 1UL, revision = 3L))
        coordinator.installSnapshot(1UL, "用户刚输入的真实正文", revision = 3L)

        detachWindow(coordinator, "a")
        assertNull(
            "Detached 后无活动 target — 无参 lease 必须为 null",
            coordinator.issueDocumentOperationLease(),
        )

        val lease = coordinator.issueDocumentOperationLease("a")
        assertNotNull("Detached 持久 session 必须能按 target 取得 lease", lease)
        val l = lease!!
        assertEquals("a", l.targetId)
        assertEquals("lease.text 必须来自真实 snapshot，不是空字符串", "用户刚输入的真实正文", l.text)
        assertEquals("lease.rustRevision 必须来自 snapshot 且与记录一致", 3L, l.rustRevision)
    }

    /**
     * #624 评论10 第1项：活动路径（无 targetId）同样必须校验
     * snapshot.revision == sessionState.revision。错版 snapshot（内核已前进、
     * 活动状态未跟上）返回 null，不伪造空正文 lease。
     */
    @Test
    fun issueLease_activePath_nullWhenSnapshotRevisionMismatch() {
        val coordinator = createCoordinator()
        // 提交时 sessionState.revision 与 store 记录均为 3。
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL, revision = 3L))
        // 注入错版 snapshot（revision=4）→ 活动路径校验失败。
        coordinator.installSnapshot(1UL, "newer", revision = 4L)
        assertNull(
            "活动路径错版 snapshot 时 lease 必须为 null，不伪造空正文",
            coordinator.issueDocumentOperationLease(),
        )
    }

    /**
     * #624 评论10 第1项：活动路径正例 — revision 匹配时 lease 携带真实 snapshot
     * 正文（用户输入后的真值），不是空字符串。
     */
    @Test
    fun issueLease_activePath_returnsLeaseWithSnapshotText() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "旧正文", sessionId = 1UL, revision = 3L))
        // 用户输入后 snapshot 已前进到 revision 3，正文是真实输入。
        coordinator.installSnapshot(1UL, "用户刚输入的真实正文", revision = 3L)

        val lease = coordinator.issueDocumentOperationLease()
        assertNotNull("活动路径 revision 匹配时必须签发 lease", lease)
        assertEquals("lease.text 必须来自真实 snapshot", "用户刚输入的真实正文", lease!!.text)
        assertEquals("lease.rustRevision 必须与 snapshot 一致", 3L, lease.rustRevision)
    }

    /** 按 target 取得 lease 时，snapshot 缺失必须返回 null（不伪造空正文）。 */
    @Test
    fun issueLease_byTarget_nullWhenSnapshotMissing() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL, revision = 1L))
        detachWindow(coordinator, "a")
        // 未安装 snapshot → querySnapshotForSession 返回 null。
        assertNull("snapshot 缺失时按 target 取得 lease 必须为 null", coordinator.issueDocumentOperationLease("a"))
    }

    /** 按 target 取得 lease 时，snapshot revision 与 store 记录不一致必须返回 null。 */
    @Test
    fun issueLease_byTarget_nullWhenRevisionMismatch() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "text", sessionId = 1UL, revision = 3L))
        detachWindow(coordinator, "a")
        // 记录 revision=3，但 snapshot 返回 revision=4（内核已前进，记录未跟上）→ 错版。
        coordinator.installSnapshot(1UL, "newer", revision = 4L)
        assertNull(
            "snapshot revision 与记录不一致时按 target 取得 lease 必须为 null",
            coordinator.issueDocumentOperationLease("a"),
        )
    }

    /** 无 session 记录（仅注册元数据）时按 target 取得 lease 必须为 null。 */
    @Test
    fun issueLease_byTarget_nullWithoutSession() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        assertNull(coordinator.issueDocumentOperationLease("a"))
    }

    /**
     * #624 评论12 第2项：lease.localDirty 唯一真值来自对应 target 的 store 记录 —
     * 未编辑为 false；applyLocalEdit(contentChanged=true) 后为 true。
     * 保存入口只消费 lease 的 localDirty，不再读 ViewModel 第二份 contentDirty。
     */
    @Test
    fun issueLease_carriesLocalDirtyFromStoreRecord() {
        val coordinator = createCoordinator()
        assertTrue(commitWithSession(coordinator, "a", "正文", sessionId = 1UL, revision = 3L))
        coordinator.installSnapshot(1UL, "正文", revision = 3L)

        val cleanLease = coordinator.issueDocumentOperationLease()
        assertNotNull(cleanLease)
        assertFalse("未编辑时 lease.localDirty 必须为 false", cleanLease!!.localDirty)

        // 真实输入路径：本地编辑事件置 store localDirty（revision 不变，保持 snapshot 一致）。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                revision = 3L,
                transactionId = 1L,
                lease = coordinator.currentInputLease()!!,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 1),
            ),
        )
        val dirtyLease = coordinator.issueDocumentOperationLease()
        assertNotNull(dirtyLease)
        assertTrue("输入后 lease.localDirty 必须为 true", dirtyLease!!.localDirty)
    }
}
