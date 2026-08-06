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
 * #595 二：输入 lease / epoch 契约测试。
 *
 * 窗口绑定时产生 EditorInputLease(targetId, sessionId, epoch)；每次
 * onLocalEdit/onExternalEdit/onContentChanged 提交都携带 lease。Coordinator
 * 只接受仍匹配当前活动 target、session 和 epoch 的事件；章节切换提交、
 * 业务关闭、窗口解绑都会使旧 lease 失效 — 旧 View 即使晚到一帧，也不能
 * 修改新章节的会话或 ViewModel。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorInputLeaseContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_lease")
        ))
    }

    private fun lease(targetId: String, sessionId: ULong = 0UL, epoch: Long = 0L): EditorInputLease =
        EditorInputLease(targetId, sessionId, epoch)

    @Test
    fun currentInputLease_nullWithoutActiveTarget() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        assertNull("未绑定/无活动目标时必须返回 null", coordinator.currentInputLease())
    }

    @Test
    fun currentInputLease_matchesActiveTarget() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        val handle = PreparedSessionHandle(
            targetId = "a",
            sessionId = 0UL,
            snapshot = TargetSnapshot("text", 4, 1L, 0, 4),
            newlyCreated = true,
            previousRecord = null,
        )
        assertTrue(coordinator.commitPreparedSession(handle))
        val lease = coordinator.currentInputLease()
        assertNotNull(lease)
        assertEquals("a", lease!!.targetId)
    }

    @Test
    fun defaultLeaseWithoutSession_isAcceptedOnlyForPureStateConstruction() {
        // 无活动绑定 + 无 session 时（纯状态测试/初始构造），默认 lease
        // （epoch 0、target 匹配事件）被接受 — 生产环境窗口回调总在绑定后发生：
        // 活动 target 存在时默认 lease 的 sessionId=0UL 与真实 session 不匹配而被拒绝。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "text", 1L, 1L)
        )
        assertEquals("text", coordinator.sessionState.text)
        // 一旦进入真实绑定（commitPreparedSession 激活带 session 的状态），
        // 默认 lease 必须被拒绝。
        val handle = PreparedSessionHandle(
            targetId = "a",
            sessionId = 0UL,
            snapshot = TargetSnapshot("text", 4, 1L, 0, 4),
            newlyCreated = true,
            previousRecord = null,
        )
        assertTrue(coordinator.commitPreparedSession(handle))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "stale default lease", 2L, 2L)
        )
        assertEquals("绑定后默认 lease（epoch 已失效）必须被拒绝", "text", coordinator.sessionState.text)
    }

    @Test
    fun staleEpoch_afterCommitPreparedSession_isRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a"))
        )
        val staleLease = lease("a")
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle = PreparedSessionHandle(
            targetId = "b",
            sessionId = 0UL,
            snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
            newlyCreated = true,
            previousRecord = null,
        )
        assertTrue(coordinator.commitPreparedSession(handle))
        assertFalse(coordinator.isInputLeaseCurrent(staleLease, "a"))

        // 旧 A 的撤销/程序化事件同样被拒绝。
        coordinator.applyUndoRestored(
            EditorDocumentUpdate.UndoRestored("a", "undo text", 1L, 9L, 9L, lease = staleLease)
        )
        assertEquals("textB", coordinator.sessionState.text)
        coordinator.applyProgrammaticReplace(
            EditorDocumentUpdate.ProgrammaticReplace("a", "replace text", 1L, 10L, 10L, lease = staleLease)
        )
        assertEquals("textB", coordinator.sessionState.text)
    }

    @Test
    fun staleEpoch_afterCloseTarget_isRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a"))
        )
        val staleLease = lease("a")
        coordinator.closeTarget("a", SessionCloseReason.WORKSPACE_NAVIGATION)
        assertFalse("业务关闭后旧 lease 必须失效", coordinator.isInputLeaseCurrent(staleLease, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "resurrected", 2L, 2L, lease = staleLease)
        )
        assertNull("关闭后晚到输入不得复活会话状态", coordinator.sessionState.targetId)
    }

    @Test
    fun staleEpoch_afterDetachWindowBinding_isRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a"))
        )
        val staleLease = lease("a")
        coordinator.detachWindowBinding("w1", "a")
        assertFalse("窗口解绑后旧 lease 必须失效", coordinator.isInputLeaseCurrent(staleLease, "a"))
    }

    @Test
    fun currentEpochEventForActiveTarget_isAccepted() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a"))
        )
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle = PreparedSessionHandle(
            targetId = "b",
            sessionId = 0UL,
            snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
            newlyCreated = true,
            previousRecord = null,
        )
        coordinator.commitPreparedSession(handle)
        val newLease = coordinator.currentInputLease()
        assertNotNull(newLease)
        // 新章节的输入携带新 lease → 接受。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("b", "textB typed", 3L, 11L, lease = newLease!!)
        )
        assertEquals("textB typed", coordinator.sessionState.text)
        assertEquals(3L, coordinator.sessionState.revision)
    }

    @Test
    fun wrongTargetLease_isRejectedWhileAnotherTargetActive() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a"))
        )
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle = PreparedSessionHandle(
            targetId = "b",
            sessionId = 0UL,
            snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
            newlyCreated = true,
            previousRecord = null,
        )
        coordinator.commitPreparedSession(handle)
        // epoch 是当前值但 target 是 a → 拒绝（当前活动是 b）。
        val current = coordinator.currentInputLease()!!
        val wrongTargetLease = current.copy(targetId = "a")
        assertFalse(coordinator.isInputLeaseCurrent(wrongTargetLease, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "cross talk", 4L, 12L, lease = wrongTargetLease)
        )
        assertEquals("textB", coordinator.sessionState.text)
    }

    @Test
    fun editorInputLease_existsAsTypedValue() {
        val lease = lease("t1", sessionId = 3UL, epoch = 7L)
        assertEquals("t1", lease.targetId)
        assertEquals(3UL, lease.sessionId)
        assertEquals(7L, lease.epoch)
    }
}
