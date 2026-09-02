@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #623 评论 2：detachWindowBinding windowId+targetId 守卫契约测试。
 *
 * 当前状态是 Attaching/Attached 时，只有 windowId + targetId 都与要释放的 View
 * 对得上才允许进入 Detached。旧 View 的 release 不能把新绑定拆掉。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DetachWindowBindingWindowGuardTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_623",
                    "/tmp/sujian_test_workspace_623",
                ),
            ),
        )
    }

    private fun setWindowBindingState(
        coordinator: EditorSessionCoordinator,
        state: WindowBindingState,
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
            )
    }

    @Test
    fun detachWindowBinding_withMatchingWindowId_transitionsToDetached() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        setWindowBindingState(coordinator, WindowBindingState.Attached("w1", "t1", 42UL))

        coordinator.detachWindowBinding("w1", "t1")

        val binding = coordinator.windowBindingState
        assertTrue(
            "Matching windowId+targetId should transition to Detached, got $binding",
            binding is WindowBindingState.Detached,
        )
        assertEquals("t1", (binding as WindowBindingState.Detached).targetId)
        assertEquals(42UL, binding.sessionId)
    }

    @Test
    fun detachWindowBinding_withDifferentWindowId_isIgnored() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        val attached = WindowBindingState.Attached("w1", "t1", 42UL)
        setWindowBindingState(coordinator, attached)

        coordinator.detachWindowBinding("w2", "t1")

        assertEquals(
            "detachWindowBinding with different windowId must not tear down the current binding",
            attached,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun detachWindowBinding_withDifferentTargetId_isIgnored() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        val attached = WindowBindingState.Attached("w1", "t1", 42UL)
        setWindowBindingState(coordinator, attached)

        coordinator.detachWindowBinding("w1", "t2")

        assertEquals(
            "detachWindowBinding with different targetId must not tear down the current binding",
            attached,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun detachWindowBinding_fromAttaching_withDifferentWindowId_isIgnored() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        val attaching = WindowBindingState.Attaching("w1", "t1", 42UL)
        setWindowBindingState(coordinator, attaching)

        coordinator.detachWindowBinding("w2", "t1")

        assertEquals(
            "detachWindowBinding from Attaching with different windowId must be ignored",
            attaching,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun detachWindowBinding_withDifferentWindowId_doesNotInvalidateInputLease() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        setWindowBindingState(coordinator, WindowBindingState.Attached("w1", "t1", 42UL))
        val epochBefore = coordinator.inputLeaseEpoch

        coordinator.detachWindowBinding("w2", "t1")

        assertEquals(
            "detachWindowBinding with different windowId must not invalidate input lease " +
                "(old View's late release must not break the new binding's lease)",
            epochBefore,
            coordinator.inputLeaseEpoch,
        )
    }

    @Test
    fun detachWindowBinding_withDifferentTargetId_doesNotInvalidateInputLease() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        setWindowBindingState(coordinator, WindowBindingState.Attached("w1", "t1", 42UL))
        val epochBefore = coordinator.inputLeaseEpoch

        coordinator.detachWindowBinding("w1", "t2")

        assertEquals(
            "detachWindowBinding with different targetId must not invalidate input lease",
            epochBefore,
            coordinator.inputLeaseEpoch,
        )
    }

    @Test
    fun detachWindowBinding_withMatchingWindowId_invalidatesInputLease() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        setWindowBindingState(coordinator, WindowBindingState.Attached("w1", "t1", 42UL))
        val epochBefore = coordinator.inputLeaseEpoch

        coordinator.detachWindowBinding("w1", "t1")

        assertTrue(
            "detachWindowBinding with matching windowId+targetId must invalidate input lease",
            coordinator.inputLeaseEpoch > epochBefore,
        )
    }

    @Test
    fun detachWindowBinding_fromDetached_withSameTargetId_isIdempotentAndDoesNotInvalidateLease() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        setWindowBindingState(coordinator, WindowBindingState.Detached("t1", 42UL, null))
        val epochBefore = coordinator.inputLeaseEpoch

        coordinator.detachWindowBinding("w1", "t1")

        val binding = coordinator.windowBindingState
        assertTrue(
            "detachWindowBinding from Detached with same targetId must stay Detached, got $binding",
            binding is WindowBindingState.Detached,
        )
        assertEquals(
            "detachWindowBinding from Detached with same targetId must not invalidate input lease " +
                "(idempotent no-op, old View late release must not redundantly bump epoch)",
            epochBefore,
            coordinator.inputLeaseEpoch,
        )
    }

    @Test
    fun detachWindowBinding_fromDetached_withDifferentTargetId_continuesDraftCleanup() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true)
        coordinator.registerTarget(target)
        coordinator.store.put(
            EditorSessionRecord(targetId = "t1", sessionId = 42UL, persistent = true),
        )
        setWindowBindingState(coordinator, WindowBindingState.Detached("t1", 42UL, null))
        val epochBefore = coordinator.inputLeaseEpoch

        // 不同 targetId 的解绑不被新 Detached(targetId) 守卫拦截 — 草稿清理路径仍继续，
        // 仍会失效 lease（t2 的解绑是独立事件，不是 t1 的重复 detach）。
        coordinator.detachWindowBinding("w1", "t2")

        val binding = coordinator.windowBindingState
        assertTrue(
            "detachWindowBinding with different targetId must not alter t1's Detached state, got $binding",
            binding is WindowBindingState.Detached,
        )
        assertEquals("t1", (binding as WindowBindingState.Detached).targetId)
        assertTrue(
            "detachWindowBinding with different targetId must still invalidate lease " +
                "(draft cleanup continues, not blocked by Detached idempotency guard)",
            coordinator.inputLeaseEpoch > epochBefore,
        )
    }
}
