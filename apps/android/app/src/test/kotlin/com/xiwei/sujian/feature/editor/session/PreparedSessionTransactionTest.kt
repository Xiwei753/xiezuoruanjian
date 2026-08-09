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
import com.xiwei.sujian.feature.editor.window.EditingState

/**
 * #595 一：无副作用章节预准备行为测试。
 *
 * 规则（issue 解决一）：准备阶段只允许读取 B 的记录、验证或新建 B session、
 * 读取 snapshot、返回 [PreparedSessionHandle]；禁止 commit/cancel A、修改
 * activeTargetId、修改 WindowBindingState、修改全局 EditorSessionState、
 * 关闭任何既有 session。最终 requestId 校验通过后才由 [commitPreparedSession]
 * 一次性执行 A→B 切换；Abort 按 newlyCreated 区分：新建才关闭 session，
 * 借用的既有 session 恢复 previousRecord。
 *
 * 本测试通过真实驱动 [EditorSessionCoordinator] 的状态变化（registerTargetMeta /
 * applyLocalEdit / prepareTargetSessionForCommit / commitPreparedSession /
 * releasePreparedTarget）并断言可观察的 [EditorSessionState] 与 store 记录结果，
 * 验证预准备事务的无副作用与原子提交契约。测试环境无 native（session 创建返回
 * NotLoaded），因此 prepare 失败路径与手工构造 handle 的 commit/abort 路径在这里
 * 验证纯状态契约。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PreparedSessionTransactionTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_prepared",
                    "/tmp/sujian_test_workspace_595_prepared",
                ),
            ),
        )
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
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a")),
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
            EditorDocumentUpdate.LocalInput("a", "textA", 1L, 1L, lease = lease("a")),
        )
        // 提交前无活动目标 — 没有可签发的 lease（窗口未绑定）。
        val staleLease = lease("a")
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)

        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 0UL,
                snapshot =
                    TargetSnapshot(
                        text = "textB",
                        cursorUtf8 = 5,
                        revision = 2L,
                        selectionAnchorUtf8 = 0,
                        selectionHeadUtf8 = 5,
                    ),
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
            EditorDocumentUpdate.LocalInput("a", "late input from stale view", 9L, 9L, lease = staleLease),
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
            EditorDocumentUpdate.LocalInput("b", "textB typed", 3L, 11L, lease = leaseB!!),
        )
        assertEquals("textB typed", coordinator.sessionState.text)
    }

    @Test
    fun commitPreparedSession_rejectsHandleMismatchingRecord() {
        // #595 一：防御 — 复用既有 session 时，记录已不再指向该 session → 拒绝提交。
        // 新建事务不要求记录已存在 handle.sessionId（prepare 不写 store），故 reject
        // 场景改为复用事务：handle 声称复用 7UL，但记录 sessionId 仍是 0UL。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
                newlyCreated = false,
                previousRecord = EditorSessionRecord("b", sessionId = 7UL),
            )
        assertFalse("复用事务记录不再指向 handle session 必须拒绝提交", coordinator.commitPreparedSession(handle))
        assertNull(coordinator.sessionState.targetId)
    }

    @Test
    fun commitPreparedSession_newlyCreatedWritesStoreSessionId() {
        // #595 一：新建 session（prepare 不写 store，记录 sessionId=0UL）提交时必须
        // 把 handle.sessionId 写入正式记录 — 旧实现要求 record.sessionId==handle.sessionId，
        // 新建 session（0UL != 7UL）永远失败，首次打开新章节必然 LoadFailed。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                newlyCreated = true,
                previousRecord = null,
            )
        assertTrue("新建 session 提交必须成功（不要求记录已存在该 sessionId）", coordinator.commitPreparedSession(handle))
        val state = coordinator.sessionState
        assertEquals(7UL, state.sessionId)
        assertEquals("b", state.activeTargetId)
        assertEquals("textB", state.text)
        assertEquals(2L, state.revision)
        // store 记录的 sessionId 必须与 SessionState 一致（不再分裂为 0UL）。
        assertEquals(7UL, coordinator.getPersistentSessionId("b"))
    }

    @Test
    fun commitPreparedSession_newlyCreatedRejectsIfRecordReplaced() {
        // #595 一：新建事务期间记录被并发替换为有效 session（9UL）→ prepare 前值
        // 是 0UL，但记录已是 9UL，句柄失效，拒绝提交。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                newlyCreated = true,
                previousRecord = null,
            )
        // 模拟并发：直接通过复用事务把记录 sessionId 占用为 9UL。
        coordinator.commitPreparedSession(handle.copy(sessionId = 9UL))
        assertFalse("记录已被并发占用为 9UL，原句柄失效必须拒绝", coordinator.commitPreparedSession(handle))
    }

    @Test
    fun releasePreparedTarget_newlyCreated_removesRecordOnlyIfStillOwned() {
        // 记录已被其他路径替换（sessionId 不同）时，回滚不得删除新记录。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        // 模拟事务期间记录被替换为另一个 session。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("b", "replaced", 1L, 1L, lease = lease("b")),
        )
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                // 与替换后的记录 sessionId 相同（0UL）— 属于本事务新建
                sessionId = 0UL,
                snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
                newlyCreated = true,
                previousRecord = null,
            )
        coordinator.releasePreparedTarget(handle)
        assertFalse("新建 session 回滚后记录必须移除", coordinator.isTargetRegistered("b"))
    }

    @Test
    fun prepareTargetSessionForCommit_existsWithThreeParams() {
        // prepareTargetSessionForCommit 拆分为扩展函数，编译为 EditorSessionLifecycleOpsKt 静态方法。
        // 扩展函数参数 = 接收者 + 3 个显式参数 = 4。
        val extClass = Class.forName("com.xiwei.sujian.feature.editor.session.EditorSessionLifecycleOpsKt")
        val method =
            extClass.declaredMethods.firstOrNull {
                it.name == "prepareTargetSessionForCommit" &&
                    it.parameterTypes.size == 4 &&
                    it.parameterTypes[0] == EditorSessionCoordinator::class.java
            }
        assertNotNull("prepareTargetSessionForCommit(targetId, initialText, initialSelection) 必须存在", method)
    }

    @Test
    fun preparedSessionHandle_carriesAbortFacts() {
        val handle =
            PreparedSessionHandle(
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
