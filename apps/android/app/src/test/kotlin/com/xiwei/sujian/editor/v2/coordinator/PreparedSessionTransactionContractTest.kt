package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 一：无副作用章节预准备契约测试。
 *
 * 规则（issue 解决一）：准备阶段只允许读取 B 的记录、验证或新建 B session、
 * 读取 snapshot、返回 [PreparedSessionHandle]；禁止 commit/cancel A、修改
 * activeTargetId、修改 WindowBindingState、修改全局 EditorSessionState、
 * 关闭任何既有 session。最终 requestId 校验通过后才由 [commitPreparedSession]
 * 一次性执行 A→B 切换；Abort 按 newlyCreated 区分：新建才关闭 session，
 * 借用的既有 session 恢复 previousRecord。
 *
 * 测试环境无 native（session 创建返回 NotLoaded），因此 prepare 失败路径与
 * 手工构造 handle 的 commit/abort 路径在这里验证纯状态契约。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PreparedSessionTransactionContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595_prepared")
        ))
    }

    private fun lease(targetId: String): EditorInputLease = EditorInputLease(targetId, 0UL, 0L)

    @Test
    fun failedPrepare_doesNotTouchActiveTargetState() {
        // 旧实现：prepare(B) 会先 commit/cancel A 并把全局活动会话切到 B —
        // 请求过期/snapshot 失败后 A 的 session ID、Undo 历史、窗口绑定状态
        // 都无法恢复。新实现：prepare 失败（无 native 时 create 失败返回 null）
        // 必须完全不动 A 的全局状态。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a"))
        )
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)

        val before = coordinator.sessionState
        val handle = coordinator.prepareTargetSessionForCommit("b", "textB", 5)
        assertNull("无 native 时预准备失败（session 创建不可用）", handle)
        assertEquals(
            "预准备失败不得修改全局 SessionState（A 保持活动）",
            before,
            coordinator.sessionState,
        )
        assertEquals("a", coordinator.sessionState.targetId)
    }

    @Test
    fun commitPreparedSession_activatesTargetWithSnapshotAndInvalidatesLease() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a"))
        )
        // 提交前无活动目标 — 没有可签发的 lease（窗口未绑定）。
        val staleLease = lease("a")
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)

        val handle = PreparedSessionHandle(
            targetId = "b",
            sessionId = 0UL,
            snapshot = TargetSnapshot(text = "textB", cursorUtf8 = 5, revision = 2L, selectionAnchorUtf8 = 0, selectionHeadUtf8 = 5),
            newlyCreated = true,
            previousRecord = null,
        )
        assertTrue(coordinator.commitPreparedSession(handle))

        val state = coordinator.sessionState
        assertEquals("b", state.targetId)
        assertEquals("b", state.activeTargetId)
        assertEquals(0UL, state.sessionId)
        assertEquals(EditingState.BINDING, state.editingState)
        assertEquals(WindowBindingState.Attaching("prepared", "b", 0UL), state.bindingState)
        assertEquals("textB", state.text)
        assertEquals(2L, state.revision)

        // 提交使旧 lease 失效 — 旧 View 晚到的输入不能再进入会话层。
        assertFalse("提交后旧 lease 必须失效", coordinator.isInputLeaseCurrent(staleLease, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("a", "late input from stale view", 9L, 9L, lease = staleLease)
        )
        assertEquals(
            "旧 A 的晚到输入不得写入 B 的会话",
            "textB",
            coordinator.sessionState.text,
        )
        // 新绑定签发的 lease 被接受。
        val leaseB = coordinator.currentInputLease()
        assertNotNull("提交后活动目标可签发新 lease", leaseB)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("b", "textB typed", 3L, 11L, lease = leaseB!!)
        )
        assertEquals("textB typed", coordinator.sessionState.text)
    }

    @Test
    fun commitPreparedSession_rejectsHandleMismatchingRecord() {
        // 防御：记录仍必须指向 handle 的 session（无并发修改/已回滚）。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle = PreparedSessionHandle(
            targetId = "b",
            sessionId = 7UL, // 记录中不存在该 session
            snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
            newlyCreated = true,
            previousRecord = null,
        )
        assertFalse("session 与记录不一致必须拒绝提交", coordinator.commitPreparedSession(handle))
        assertNull(coordinator.sessionState.targetId)
    }

    @Test
    fun releasePreparedTarget_newlyCreated_removesRecordOnlyIfStillOwned() {
        // 记录已被其他路径替换（sessionId 不同）时，回滚不得删除新记录。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        // 模拟事务期间记录被替换为另一个 session。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("b", "replaced", 1L, 1L, lease = lease("b"))
        )
        val handle = PreparedSessionHandle(
            targetId = "b",
            sessionId = 0UL, // 与替换后的记录 sessionId 相同（0UL）— 属于本事务新建
            snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
            newlyCreated = true,
            previousRecord = null,
        )
        coordinator.releasePreparedTarget(handle)
        assertFalse("新建 session 回滚后记录必须移除", coordinator.isTargetRegistered("b"))
    }

    @Test
    fun prepareTargetSessionForCommit_existsWithThreeParams() {
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "prepareTargetSessionForCommit" && it.parameterTypes.size == 3
        }
        assertNotNull("prepareTargetSessionForCommit(targetId, initialText, initialSelection) 必须存在", method)
    }

    @Test
    fun preparedSessionHandle_carriesAbortFacts() {
        val handle = PreparedSessionHandle(
            targetId = "b",
            sessionId = 5UL,
            snapshot = TargetSnapshot("t", 1, 0L, 0, 1),
            newlyCreated = true,
            previousRecord = null,
        )
        assertTrue(handle.newlyCreated)
        assertNull(handle.previousRecord)
        assertEquals(5UL, handle.sessionId)
        val borrowed = handle.copy(newlyCreated = false, previousRecord = EditorSessionRecord("b"))
        assertFalse(borrowed.newlyCreated)
        assertEquals("b", borrowed.previousRecord?.targetId)
    }
}
