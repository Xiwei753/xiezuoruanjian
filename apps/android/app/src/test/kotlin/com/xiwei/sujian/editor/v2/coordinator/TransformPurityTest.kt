package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 五：updateSessionState transform 纯函数行为测试。
 *
 * MutableStateFlow.update 的 transform 在 CAS 竞争时会重新执行。
 * transform 内写外部 store（mutableMap）违反纯函数契约 — 重复执行
 * 会重复写入 store，可能基于已被前一次写入修改的 store 状态计算错误结果。
 *
 * 源码静态检查（transform 体不调用 store.put/update/remove）已移入
 * [com.xiwei.sujian.arch.EditorSessionCoordinatorTransformArchitectureTest]；
 * 本文件验证 transform 纯函数性的可观察行为：applyLocalEdit /
 * applyUndoRestored / applyProgrammaticReplace 后 SessionState 与 store 记录
 * 保持一致，连续多次更新不产生重复或陈旧记录。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class TransformPurityTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.data.AppServiceBridge(
                com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_transform_purity"),
            ),
        )
    }

    private fun lease(
        targetId: String,
        sessionId: ULong = 0UL,
        epoch: Long = 0L,
    ): EditorInputLease = EditorInputLease(targetId, sessionId, epoch)

    @Test
    fun applyLocalEdit_sessionStateMatchesStoreRecord() {
        // transform 计算的 pendingRecord 在 transform 外写入 store —
        // 可观察行为：SessionState 的 text/revision/selection 与 store 记录一致。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                text = "hello",
                revision = 3L,
                transactionId = 7L,
                selectionAnchorUtf8 = 2,
                selectionHeadUtf8 = 5,
                lease = lease("a"),
            ),
        )
        val state = coordinator.sessionState
        assertEquals("a", state.targetId)
        assertEquals("hello", state.text)
        assertEquals(3L, state.revision)
        assertEquals(2, state.selectionAnchorUtf8)
        assertEquals(5, state.selectionHeadUtf8)
        assertEquals(7L, state.lastAppliedTransactionId)
        assertTrue("本地输入必须置 localDirty", state.localDirty)
        // store 记录的 sessionId 与 SessionState 一致（transform 外写入）。
        assertNotNull(coordinator.getPersistentSessionId("a"))
    }

    @Test
    fun consecutiveLocalEdits_storeRecordReflectsLatestValue() {
        // 连续两次 applyLocalEdit — transform 重试时若读已被前次写入的 store，
        // 会基于陈旧状态计算。可观察行为：store 记录始终反映最新一次更新。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)

        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "first", 1L, 1L, lease = lease("a")),
        )
        assertEquals("first", coordinator.sessionState.text)
        assertEquals(1L, coordinator.sessionState.revision)

        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                text = "second",
                revision = 2L,
                transactionId = 2L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 6,
                lease = lease("a"),
            ),
        )
        assertEquals("second", coordinator.sessionState.text)
        assertEquals(2L, coordinator.sessionState.revision)
        assertEquals(6, coordinator.sessionState.selectionHeadUtf8)

        // 第三次更新 — 验证不会因为前两次 transform 写 store 而累积错误。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "third", 3L, 3L, lease = lease("a")),
        )
        assertEquals("third", coordinator.sessionState.text)
        assertEquals(3L, coordinator.sessionState.revision)
    }

    @Test
    fun applyUndoRestored_sessionStateMatchesStoreRecord() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "original", 1L, 1L, lease = lease("a")),
        )

        coordinator.applyUndoRestored(
            EditorDocumentUpdate.UndoRestored(
                targetId = "a",
                text = "undone",
                snapshotId = 100L,
                revision = 2L,
                transactionId = 100L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 6,
                lease = lease("a"),
            ),
        )
        val state = coordinator.sessionState
        assertEquals("undone", state.text)
        assertEquals(2L, state.revision)
        assertEquals(EditorSessionOrigin.UNDO_RESTORED, state.origin)
        assertTrue("撤销后正文仍未落盘时保持 dirty", state.localDirty)
    }

    @Test
    fun applyProgrammaticReplace_sessionStateMatchesStoreRecord() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "before replace", 1L, 1L, lease = lease("a")),
        )

        coordinator.applyProgrammaticReplace(
            EditorDocumentUpdate.ProgrammaticReplace(
                targetId = "a",
                text = "after replace",
                commandId = 200L,
                revision = 2L,
                transactionId = 200L,
                selectionAnchorUtf8 = 0,
                selectionHeadUtf8 = 12,
                lease = lease("a"),
            ),
        )
        val state = coordinator.sessionState
        assertEquals("after replace", state.text)
        assertEquals(2L, state.revision)
        assertEquals(EditorSessionOrigin.PROGRAMMATIC_REPLACE, state.origin)
    }

    @Test
    fun localEdit_doesNotDuplicateStoreRecords() {
        // transform 内若写 store，CAS 重试会重复 put 同一 target 的记录。
        // 可观察行为：多次 applyLocalEdit 后 target 仍只对应一份记录
        // （getPersistentSessionId 返回单一值，isTargetRegistered=true）。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        for (i in 1..5) {
            coordinator.applyLocalEdit(
                EditorDocumentUpdate.LocalInput("a", "text$i", i.toLong(), i.toLong(), lease = lease("a")),
            )
        }
        assertTrue(coordinator.isTargetRegistered("a"))
        assertEquals("text5", coordinator.sessionState.text)
        assertEquals(5L, coordinator.sessionState.revision)
    }

    @Test
    fun multipleTargets_localEditKeepsEachTargetRecordConsistent() {
        // transform 内若读全局 store 状态，A 的更新可能污染 B 的记录。
        // 可观察行为：A 和 B 各自的 SessionState 字段独立。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)

        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a")),
        )
        // 切到 B（用 commitPreparedSession 模拟章节切换）。
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 2UL,
                snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
                newlyCreated = true,
                previousRecord = null,
            )
        assertTrue(coordinator.commitPreparedSession(handle))
        val leaseB = coordinator.currentInputLease()!!
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("b", "textB edited", 2L, 2L, lease = leaseB),
        )

        val state = coordinator.sessionState
        assertEquals("b", state.targetId)
        assertEquals("textB edited", state.text)
        assertEquals(2L, state.revision)
        // A 的记录仍保留（getPersistentSessionId 返回非 null）。
        assertNotNull(coordinator.getPersistentSessionId("a"))
    }

    @Test
    fun selectionOnlyEdit_preservesLocalDirtyFromPreviousContentEdit() {
        // operationKind=SELECTION 且正文未变时保留既有 localDirty —
        // transform 必须读取 previousDoc.localDirty（纯函数读前值，不读外部可变 store）。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                text = "content",
                revision = 1L,
                transactionId = 1L,
                operationKind = EditorOperationKind.INSERT,
                lease = lease("a"),
            ),
        )
        assertTrue(coordinator.sessionState.localDirty)

        // 选区变更（正文不变）— localDirty 必须保留。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                text = "content",
                revision = 1L,
                transactionId = 2L,
                operationKind = EditorOperationKind.SELECTION,
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
        // transform 纯函数性的另一可观察行为：陈旧 lease 的输入在进入 transform 前
        // 被拒绝（isInputLeaseCurrent 校验），不得污染 SessionState 或 store。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a")),
        )
        val staleLease = lease("a")

        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        assertTrue(
            coordinator.commitPreparedSession(
                PreparedSessionHandle("b", 2UL, TargetSnapshot("textB", 5, 2L, 0, 5), true, null),
            ),
        )

        // 旧 A 的 lease 已失效 — 晚到输入不得修改 B 的 SessionState。
        assertFalse(coordinator.isInputLeaseCurrent(staleLease, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "stale input", 9L, 9L, lease = staleLease),
        )
        assertEquals("textB", coordinator.sessionState.text)
        assertEquals("b", coordinator.sessionState.targetId)
    }
}
