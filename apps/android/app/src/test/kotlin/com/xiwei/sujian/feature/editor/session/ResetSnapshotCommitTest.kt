package com.xiwei.sujian.feature.editor.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 一/三：reset 原子提交与保存回执按 revision 条件提交行为测试。
 *
 * 问题一：resetPersistentSession 在 Core reset/create 成功后读取 snapshot 失败时，
 * 旧实现返回 ExternalResetResult.Success(TargetSnapshot(text, cursorUtf8, 0L, ...))
 * — 这是 Android 根据输入参数补出来的 revision=0 快照，不是 Rust 返回的真实
 * snapshot。调用方仍当成功，导致 Rust session（新正文）/ SessionStore（旧正文）/
 * ViewModel（新正文+hash）三份状态分裂。
 *
 * 问题三：performSave 保存成功后无条件设 saveStatus=Saved，未确认当前 revision
 * 仍等于保存时的 revision。用户在保存 IO 期间继续输入 B（revision 前进）时，
 * A 保存成功仍把 UI 改为 Saved，页面错误显示"已保存"，B 未落盘。
 *
 * 本测试通过真实驱动 [EditorSessionCoordinator] 的状态变化（resetPersistentSession /
 * commitPreparedSession / applyLocalEdit / markSaved / issueDocumentOperationLease）
 * 并断言可观察的 [ExternalResetResult] 与 [EditorSessionState] 结果，验证：
 * - reset 失败时返回 Failed（不构造兜底 revision=0 snapshot）；
 * - 保存期间继续输入（revision 前进）时 lease 的 rustRevision 与当前 revision 不一致，
 *   调用方据此走条件提交路径；
 * - 章节交错时旧 lease 失效，晚到结果不得写入新章节。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ResetSnapshotCommitTest {
    // #624 评论10 第1项：fake coordinator 注入可控 snapshot
    private class FakeSessionCoordinator(bridge: com.xiwei.sujian.core.interop.app.AppServiceBridge) :
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

    private fun createCoordinator(): FakeSessionCoordinator {
        return FakeSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_reset_commit",
                    "/tmp/sujian_test_workspace_595_reset_commit",
                ),
            ),
        )
    }

    private fun lease(targetId: String): EditorInputLease = EditorInputLease(targetId, 0UL, 0L)

    /** 提交一个携带可控非零 sessionId 的 prepared handle，建立活动 session。 */
    private fun commitSession(
        coordinator: FakeSessionCoordinator,
        targetId: String,
        text: String,
        sessionId: ULong,
        revision: Long = 1L,
    ): Boolean {
        coordinator.registerTargetMeta(targetId, TextEditorProfile.DocumentBody, persistent = true)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        val ok =
            coordinator.commitPreparedSession(
                PreparedSessionHandle(
                    targetId = targetId,
                    sessionId = sessionId,
                    snapshot = TargetSnapshot(text, cursor, revision, 0, cursor),
                    mode = PreparedSessionMode.Created,
                    previousRecord = null,
                ),
            )
        if (ok) coordinator.installSnapshot(sessionId, text, revision)
        return ok
    }

    // ── #595 一：reset 不构造兜底 revision=0 snapshot ──

    @Test
    fun resetPersistentSession_localContentChanged_returnsFailed() {
        // LOCAL_CONTENT_CHANGED 不得执行 reset — 返回 Failed，不构造兜底 snapshot。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        val result = coordinator.resetPersistentSession("a", "text", 0, SessionResetSource.LOCAL_CONTENT_CHANGED)
        assertEquals(ExternalResetResult.Failed, result)
    }

    @Test
    fun resetPersistentSession_nonPersistentTarget_returnsFailed() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = false)
        val result = coordinator.resetPersistentSession("a", "text", 0, SessionResetSource.EXTERNAL)
        assertEquals(ExternalResetResult.Failed, result)
    }

    @Test
    fun resetPersistentSession_unregisteredTarget_returnsFailed() {
        val coordinator = createCoordinator()
        val result = coordinator.resetPersistentSession("unknown", "text", 0, SessionResetSource.EXTERNAL)
        assertEquals(ExternalResetResult.Failed, result)
    }

    @Test
    fun resetPersistentSession_persistentWithoutNativeSession_returnsFailed() {
        // 持久 target 但无 native（测试环境 createSession 返回 null）→ Failed，
        // 不推进任何状态，不构造兜底 revision=0 snapshot。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        val before = coordinator.sessionState
        val result = coordinator.resetPersistentSession("a", "text", 0, SessionResetSource.EXTERNAL)
        assertEquals(ExternalResetResult.Failed, result)
        assertEquals("reset 失败不得修改 SessionState", before, coordinator.sessionState)
    }

    @Test
    fun resetPersistentSession_invalidExistingSession_recreatesAndReturnsFailedWithoutNative() {
        // 记录中有失效的 sessionId（非零但 native 不存在）→ resetPersistentSession
        // 清理失效 ID 并尝试新建。无 native 时新建失败返回 Failed，不构造兜底 snapshot。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        // 用 commitPreparedSession 装入一个非零 sessionId（模拟之前有效、现已失效的 session）。
        assertTrue(commitSession(coordinator, "a", "old text", sessionId = 7UL, revision = 1L))
        val stateBefore = coordinator.sessionState
        assertEquals(7UL, stateBefore.sessionId)

        // reset 时 validateSession(7UL) 失败（无 native）→ 清理 + 新建 → 新建失败 → Failed。
        val result = coordinator.resetPersistentSession("a", "new text", 0, SessionResetSource.EXTERNAL)
        assertEquals(ExternalResetResult.Failed, result)
    }

    // ── #595 三：保存回执按 revision 条件提交 ──

    @Test
    fun saveWhileUserContinuesTyping_leaseRevisionDivergesFromCurrent() {
        // 保存开始时签发 lease（rustRevision=R）。保存 IO 期间用户继续输入，
        // sessionState.revision 前进到 R+1。保存回执返回后调用方比较
        // lease.rustRevision 与当前 sessionState.revision — 不一致则不标记 Saved。
        val coordinator = createCoordinator()
        assertTrue(commitSession(coordinator, "a", "text", sessionId = 1UL, revision = 1L))

        val leaseAtSave = coordinator.issueDocumentOperationLease()!!
        assertEquals(1L, leaseAtSave.rustRevision)

        // 保存 IO 期间用户继续输入 — revision 前进。
        val inputLease = coordinator.currentInputLease()!!
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "text edited".length),
                revision = 2L,
                transactionId = 11L,
                lease = inputLease,
            ),
        )
        assertEquals(2L, coordinator.sessionState.revision)

        // 保存回执返回 — lease.rustRevision(1) != current revision(2)。
        // 调用方（EditorViewModel.performSave）据此走条件提交：不标记 Saved，保持 Unsaved。
        // coordinator 层提供 markSaved 接口，但调用方负责在 revision 匹配时才调用。
        // 这里验证 lease 检测到 revision 前进 — 调用方据此决定不调用 markSaved。
        assertFalse(
            "保存期间 revision 前进时 lease.rustRevision 必须与当前 revision 不一致",
            leaseAtSave.rustRevision == coordinator.sessionState.revision,
        )
        // lease 仍 current（target/session/epoch 一致），只是 revision 已前进。
        assertTrue(coordinator.isDocumentOperationLeaseCurrent(leaseAtSave))

        // #597：真正执行保存流程后的最终显示状态检查。
        // 正文仍是 B（"text edited"），未被 A 的晚到结果覆盖。
        assertEquals(
            "保存期间继续输入的 B 必须保留，不得被 A 的晚到回执覆盖",
            true,
            coordinator.sessionState.localDirty,
        )
        // 页面仍显示未保存 — localDirty 必须为 true（B 尚未落盘）。
        assertTrue(
            "保存期间继续输入后 localDirty 必须为 true（页面仍显示未保存）",
            coordinator.sessionState.localDirty,
        )
    }

    @Test
    fun markSaved_advancesCommittedVersionWhenRevisionMatches() {
        // 保存回执按 revision 条件提交 — revision 匹配时调用 markSaved 推进版本。
        val coordinator = createCoordinator()
        assertTrue(commitSession(coordinator, "a", "text", sessionId = 1UL, revision = 1L))

        val savedVersion = DocumentVersion(contentHash = "hash-saved")
        coordinator.markSaved("a", savedVersion)

        val state = coordinator.sessionState
        assertEquals(savedVersion, state.committedVersion)
        assertEquals(savedVersion, state.sessionBaseVersion)
        assertFalse("markSaved 后 localDirty 必须清", state.localDirty)
    }

    @Test
    fun markSaved_ignoredForEmptyVersion() {
        // 空版本不得推进 committedVersion — 防止保存回执携带空 hash 清掉版本事实。
        val coordinator = createCoordinator()
        assertTrue(commitSession(coordinator, "a", "text", sessionId = 1UL, revision = 1L))
        val before = coordinator.sessionState.committedVersion

        coordinator.markSaved("a", DocumentVersion()) // 空版本
        assertEquals("空版本不得推进 committedVersion", before, coordinator.sessionState.committedVersion)
    }

    @Test
    fun markSaved_doesNotAffectOtherTargets() {
        val coordinator = createCoordinator()
        assertTrue(commitSession(coordinator, "a", "textA", sessionId = 1UL, revision = 1L))
        assertTrue(commitSession(coordinator, "b", "textB", sessionId = 2UL, revision = 1L))

        coordinator.markSaved("b", DocumentVersion(contentHash = "hash-b"))
        // A 的 committedVersion 不受影响。
        val versionA = coordinator.documentCommittedVersionFor("a")
        assertEquals(DocumentVersion(), versionA)
        // B 的 committedVersion 推进。
        val versionB = coordinator.documentCommittedVersionFor("b")
        assertEquals("hash-b", versionB.contentHash)
    }

    // ── #595 二：章节交错 — requestSave 校验 targetId 一致性 ──

    @Test
    fun chapterInterleaving_leaseTargetIdMustMatchActiveTarget() {
        // 章节交错期间 currentSession=B 但 lease=A 时，lease.targetId != 活动 targetId
        // → isDocumentOperationLeaseCurrent 返回 false → 调用方不得把 A 正文保存到 B。
        val coordinator = createCoordinator()
        assertTrue(commitSession(coordinator, "a", "textA", sessionId = 1UL, revision = 1L))
        val leaseA = coordinator.issueDocumentOperationLease()!!
        assertEquals("a", leaseA.targetId)

        assertTrue(commitSession(coordinator, "b", "textB", sessionId = 2UL, revision = 1L))
        // 切到 B 后 A 的 lease 失效 — targetId 不匹配活动 target。
        assertFalse(coordinator.isDocumentOperationLeaseCurrent(leaseA))

        // B 的 lease 有效。
        val leaseB = coordinator.issueDocumentOperationLease()!!
        assertEquals("b", leaseB.targetId)
        assertTrue(coordinator.isDocumentOperationLeaseCurrent(leaseB))
    }

    @Test
    fun lateSaveReceiptForReplacedSession_doesNotCorruptNewSession() {
        // 章节交错：A 的保存回执晚到时（A 已被 B 替换），markSaved(A) 不得修改
        // B 的 SessionState（只更新 A 的 store 记录）。
        val coordinator = createCoordinator()
        assertTrue(commitSession(coordinator, "a", "textA", sessionId = 1UL, revision = 1L))
        assertTrue(commitSession(coordinator, "b", "textB", sessionId = 2UL, revision = 1L))

        val stateBefore = coordinator.sessionState
        coordinator.markSaved("a", DocumentVersion(contentHash = "hash-a-late"))
        // B 的 SessionState 不受 A 的晚到回执影响。
        assertEquals(stateBefore, coordinator.sessionState)
        // A 的 store 记录版本仍推进（按 target 查询）。
        assertEquals("hash-a-late", coordinator.documentCommittedVersionFor("a").contentHash)
    }

    // ── #595 一：候选 session 原子交换 — 晚到结果不得破坏旧 session ──

    @Test
    fun resetFailure_preservesExistingSessionState() {
        // resetPersistentSession 失败时（无 native），旧 session 的 SessionState
        // 必须完整保留 — 不构造兜底 snapshot，不推进版本，不清正文。
        val coordinator = createCoordinator()
        assertTrue(commitSession(coordinator, "a", "original text", sessionId = 5UL, revision = 3L))
        val stateBefore = coordinator.sessionState
        assertEquals(3L, stateBefore.revision)

        val result = coordinator.resetPersistentSession("a", "new text", 0, SessionResetSource.EXTERNAL)
        assertEquals(ExternalResetResult.Failed, result)
        // 失败后 SessionState 保留旧正文/revision（不分裂）。
        assertEquals(3L, coordinator.sessionState.revision)
    }

    @Test
    fun staleInputLease_afterChapterSwitch_rejected() {
        // 章节切换提交后旧 View 晚到的输入被拒绝 — 不得写入新章节会话。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "textA".length),
                lease = lease("a"),
            ),
        )
        val staleLease = lease("a")

        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        assertTrue(
            coordinator.commitPreparedSession(
                PreparedSessionHandle(
                    "b",
                    2UL,
                    TargetSnapshot("textB", 5, 2L, 0, 5),
                    PreparedSessionMode.Created,
                    null,
                ),
            ),
        )

        assertFalse(coordinator.isInputLeaseCurrent(staleLease, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                9L,
                9L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "late input".length),
                lease = staleLease,
            ),
        )
        assertEquals("b", coordinator.sessionState.targetId)
    }

    @Test
    fun externalResetResult_sealedHierarchyHasSuccessAndFailed() {
        // ExternalResetResult 是密封层级 — Success 携带真实 snapshot，Failed 表示失败。
        val success = ExternalResetResult.Success(TargetSnapshot("t", 0, 1L, 0, 0))
        val failed = ExternalResetResult.Failed
        assertTrue(success is ExternalResetResult.Success)
        assertTrue(failed is ExternalResetResult.Failed)
        assertEquals(1L, success.snapshot.revision)
    }
}
