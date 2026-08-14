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
 * #595 二/三：会话层状态行为测试 — 订阅 sessionStateFlow 后输入和章节切换的真实状态变化。
 *
 * 静态结构约束（字段/方法存在性、Compose mutableState 字段检查）已移入
 * [com.xiwei.sujian.arch.EditorSessionLayerArchitectureTest]；本文件只保留运行时行为：
 * - sessionStateFlow 是唯一可观察状态源，订阅后能收到真实状态变化；
 * - activeTargetId / editingState / windowBindingState 三个 getter 从同一快照派生，
 *   不存在独立的可写第二份状态；
 * - 本地输入后快照的 text/revision/selection 同步更新；
 * - 章节切换提交后快照原子切换到新 target，旧 target 状态不再可写。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SessionLayerNoMutableStateTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_session_layer",
                    "/tmp/sujian_test_workspace_595_session_layer",
                ),
            ),
        )
    }

    @Test
    fun sessionStateFlow_initialSnapshotIsIdle() {
        val coordinator = createCoordinator()
        val snapshot = coordinator.sessionStateFlow.value
        assertEquals(EditorSessionState(), snapshot)
        assertNull(snapshot.activeTargetId)
        assertEquals(EditingState.IDLE, snapshot.editingState)
        assertEquals(WindowBindingState.Idle, snapshot.bindingState)
    }

    @Test
    fun valueGetters_deriveFromSameSnapshotAsSessionStateFlow() {
        // activeTargetId / editingState / windowBindingState 三个 getter 必须从
        // sessionStateFlow.value 派生 — 不存在独立的可写第二份状态。
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
                lease = EditorInputLease("a", 0UL, 0L),
            ),
        )

        val snapshot = coordinator.sessionStateFlow.value
        assertEquals(snapshot.activeTargetId, coordinator.activeTargetId)
        assertEquals(snapshot.editingState, coordinator.editingState)
        assertEquals(snapshot.bindingState, coordinator.windowBindingState)
        assertEquals(snapshot.targetId, coordinator.sessionState.targetId)
    }

    @Test
    fun localEdit_advancesSnapshotTextRevisionAndSelection() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        val lease = EditorInputLease("a", 0UL, 0L)

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
                lease = lease,
            ),
        )
        val snapshot = coordinator.sessionStateFlow.value
        assertEquals("a", snapshot.targetId)
        assertEquals(3L, snapshot.revision)
        assertEquals(7L, snapshot.lastAppliedTransactionId)
        assertEquals(2, snapshot.selectionAnchorUtf8)
        assertEquals(5, snapshot.selectionHeadUtf8)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, snapshot.origin)
        assertTrue("本地输入必须置 localDirty", snapshot.localDirty)
    }

    @Test
    fun chapterSwitch_atomicallySwapsSnapshotToNewTarget() {
        // 章节切换提交后快照原子切换到新 target — 旧 target 的 lease 失效，
        // 旧 View 晚到的输入不得写入新章节的快照。
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
                lease = EditorInputLease("a", 0UL, 0L),
            ),
        )
        val staleLeaseA = EditorInputLease("a", 0UL, 0L)

        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 5UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        assertTrue(coordinator.commitPreparedSession(handle))

        val snapshot = coordinator.sessionStateFlow.value
        assertEquals("b", snapshot.targetId)
        // #624 评论17 问题2：commitPreparedSession 后 target 进入 Detached（不造假窗口）。
        assertNull(snapshot.activeTargetId)
        assertEquals(5UL, snapshot.sessionId)
        assertEquals(2L, snapshot.revision)
        assertEquals(EditingState.IDLE, snapshot.editingState)

        // 模拟真实窗口绑定完成 — 激活 target 以签发 lease。
        coordinator.activateAttachedForTest("b")

        // 旧 A 的 lease 失效 — 晚到的输入不得修改快照。
        assertFalse(coordinator.isInputLeaseCurrent(staleLeaseA, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                9L,
                9L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "late from A".length),
                lease = staleLeaseA,
            ),
        )
        assertEquals("旧 A 晚到输入不得写入 B 快照", "b", coordinator.sessionStateFlow.value.targetId)

        // 新章节的 lease 被接受，输入推进快照。
        val leaseB = coordinator.currentInputLease()!!
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "b",
                3L,
                12L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "textB edited".length),
                lease = leaseB,
            ),
        )
        val after = coordinator.sessionStateFlow.value
        assertEquals(3L, after.revision)
    }

    @Test
    fun closeTarget_resetsSnapshotToIdle() {
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
                lease = EditorInputLease("a", 0UL, 0L),
            ),
        )
        assertNotNull(coordinator.sessionStateFlow.value.targetId)

        coordinator.closeTarget("a", SessionCloseReason.WORKSPACE_NAVIGATION)
        val snapshot = coordinator.sessionStateFlow.value
        assertNull(snapshot.targetId)
        assertNull(snapshot.activeTargetId)
        assertEquals(EditingState.IDLE, snapshot.editingState)
        assertEquals(WindowBindingState.Idle, snapshot.bindingState)
    }

    @Test
    fun detachWindowBinding_syncsSnapshotBindingState() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = false)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "text".length),
                lease = EditorInputLease("a", 0UL, 0L),
            ),
        )
        coordinator.detachWindowBinding("w1", "a")
        val snapshot = coordinator.sessionStateFlow.value
        assertEquals(WindowBindingState.Idle, snapshot.bindingState)
        assertNull(snapshot.targetId)
    }

    @Test
    fun motionPolicyFlow_isSeparateWritableSourceAndAtomicallyApplied() {
        // EditorMotionPolicy 是会话层唯一另一个可写事实源（与 sessionStateFlow 并列），
        // 通过 applyMotionPolicy 原子更新。验证它确实可独立更新且不影响 sessionState。
        val coordinator = createCoordinator()
        val initialPolicy = coordinator.motionPolicyFlow.value
        val newPolicy =
            com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy(
                reduceMotion = true,
            )
        coordinator.applyMotionPolicy(newPolicy)
        assertEquals(newPolicy, coordinator.motionPolicyFlow.value)
        assertEquals(newPolicy, coordinator.getMotionPolicy())

        // motionPolicy 更新不得污染 sessionState 快照。
        val snapshot = coordinator.sessionStateFlow.value
        assertEquals(EditorSessionState(), snapshot)
    }
}
