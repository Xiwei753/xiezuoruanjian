package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertFalse
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
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SessionMutationGateCoreOutsideLockTest {
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
                onCloseSession = { insideLock -> if (insideLock) closeCalledInsideLock = true },
            )
        // "a"：非持久目标，已激活（Attached），store 里 sessionId=10UL。
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = false)
        coordinator.installExistingPersistentSession("a", 10UL, "textA", 1L)
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
}

/**
 * 测试用 Coordinator：override `closeSession` 检查是否在 [EditorSessionCoordinator.mutationLock] 内调用。
 * `installExistingPersistentSession` 设 persistent=false（非持久目标才会被 commitPreparedSession 关闭）。
 * `activateTarget` 设置 bindingState=Attached，使 oldWindowBound=true，确保进入 `!oldPersistent` 分支。
 */
private class CoreOutsideLockCoordinator(
    bridge: com.xiwei.sujian.core.interop.app.AppServiceBridge,
    private val onCloseSession: (insideLock: Boolean) -> Unit,
) : EditorSessionCoordinator(bridge) {
    private val snapshots = mutableMapOf<ULong, TargetSnapshot>()
    private val validSessions = mutableSetOf<ULong>()

    fun installExistingPersistentSession(
        targetId: String,
        sessionId: ULong,
        text: String,
        revision: Long,
        localDirty: Boolean = false,
    ) {
        validSessions.add(sessionId)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        snapshots[sessionId] = TargetSnapshot(text, cursor, revision, 0, cursor)
        store.put(
            EditorSessionRecord(
                targetId = targetId,
                sessionId = sessionId,
                persistent = false,
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
        onCloseSession(mutationLock.isHeldByCurrentThread)
    }

    internal override fun validateSession(sessionId: ULong): Boolean = sessionId != 0UL && sessionId in validSessions

    internal override fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? = snapshots[sessionId]
}
