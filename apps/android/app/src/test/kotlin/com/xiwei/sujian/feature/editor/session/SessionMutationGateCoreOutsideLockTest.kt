package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论17 问题3：Core 调用不得持 mutationLock。
 *
 * `commitPreparedSession` 关闭旧活动目标的 Core session 必须在 [EditorSessionCoordinator.mutationLock]
 * 之外执行（锁内读取需要的 id/revision → 解锁调用 Core → 再进锁校验前提仍成立并提交）。
 * 旧实现在 `mutateSession` 闭包内直接调 `coordinator.closeSession`，违反
 * "createSession/validateSession/querySnapshot/closeSession 不得持锁" 契约。
 *
 * 测试思路：override `closeSession` 检查 `mutationLock.isHeldByCurrentThread`，
 * 注册非持久目标 "a" 并激活，注册未激活目标 "b"（store 里 sessionId=0UL），
 * `commitPreparedSession` 提交 "b" 的新 session 会触发关闭 "a" 的旧 session
 * （"a" 非持久 → `!oldPersistent` 为 true）。断言 `closeSession` 不在锁内调用。
 *
 * #624 评论17 问题3 扩展：`detachWindowBinding` / `commitActiveSession` /
 * `cancelActiveSession` / `releaseHost` 同样不得在 `mutateSession` 锁内调用
 * `coordinator.closeSession` — 必须拆成"锁内读取待关闭 id → 锁外 closeSession →
 * 再进锁校验前提仍成立并提交"。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SessionMutationGateCoreOutsideLockTest {
    private companion object {
        const val TEXT_A = "textA"
    }

    @Test
    fun commitPreparedSession_closeSessionNotCalledInsideMutationLock() {
        var closeCalledInsideLock = false
        val coordinator =
            CoreOutsideLockCoordinator(
                com.xiwei.sujian.core.interop.app.AppServiceBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_core_outside",
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_core_outside",
                    ),
                ),
                onCloseSession = { insideLock, _ -> if (insideLock) closeCalledInsideLock = true },
            )
        // "a"：非持久目标，已激活（Attached），store 里 sessionId=10UL。
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = false)
        coordinator.installExistingPersistentSession("a", 10UL, TEXT_A, 1L)
        coordinator.activateTarget("a", 10UL, 1L)
        // "b"：未激活，store 里 sessionId=0UL（registerTargetMeta 默认）。
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        // commitPreparedSession 提交 "b" 的新 session，触发关闭 "a" 的旧 session
        // （"a" 非持久 → !oldPersistent 为 true）。
        val committed =
            coordinator.commitPreparedSession(
                PreparedSessionHandle(
                    targetId = "b",
                    sessionId = 20UL,
                    snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
                    mode = PreparedSessionMode.Created,
                    previousRecord = null,
                ),
            )
        assertTrue("commitPreparedSession 必须成功", committed)
        assertFalse("closeSession 不得在 mutationLock 内调用（#624 评论17 问题3）", closeCalledInsideLock)
    }

    /**
     * detachWindowBinding 非持久分支：closeSession 必须在锁外调用。
     * 旧实现在 mutateSession { ... coordinator.closeSession(sessionId) ... } 内调用，违反契约。
     */
    @Test
    fun detachWindowBinding_closeSessionNotCalledInsideMutationLock() {
        var closeCalledInsideLock = false
        val coordinator =
            CoreOutsideLockCoordinator(
                com.xiwei.sujian.core.interop.app.AppServiceBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_detach",
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_detach",
                    ),
                ),
                onCloseSession = { insideLock, _ -> if (insideLock) closeCalledInsideLock = true },
            )
        // "a"：非持久目标，已激活（Attached），store 里 sessionId=10UL。
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = false)
        coordinator.installExistingPersistentSession("a", 10UL, TEXT_A, 1L)
        coordinator.activateTarget("a", 10UL, 1L)

        // detachWindowBinding 走非持久分支（!isPersistent → true），触发 closeSession(10)。
        coordinator.detachWindowBinding("w1", "a")

        assertFalse(
            "detachWindowBinding 的 closeSession 不得在 mutationLock 内调用（#624 评论17 问题3）",
            closeCalledInsideLock,
        )
        // 记录被移除。
        assertNull("非持久目标的记录必须被移除", coordinator.store.record("a"))
        // bindingState 变 Idle（sessionState.targetId == targetId 分支）。
        assertEquals(
            "bindingState 必须回到 Idle",
            WindowBindingState.Idle,
            coordinator.sessionState.bindingState,
        )
    }

    /**
     * commitActiveSession 非持久/未绑窗分支：closeSession 必须在锁外调用。
     * 旧实现在 mutateSession { ... coordinator.closeSession(sessionId) ... } 内调用，违反契约。
     */
    @Test
    fun commitActiveSession_closeSessionNotCalledInsideMutationLock() {
        var closeCalledInsideLock = false
        val coordinator =
            CoreOutsideLockCoordinator(
                com.xiwei.sujian.core.interop.app.AppServiceBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_commit",
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_commit",
                    ),
                ),
                onCloseSession = { insideLock, _ -> if (insideLock) closeCalledInsideLock = true },
            )
        // "a"：非持久目标，已激活（Attached），store 里 sessionId=10UL。
        // commitActiveSession 的 !isPersistent 分支为 true → 触发 closeSession(10)。
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = false)
        coordinator.installExistingPersistentSession("a", 10UL, TEXT_A, 1L)
        coordinator.activateTarget("a", 10UL, 1L)

        val committed = coordinator.commitActiveSession(null)

        assertTrue("commitActiveSession 必须成功", committed)
        assertFalse(
            "commitActiveSession 的 closeSession 不得在 mutationLock 内调用（#624 评论17 问题3）",
            closeCalledInsideLock,
        )
        // 记录被移除。
        assertNull("非持久目标的记录必须被移除", coordinator.store.record("a"))
        // sessionState 重置为默认（activeTargetId=null, sessionId=null）。
        assertNull("activeTargetId 必须为 null", coordinator.sessionState.activeTargetId)
        assertNull("sessionId 必须为 null", coordinator.sessionState.sessionId)
    }

    /**
     * cancelActiveSession：closeSession 必须在锁外调用。
     * 旧实现在 mutateSession { ... coordinator.closeSession(sessionId) ... } 内调用，违反契约。
     */
    @Test
    fun cancelActiveSession_closeSessionNotCalledInsideMutationLock() {
        var closeCalledInsideLock = false
        val coordinator =
            CoreOutsideLockCoordinator(
                com.xiwei.sujian.core.interop.app.AppServiceBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_cancel",
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_cancel",
                    ),
                ),
                onCloseSession = { insideLock, _ -> if (insideLock) closeCalledInsideLock = true },
            )
        // "a"：已激活（Attached），store 里 sessionId=10UL。
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession("a", 10UL, TEXT_A, 1L, persistent = true)
        coordinator.activateTarget("a", 10UL, 1L)

        val cancelled = coordinator.cancelActiveSession()

        assertTrue("cancelActiveSession 必须成功", cancelled)
        assertFalse(
            "cancelActiveSession 的 closeSession 不得在 mutationLock 内调用（#624 评论17 问题3）",
            closeCalledInsideLock,
        )
        // 记录被移除。
        assertNull("目标的记录必须被移除", coordinator.store.record("a"))
        // sessionState 重置为默认。
        assertNull("activeTargetId 必须为 null", coordinator.sessionState.activeTargetId)
        assertNull("sessionId 必须为 null", coordinator.sessionState.sessionId)
    }

    /**
     * releaseHost：所有 closeSession 调用都必须在锁外执行。
     * 旧实现在 mutateSession { recordsToClose.forEach { coordinator.closeSession(...) } ... } 内调用，
     * 且 cancelActiveSession 也在锁内调用 closeSession，违反契约。
     */
    @Test
    fun releaseHost_closeSessionNotCalledInsideMutationLock() {
        val closeInsideLockCalls = mutableListOf<ULong>()
        val coordinator =
            CoreOutsideLockCoordinator(
                com.xiwei.sujian.core.interop.app.AppServiceBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_release",
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_release",
                    ),
                ),
                onCloseSession = { insideLock, sid -> if (insideLock) closeInsideLockCalls.add(sid) },
            )
        // "a" 和 "b"：都有 sessionId，激活 "a"。
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession("a", 10UL, TEXT_A, 1L, persistent = true)
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession("b", 20UL, "textB", 1L, persistent = true)
        coordinator.activateTarget("a", 10UL, 1L)

        coordinator.releaseHost()

        assertTrue(
            "releaseHost 的所有 closeSession 不得在 mutationLock 内调用（#624 评论17 问题3），" +
                "违规调用 sessionId 集合：$closeInsideLockCalls",
            closeInsideLockCalls.isEmpty(),
        )
        // clearRecords + sessionState=RELEASED。
        assertEquals("所有记录必须被清空", 0, coordinator.store.allRecords().size)
        assertEquals(
            "editingState 必须为 RELEASED",
            EditingState.RELEASED,
            coordinator.sessionState.editingState,
        )
    }
}

