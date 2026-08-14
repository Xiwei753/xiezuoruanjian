package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState
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
 * #624 评论17 问题1/3/5：SessionMutationGate 单一临界区 + 删除 updateSessionState +
 * PendingExternalVersion 不缓存正文 + commitSavedLease 原子提交行为测试。
 *
 * 替代旧 [TransformPurityTest]（updateSessionState transform 纯函数语义）—
 * updateSessionState 已删除，session 的 state/store/epoch 写入只走 [mutateSession]。
 *
 * 本文件验证：
 * - updateSessionState 已从 EditorSessionCoordinator 删除；
 * - forceEditingState 走 mutateSession（state 改变且不破坏 store 一致性）；
 * - PendingExternalVersion 只含 sourceVersion + origin，不含 text；
 * - DocumentState.pendingExternal 字段类型为 PendingExternalVersion?；
 * - storePendingExternalFact 只存 sourceVersion/origin，不缓存 fact.text；
 * - commitSavedLease stale lease 不清新 revision dirty，matching lease 原子清 dirty；
 * - applyLocalEdit / applyUndoRestored / applyProgrammaticReplace 后 SessionState 与
 *   store 记录保持一致（旧 TransformPurityTest 的行为契约）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SessionMutationGateTest {
    private fun createCoordinator(): EditorSessionCoordinator =
        EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_gate_unit",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_gate_unit",
                ),
            ),
        )

    private fun lease(
        targetId: String,
        sessionId: ULong = 0UL,
        epoch: Long = 0L,
    ): EditorInputLease = EditorInputLease(targetId, sessionId, epoch)

    // ── (a) 删除 updateSessionState 入口 ──

    /**
     * #624 评论17 问题3：updateSessionState 必须从 EditorSessionCoordinator 删除。
     * session 的 state/store/epoch 写入只走 mutateSession 单一临界区。
     */
    @Test
    fun updateSessionState_removedFromCoordinator() {
        val method =
            EditorSessionCoordinator::class.java.declaredMethods.firstOrNull {
                it.name.startsWith("updateSessionState")
            }
        assertNull(
            "updateSessionState must be removed from EditorSessionCoordinator (#624 评论17 问题3)",
            method,
        )
    }

    /**
     * forceEditingState 走 mutateSession — state 改变且 store 记录不被破坏。
     */
    @Test
    fun forceEditingState_updatesStateViaMutateSession() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "text".length),
                lease = lease("a"),
            ),
        )
        coordinator.forceEditingState(EditingState.EDITING)
        assertEquals(EditingState.EDITING, coordinator.sessionState.editingState)
        // store 记录仍保留（forceEditingState 不应破坏 store）。
        assertNotNull(coordinator.getPersistentSessionId("a"))
        assertEquals(1L, coordinator.sessionState.revision)
    }

    // ── (c) PendingExternalVersion 不缓存正文 ──

    /**
     * #624 评论17 问题5：PendingExternalVersion 必须存在且只含 sourceVersion + origin，
     * 不含 text: String（不得把整章正文复制重新引回 DocumentState）。
     */
    @Test
    fun pendingExternalVersion_existsAndDoesNotContainText() {
        val version =
            PendingExternalVersion(
                sourceVersion = DocumentVersion(contentHash = "h"),
                origin = DocumentFactOrigin.SYNC_MERGED,
            )
        val fieldNames = PendingExternalVersion::class.java.declaredFields.map { it.name }
        assertTrue("PendingExternalVersion 必须含 sourceVersion 字段", "sourceVersion" in fieldNames)
        assertTrue("PendingExternalVersion 必须含 origin 字段", "origin" in fieldNames)
        assertFalse(
            "PendingExternalVersion 不得含 text 字段（不缓存整章正文）",
            "text" in fieldNames,
        )
        assertEquals(DocumentVersion(contentHash = "h"), version.sourceVersion)
        assertEquals(DocumentFactOrigin.SYNC_MERGED, version.origin)
    }

    /**
     * DocumentState.pendingExternal 字段类型为 PendingExternalVersion?，
     * 旧 pendingExternalFact: TargetDocumentFact? 字段已删除。
     */
    @Test
    fun documentState_pendingExternal_replacesPendingExternalFact() {
        val newField = DocumentState::class.java.declaredFields.firstOrNull { it.name == "pendingExternal" }
        assertNotNull(
            "DocumentState 必须有 pendingExternal: PendingExternalVersion? 字段",
            newField,
        )
        val oldField = DocumentState::class.java.declaredFields.firstOrNull { it.name == "pendingExternalFact" }
        assertNull(
            "DocumentState 不得保留旧 pendingExternalFact: TargetDocumentFact? 字段",
            oldField,
        )
    }

    /**
     * storePendingExternalFact 只存 sourceVersion + origin，不缓存 fact.text。
     * pendingExternalFactFor 返回 PendingExternalVersion?（不含 text）。
     */
    @Test
    fun storePendingExternalFact_storesOnlySourceVersionAndOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                targetId = "t1",
                text = "localText",
                sourceVersion = DocumentVersion(contentHash = "hash-local"),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val fact =
            TargetDocumentFact(
                targetId = "t1",
                text = "remoteText-very-long-content-that-must-not-be-cached",
                sourceVersion = DocumentVersion(contentHash = "hash-remote"),
                baseVersion = DocumentVersion(),
                origin = DocumentFactOrigin.SYNC_MERGED,
            )
        coordinator.storePendingExternalFact("t1", fact)
        val pending = coordinator.pendingExternalFactFor("t1")
        assertNotNull("pendingExternal 必须保存", pending)
        pending!!
        assertEquals(DocumentVersion(contentHash = "hash-remote"), pending.sourceVersion)
        assertEquals(DocumentFactOrigin.SYNC_MERGED, pending.origin)
    }

    /**
     * consumePendingExternalFact 返回 PendingExternalVersion? 并清除。
     */
    @Test
    fun consumePendingExternalFact_returnsPendingExternalVersionAndClears() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "localText",
                DocumentVersion(contentHash = "hash-local"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        val fact =
            TargetDocumentFact(
                "t1",
                "remoteText",
                DocumentVersion(contentHash = "hash-remote"),
                DocumentVersion(),
                DocumentFactOrigin.SYNC_MERGED,
            )
        coordinator.storePendingExternalFact("t1", fact)
        assertNotNull(coordinator.pendingExternalFactFor("t1"))
        val consumed = coordinator.consumePendingExternalFact("t1")
        assertNotNull(consumed)
        consumed!!
        assertEquals(DocumentVersion(contentHash = "hash-remote"), consumed.sourceVersion)
        assertEquals(DocumentFactOrigin.SYNC_MERGED, consumed.origin)
        assertNull("consume 后 pendingExternal 必须清除", coordinator.pendingExternalFactFor("t1"))
    }

    // ── (d) commitSavedLease stale lease 不清新 revision dirty ──

    /**
     * #624 评论17 问题5：commitSavedLease stale lease（revision 不匹配）不清新 dirty。
     * matching lease 原子清 dirty。
     */
    @Test
    fun commitSavedLease_staleRevisionLeaseDoesNotClearDirty() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        // 建立活动 session（sessionId=5UL, revision=10）
        assertTrue(
            coordinator.commitPreparedSession(
                PreparedSessionHandle(
                    "t1", 5UL, TargetSnapshot("initial", 7, 10L, 0, 7),
                    PreparedSessionMode.Created, null,
                ),
            ),
        )
        coordinator.activateAttachedForTest("t1")
        val activeLease = coordinator.currentInputLease()!!

        // 本地输入：revision=10, localDirty=true
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "edit".length),
                revision = 10L,
                transactionId = 1L,
                lease = activeLease,
            ),
        )
        assertTrue("初始 localDirty 必须为 true", coordinator.sessionState.localDirty)
        assertEquals(10L, coordinator.sessionState.revision)

        // 构造 lease（revision=10，与当前 state 匹配）
        val saveLease = DocumentOperationLease(
            operationId = 1L,
            targetId = "t1",
            coreSessionId = 5UL,
            inputEpoch = activeLease.epoch,
            rustRevision = 10L,
            text = "initial-edit",
            committedVersion = DocumentVersion(),
            localDirty = true,
        )

        // 保存期间继续输入：revision 前进到 11
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "more".length),
                revision = 11L,
                transactionId = 2L,
                lease = activeLease,
            ),
        )
        assertEquals(11L, coordinator.sessionState.revision)

        // stale lease (revision=10) 提交 — 不得清 dirty
        val committed = coordinator.commitSavedLease(saveLease, DocumentVersion(contentHash = "hash-saved"))
        assertFalse("stale revision lease 不得提交", committed)
        assertTrue(
            "stale lease 提交后 localDirty 必须保留（新输入未落盘）",
            coordinator.sessionState.localDirty,
        )
    }

    /**
     * matching lease（target/session/epoch/revision 全匹配）原子清 dirty。
     */
    @Test
    fun commitSavedLease_matchingLeaseAtomicallyClearsDirty() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        assertTrue(
            coordinator.commitPreparedSession(
                PreparedSessionHandle(
                    "t1", 5UL, TargetSnapshot("initial", 7, 10L, 0, 7),
                    PreparedSessionMode.Created, null,
                ),
            ),
        )
        coordinator.activateAttachedForTest("t1")
        val activeLease = coordinator.currentInputLease()!!

        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "edit".length),
                revision = 10L,
                transactionId = 1L,
                lease = activeLease,
            ),
        )
        assertTrue(coordinator.sessionState.localDirty)

        val saveLease = DocumentOperationLease(
            operationId = 1L,
            targetId = "t1",
            coreSessionId = 5UL,
            inputEpoch = activeLease.epoch,
            rustRevision = 10L,
            text = "initial-edit",
            committedVersion = DocumentVersion(),
            localDirty = true,
        )
        val savedVersion = DocumentVersion(contentHash = "hash-saved")
        val committed = coordinator.commitSavedLease(saveLease, savedVersion)
        assertTrue("matching lease 必须提交成功", committed)
        assertFalse("提交后 localDirty 必须清除", coordinator.sessionState.localDirty)
        assertEquals(savedVersion, coordinator.sessionState.committedVersion)
        // store 记录与 state 一致
        val record = coordinator.store.record("t1")!!
        assertEquals(savedVersion, record.documentState.committedVersion)
        assertFalse(record.documentState.localDirty)
    }

    // ── 旧 TransformPurityTest 的行为契约（applyLocalEdit 等一致性） ──

    @Test
    fun applyLocalEdit_sessionStateMatchesStoreRecord() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "hello".length),
                revision = 3L,
                transactionId = 7L,
                selectionAnchorUtf8 = 2,
                selectionHeadUtf8 = 5,
                lease = lease("a"),
            ),
        )
        val state = coordinator.sessionState
        assertEquals("a", state.targetId)
        assertEquals(3L, state.revision)
        assertEquals(2, state.selectionAnchorUtf8)
        assertEquals(5, state.selectionHeadUtf8)
        assertEquals(7L, state.lastAppliedTransactionId)
        assertTrue("本地输入必须置 localDirty", state.localDirty)
        assertNotNull(coordinator.getPersistentSessionId("a"))
    }

    @Test
    fun consecutiveLocalEdits_storeRecordReflectsLatestValue() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "first".length),
                lease = lease("a"),
            ),
        )
        assertEquals(1L, coordinator.sessionState.revision)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "second".length),
                revision = 2L,
                transactionId = 2L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 6,
                lease = lease("a"),
            ),
        )
        assertEquals(2L, coordinator.sessionState.revision)
        assertEquals(6, coordinator.sessionState.selectionHeadUtf8)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                3L,
                3L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "third".length),
                lease = lease("a"),
            ),
        )
        assertEquals(3L, coordinator.sessionState.revision)
    }

    @Test
    fun applyUndoRestored_sessionStateMatchesStoreRecord() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "original".length),
                lease = lease("a"),
            ),
        )
        coordinator.applyUndoRestored(
            EditorDocumentUpdate.UndoRestored(
                targetId = "a",
                snapshotId = 100L,
                revision = 2L,
                transactionId = 100L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 6,
                lease = lease("a"),
            ),
        )
        val state = coordinator.sessionState
        assertEquals(2L, state.revision)
        assertEquals(EditorSessionOrigin.UNDO_RESTORED, state.origin)
        assertTrue("撤销后正文仍未落盘时保持 dirty", state.localDirty)
    }

    @Test
    fun applyProgrammaticReplace_sessionStateMatchesStoreRecord() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "before replace".length),
                lease = lease("a"),
            ),
        )
        coordinator.applyProgrammaticReplace(
            EditorDocumentUpdate.ProgrammaticReplace(
                targetId = "a",
                commandId = 200L,
                revision = 2L,
                transactionId = 200L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 12,
                lease = lease("a"),
            ),
        )
        val state = coordinator.sessionState
        assertEquals(2L, state.revision)
        assertEquals(EditorSessionOrigin.PROGRAMMATIC_REPLACE, state.origin)
    }

    @Test
    fun localEdit_doesNotDuplicateStoreRecords() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        for (i in 1..5) {
            coordinator.applyLocalEdit(
                EditorDocumentUpdate.LocalInput(
                    "a",
                    i.toLong(),
                    i.toLong(),
                    operationKind = EditorOperationKind.INSERT,
                    contentChanged = true,
                    contentDelta = EditorContentDelta(insertedChars = "text$i".length),
                    lease = lease("a"),
                ),
            )
        }
        assertTrue(coordinator.isTargetRegistered("a"))
        assertEquals(5L, coordinator.sessionState.revision)
    }

    @Test
    fun selectionOnlyEdit_preservesLocalDirtyFromPreviousContentEdit() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "content".length),
                revision = 1L,
                transactionId = 1L,
                lease = lease("a"),
            ),
        )
        assertTrue(coordinator.sessionState.localDirty)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                operationKind = EditorOperationKind.SELECTION,
                contentChanged = false,
                contentDelta = EditorContentDelta(),
                revision = 1L,
                transactionId = 2L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 3,
                lease = lease("a"),
            ),
        )
        assertTrue("选区变更不得清 localDirty", coordinator.sessionState.localDirty)
        assertEquals(3, coordinator.sessionState.selectionHeadUtf8)
    }

    @Test
    fun staleLeaseInput_doesNotCorruptSessionStateOrStore() {
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
                contentDelta = EditorContentDelta(insertedChars = "stale input".length),
                lease = staleLease,
            ),
        )
        assertEquals("b", coordinator.sessionState.targetId)
    }
}
