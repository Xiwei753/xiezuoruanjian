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
            com.xiwei.sujian.data.AppServiceBridge(
                com.xiwei.sujian.data.WriterAppServiceHolder(
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
            EditorDocumentUpdate.LocalInput("a", "text", 1L, 1L, lease = EditorInputLease("a", 0UL, 0L)),
        )

        val snapshot = coordinator.sessionStateFlow.value
        assertEquals(snapshot.activeTargetId, coordinator.activeTargetId)
        assertEquals(snapshot.editingState, coordinator.editingState)
        assertEquals(snapshot.bindingState, coordinator.windowBindingState)
        assertEquals(snapshot.targetId, coordinator.sessionState.targetId)
        assertEquals(snapshot.text, coordinator.sessionState.text)
    }

    @Test
    fun localEdit_advancesSnapshotTextRevisionAndSelection() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        val lease = EditorInputLease("a", 0UL, 0L)

        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "a",
                text = "hello",
                revision = 3L,
                transactionId = 7L,
                selectionAnchorUtf8 = 2,
                selectionHeadUtf8 = 5,
                lease = lease,
            ),
        )
        val snapshot = coordinator.sessionStateFlow.value
        assertEquals("a", snapshot.targetId)
        assertEquals("hello", snapshot.text)
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
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = EditorInputLease("a", 0UL, 0L)),
        )
        val staleLeaseA = EditorInputLease("a", 0UL, 0L)

        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 5UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                newlyCreated = true,
                previousRecord = null,
            )
        assertTrue(coordinator.commitPreparedSession(handle))

        val snapshot = coordinator.sessionStateFlow.value
        assertEquals("b", snapshot.targetId)
        assertEquals("b", snapshot.activeTargetId)
        assertEquals(5UL, snapshot.sessionId)
        assertEquals("textB", snapshot.text)
        assertEquals(2L, snapshot.revision)
        assertEquals(EditingState.BINDING, snapshot.editingState)

        // 旧 A 的 lease 失效 — 晚到的输入不得修改快照。
        assertFalse(coordinator.isInputLeaseCurrent(staleLeaseA, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "late from A", 9L, 9L, lease = staleLeaseA),
        )
        assertEquals("旧 A 晚到输入不得写入 B 快照", "textB", coordinator.sessionStateFlow.value.text)

        // 新章节的 lease 被接受，输入推进快照。
        val leaseB = coordinator.currentInputLease()!!
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("b", "textB edited", 3L, 12L, lease = leaseB),
        )
        val after = coordinator.sessionStateFlow.value
        assertEquals("textB edited", after.text)
        assertEquals(3L, after.revision)
    }

    @Test
    fun closeTarget_resetsSnapshotToIdle() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "text", 1L, 1L, lease = EditorInputLease("a", 0UL, 0L)),
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
            EditorDocumentUpdate.LocalInput("a", "text", 1L, 1L, lease = EditorInputLease("a", 0UL, 0L)),
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
            com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy(
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