/**
 * 测试用 Coordinator：override `closeSession` 检查是否在 [EditorSessionCoordinator.mutationLock] 内调用。
 * `installExistingPersistentSession` 默认 persistent=false（非持久目标才会被 commitPreparedSession 关闭），
 * 可通过 persistent 参数覆盖。
 * `activateTarget` 设置 bindingState=Attached，使 oldWindowBound=true，确保进入 `!oldPersistent` 分支。
 */
private class CoreOutsideLockCoordinator(
    bridge: com.xiwei.sujian.core.interop.app.AppServiceBridge,
    private val onCloseSession: (insideLock: Boolean, sessionId: ULong) -> Unit,
) : EditorSessionCoordinator(bridge) {
    private val snapshots = mutableMapOf<ULong, TargetSnapshot>()
    private val validSessions = mutableSetOf<ULong>()

    fun installExistingPersistentSession(
        targetId: String,
        sessionId: ULong,
        text: String,
        revision: Long,
        localDirty: Boolean = false,
        persistent: Boolean = false,
    ) {
        validSessions.add(sessionId)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        snapshots[sessionId] = TargetSnapshot(text, cursor, revision, 0, cursor)
        store.put(
            EditorSessionRecord(
                targetId = targetId,
                sessionId = sessionId,
                persistent = persistent,
                documentState =
                    DocumentState(
                        revision = revision,
                        selectionAnchorUtf8 = 0,
                        selectionHeadUtf8 = cursor,
                        localDirty = localDirty,
                    ),
            ),
        )
    }

    fun activateTarget(
        targetId: String,
        sessionId: ULong,
        revision: Long,
    ) {
        val record = store.record(targetId)!!
        _sessionStateFlow.value =
            EditorSessionState(
                targetId = targetId,
                sessionId = sessionId,
                revision = revision,
                activeTargetId = targetId,
                localDirty = record.documentState.localDirty,
                committedVersion = record.documentState.committedVersion,
                sessionBaseVersion = record.documentState.sessionBaseVersion,
                bindingState = WindowBindingState.Attached("w1", targetId, sessionId),
                editingState = EditingState.EDITING,
            )
    }

    internal override fun createSession(
        targetId: String,
        text: String,
        cursorByteOffset: Int,
        isPersistent: Boolean,
    ): ULong? = null

    internal override fun closeSession(sessionId: ULong) {
        onCloseSession(mutationLock.isHeldByCurrentThread, sessionId)
    }

    internal override fun validateSession(sessionId: ULong): Boolean = sessionId != 0UL && sessionId in validSessions

    internal override fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? = snapshots[sessionId]
}
