@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #623 评论5/6：窗口重建后绑定身份完整性契约测试。
 *
 * 配置变化/Activity 重建后新 EditorWindowHost 有新的 windowId，旧窗口的
 * Attached/Attaching 拖留会让新窗口误判"已附着"而跳过 beginEdit。规则：
 * 1. prepareSessionForEdit 复用活动 session 时，把属于其他窗口的绑定
 *    重贴为当前窗口的 Attaching（同一 targetId + sessionId 才生效）——
 *    这是窗口接管旧绑定的唯一动作；
 * 2. completeWindowAttach 不再参与所有权接管，只允许两种结果：
 *    当前是精确相同的 Attached(windowId,targetId,sessionId) → 幂等 return；
 *    当前是精确相同的 Attaching(windowId,targetId,sessionId) → 推进为 Attached；
 *    windowId/targetId/sessionId 任一不一致（包括旧窗口晚到的 completion、
 *    错误 session 的 completion）都直接忽略，不能修改状态；
 * 3. 不同 target/session 的完成请求不得覆盖现有绑定。
 * #623 评论8：跨窗口 restamp（窗口接管旧绑定的唯一动作）必须同时使旧窗口的
 * input lease 失效（epoch+1）— 旧 w1 View 晚到的 IME 回调被拒绝、晚到的
 * detachWindowBinding 不再二次递增 epoch，新 w2 的 performViewBind 签发新 lease。
 *
 * #624 评论5294575627 要求2：删除 fast path 后，restamp 通过 precondition CAS
 * （commitPreparedBindingState）完成 — prepareSessionForEdit 走完整 resolve →
 * querySnapshot → CAS 路径，不再有 prepareActiveSessionIfCurrent + restampAttachingToWindow
 * 快捷路径。测试用 [RebindTestCoordinator] mock native session（validateSession=true、
 * querySnapshotForSession 返回有效 snapshot）使 CAS 路径在测试环境完整执行。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class WindowRebindIdentityTest {
    /**
     * #624 评论5294575627 要求2：fast path 删除后 prepareSessionForEdit 走完整 precondition CAS 路径，
     * 需要 validateSession/querySnapshotForSession 返回有效值。RebindTestCoordinator mock native session
     * 使 CAS 路径可在测试环境完整执行（验证 restamp 通过 commitPreparedBindingState CAS 完成）。
     */
    private class RebindTestCoordinator(
        private val validSessionId: ULong,
        private val snapshotText: String,
    ) : EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_rebind",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_rebind",
                ),
            ),
        ) {
        private val snapshot = TargetSnapshot(snapshotText, 5, 1L, 0, 5)

        internal override fun validateSession(sessionId: ULong): Boolean = sessionId == validSessionId

        internal override fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? =
            if (sessionId == validSessionId) snapshot else null

        internal override fun createSession(
            targetId: String,
            text: String,
            cursorByteOffset: Int,
            isPersistent: Boolean,
        ): ULong? = null

        internal override fun closeSession(sessionId: ULong) {
            // no-op — 测试环境无 native
        }
    }

    private fun createRebindCoordinator(
        sessionId: ULong = 42UL,
        text: String = "hello",
    ): RebindTestCoordinator = RebindTestCoordinator(sessionId, text)

    private fun setWindowBindingState(
        coordinator: EditorSessionCoordinator,
        state: WindowBindingState,
        editingState: EditingState = EditingState.EDITING,
    ) {
        val field = EditorSessionCoordinator::class.java.getDeclaredField("_sessionStateFlow")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val flow = field.get(coordinator) as kotlinx.coroutines.flow.MutableStateFlow<EditorSessionState>
        val targetId =
            when (state) {
                is WindowBindingState.Attached -> state.targetId
                is WindowBindingState.Attaching -> state.targetId
                is WindowBindingState.Detached -> state.targetId
                else -> null
            }
        val sessionId =
            when (state) {
                is WindowBindingState.Attached -> state.sessionId
                is WindowBindingState.Attaching -> state.sessionId
                is WindowBindingState.Detached -> state.sessionId
                else -> null
            }
        flow.value =
            flow.value.copy(
                bindingState = state,
                targetId = targetId,
                activeTargetId = targetId,
                sessionId = sessionId,
                editingState = editingState,
            )
    }

    private fun registerPersistentTarget(
        coordinator: EditorSessionCoordinator,
        targetId: String,
        sessionId: ULong,
    ) {
        val target = EditableTextTarget(targetId, isPersistent = true).apply { updateText("hello") }
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = targetId, sessionId = sessionId, persistent = true),
        )
    }

    // ── prepareSessionForEdit 重贴 ──

    @Test
    fun prepareSessionForEdit_restampsAttachedFromForeignWindow() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        setWindowBindingState(coordinator, WindowBindingState.Attached("w1", "t1", 42UL))

        val bind = coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")

        assertTrue(
            "reuse of active session must return bind info",
            bind != null,
        )
        val binding = coordinator.windowBindingState
        assertTrue(
            "stale Attached from another window must be re-stamped to the current window's Attaching, got $binding",
            binding is WindowBindingState.Attaching,
        )
        assertEquals("w2", (binding as WindowBindingState.Attaching).windowId)
        assertEquals("t1", binding.targetId)
        assertEquals(42UL, binding.sessionId)
        assertEquals(
            "re-stamp must move editing state back to BINDING until the new view binds",
            EditingState.BINDING,
            coordinator.editingState,
        )
    }

    @Test
    fun prepareSessionForEdit_restampsPreparedAttachingFromForeignWindow() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        // 章节切换 commitPreparedSession 的 "prepared" 预绑定
        setWindowBindingState(
            coordinator,
            WindowBindingState.Attaching("prepared", "t1", 42UL),
            editingState = EditingState.BINDING,
        )

        coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")

        val binding = coordinator.windowBindingState
        assertTrue(
            "prepared pre-binding must be re-stamped to the current window's Attaching, got $binding",
            binding is WindowBindingState.Attaching,
        )
        assertEquals("w2", (binding as WindowBindingState.Attaching).windowId)
    }

    @Test
    fun prepareSessionForEdit_keepsAttachingFromSameWindow() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        val attaching = WindowBindingState.Attaching("w2", "t1", 42UL)
        setWindowBindingState(coordinator, attaching, editingState = EditingState.BINDING)

        coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")

        assertEquals(
            "same-window Attaching must not be re-stamped",
            attaching,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun prepareSessionForEdit_rebindsAttachedFromSameWindow() {
        // #624 评论5294575627 要求2：删除 fast path 后 prepareSessionForEdit 总是走 precondition CAS，
        // 即使当前已是同窗口 Attached 也会重写为 Attaching + BINDING（等待 completeWindowAttach 重新推进）。
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        setWindowBindingState(coordinator, WindowBindingState.Attached("w2", "t1", 42UL))

        coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")

        val binding = coordinator.windowBindingState
        assertEquals(
            "same-window Attached is re-prepared as Attaching via CAS (fast path deleted)",
            WindowBindingState.Attaching("w2", "t1", 42UL),
            binding,
        )
        assertEquals(
            "editing state moves back to BINDING until the view re-completes attach",
            EditingState.BINDING,
            coordinator.editingState,
        )
    }

    // ── completeWindowAttach 身份（#623 评论6：completion 不参与所有权接管）──

    @Test
    fun completeWindowAttach_foreignWindowCompletion_doesNotRestampAttached() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        // 新窗口 w2 的真实 View 已绑定完成
        setWindowBindingState(coordinator, WindowBindingState.Attached("w2", "t1", 42UL))

        // 旧窗口 w1 晚到一次 completion — 不得把已经属于 w2 的 Attached 抢回 w1。
        coordinator.completeWindowAttach("w1", "t1", 42UL)

        assertEquals(
            "old window's late completion must not re-stamp the new window's Attached",
            WindowBindingState.Attached("w2", "t1", 42UL),
            coordinator.windowBindingState,
        )
        assertEquals(EditingState.EDITING, coordinator.editingState)
    }

    @Test
    fun completeWindowAttach_foreignWindowCompletion_keepsAttaching() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        // 新窗口 w2 已进入 Attaching（真实 View 尚未完成绑定）
        val attaching = WindowBindingState.Attaching("w2", "t1", 42UL)
        setWindowBindingState(coordinator, attaching, editingState = EditingState.BINDING)

        // 旧窗口 w1 晚到 completion — 不得把 Attaching(w2) 推进或改写。
        coordinator.completeWindowAttach("w1", "t1", 42UL)

        assertEquals(
            "old window's late completion must leave the new window's Attaching untouched",
            attaching,
            coordinator.windowBindingState,
        )
        assertEquals(EditingState.BINDING, coordinator.editingState)
    }

    @Test
    fun completeWindowAttach_wrongSessionCompletion_keepsAttaching() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        val attaching = WindowBindingState.Attaching("w2", "t1", 42UL)
        setWindowBindingState(coordinator, attaching, editingState = EditingState.BINDING)

        // 错误 session 的 completion（同窗口、不同 sessionId）— 必须保持原 Attaching。
        coordinator.completeWindowAttach("w2", "t1", 99UL)

        assertEquals(
            "completion with wrong sessionId must leave Attaching untouched",
            attaching,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun completeWindowAttach_exactAttaching_advancesToAttached() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        setWindowBindingState(
            coordinator,
            WindowBindingState.Attaching("w2", "t1", 42UL),
            editingState = EditingState.BINDING,
        )

        // 当前窗口自己的真实 View bind 完成 — Attaching(w2) → Attached(w2)。
        coordinator.completeWindowAttach("w2", "t1", 42UL)

        val binding = coordinator.windowBindingState
        assertEquals(WindowBindingState.Attached("w2", "t1", 42UL), binding)
        assertEquals(EditingState.EDITING, coordinator.editingState)
    }

    @Test
    fun completeWindowAttach_sameWindowSameSession_isIdempotentNoOp() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        val attached = WindowBindingState.Attached("w1", "t1", 42UL)
        setWindowBindingState(coordinator, attached)

        coordinator.completeWindowAttach("w1", "t1", 42UL)

        assertEquals(
            "same windowId+targetId+sessionId must keep the existing Attached unchanged",
            attached,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun completeWindowAttach_differentSession_isIgnored() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        val attached = WindowBindingState.Attached("w1", "t1", 42UL)
        setWindowBindingState(coordinator, attached)

        coordinator.completeWindowAttach("w2", "t1", 99UL)

        assertEquals(
            "completeWindowAttach for a different session must not clobber the existing binding",
            attached,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun completeWindowAttach_differentTarget_isIgnored() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        val attached = WindowBindingState.Attached("w1", "t1", 42UL)
        setWindowBindingState(coordinator, attached)

        coordinator.completeWindowAttach("w1", "t2", 42UL)

        assertEquals(
            "completeWindowAttach for a different target must not clobber the existing binding",
            attached,
            coordinator.windowBindingState,
        )
    }

    // ── #623 评论8：跨窗口 restamp 时 input lease 随窗口所有权一起转移 ──

    @Test
    fun restampToNewWindow_invalidatesOldInputLeaseExactlyOnce() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        setWindowBindingState(coordinator, WindowBindingState.Attached("w1", "t1", 42UL))

        val oldLease = coordinator.currentInputLease()
        assertNotNull("old window must hold a lease before restamp", oldLease)
        val oldEpoch = coordinator.inputLeaseEpoch
        assertTrue("old lease must be current before restamp", coordinator.isInputLeaseCurrent(oldLease))

        coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")

        assertEquals(
            "cross-window restamp must bump inputLeaseEpoch exactly once",
            oldEpoch + 1,
            coordinator.inputLeaseEpoch,
        )
        assertFalse(
            "old window's lease must be rejected after restamp",
            coordinator.isInputLeaseCurrent(oldLease),
        )
        val newLease = coordinator.currentInputLease()
        assertNotNull("new window must hold a lease after restamp", newLease)
        assertEquals(
            "new lease must carry the bumped epoch",
            oldEpoch + 1,
            newLease!!.epoch,
        )
        assertTrue(
            "new lease must be current",
            coordinator.isInputLeaseCurrent(newLease),
        )
    }

    @Test
    fun restampToNewWindow_lateOldWindowDetach_doesNotBumpEpochAgain() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        setWindowBindingState(coordinator, WindowBindingState.Attached("w1", "t1", 42UL))
        val oldEpoch = coordinator.inputLeaseEpoch

        coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")
        assertEquals(oldEpoch + 1, coordinator.inputLeaseEpoch)

        // 旧 w1 View 晚到的 detachWindowBinding — 绑定已属于 w2，windowId 不匹配
        // 直接 no-op，不得再次递增 epoch（旧 lease 已经因 restamp 失效）。
        coordinator.detachWindowBinding("w1", "t1")

        assertEquals(
            "old window's late detach must not bump epoch a second time",
            oldEpoch + 1,
            coordinator.inputLeaseEpoch,
        )
        assertEquals(
            "binding must still belong to the new window",
            WindowBindingState.Attaching("w2", "t1", 42UL),
            coordinator.windowBindingState,
        )
    }

    @Test
    fun restampFromPreparedAttaching_issuesNewLeaseForRealWindow() {
        val coordinator = createRebindCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        // 章节切换 commitPreparedSession 的 "prepared" 预绑定
        setWindowBindingState(
            coordinator,
            WindowBindingState.Attaching("prepared", "t1", 42UL),
            editingState = EditingState.BINDING,
        )
        val preparedLease = coordinator.currentInputLease()
        assertNotNull("prepared stage must hold a lease", preparedLease)
        val oldEpoch = coordinator.inputLeaseEpoch

        // 真实窗口 w2 接管预绑定时重新签发自己的 lease，预绑定阶段的 lease 不得沿用。
        coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")

        assertEquals(
            "real window taking over the prepared binding must bump epoch exactly once",
            oldEpoch + 1,
            coordinator.inputLeaseEpoch,
        )
        assertFalse(
            "prepared-stage lease must not survive the handover",
            coordinator.isInputLeaseCurrent(preparedLease),
        )
        val realLease = coordinator.currentInputLease()
        assertNotNull("real window must hold a fresh lease", realLease)
        assertEquals(
            "real window's lease must carry the bumped epoch",
            oldEpoch + 1,
            realLease!!.epoch,
        )
        assertTrue(
            "real window's lease must be current",
            coordinator.isInputLeaseCurrent(realLease),
        )
        assertEquals(
            "binding must now belong to the real window",
            WindowBindingState.Attaching("w2", "t1", 42UL),
            coordinator.windowBindingState,
        )
    }
}
