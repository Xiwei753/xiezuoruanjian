@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #623 评论5/6：窗口重建后绑定身份完整性契约测试。
 *
 * 配置变化/Activity 重建后新 EditorWindowHost 有新的 windowId，旧窗口的
 * Attached/Attaching 残留会让新窗口误判"已附着"而跳过 beginEdit。规则：
 * 1. prepareSessionForEdit 复用活动 session 时，把属于其他窗口的绑定
 *    重贴为当前窗口的 Attaching（同一 targetId + sessionId 才生效）——
 *    这是窗口接管旧绑定的唯一动作；
 * 2. completeWindowAttach 不再参与所有权接管，只允许两种结果：
 *    当前是精确相同的 Attached(windowId,targetId,sessionId) → 幂等 return；
 *    当前是精确相同的 Attaching(windowId,targetId,sessionId) → 推进为 Attached；
 *    windowId/targetId/sessionId 任一不一致（包括旧窗口晚到的 completion、
 *    错误 session 的 completion）都直接忽略，不能修改状态；
 * 3. 不同 target/session 的完成请求不得覆盖现有绑定。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class WindowRebindIdentityTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_623_rebind",
                    "/tmp/sujian_test_workspace_623_rebind",
                ),
            ),
        )
    }

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
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
    fun prepareSessionForEdit_keepsAttachedFromSameWindow() {
        val coordinator = createCoordinator()
        registerPersistentTarget(coordinator, "t1", 42UL)
        val attached = WindowBindingState.Attached("w2", "t1", 42UL)
        setWindowBindingState(coordinator, attached)

        coordinator.prepareSessionForEdit("t1", "hello", 5, "w2")

        assertEquals(
            "same-window Attached must not be re-stamped",
            attached,
            coordinator.windowBindingState,
        )
    }

    // ── completeWindowAttach 身份（#623 评论6：completion 不参与所有权接管）──

    @Test
    fun completeWindowAttach_foreignWindowCompletion_doesNotRestampAttached() {
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
        val coordinator = createCoordinator()
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
}
