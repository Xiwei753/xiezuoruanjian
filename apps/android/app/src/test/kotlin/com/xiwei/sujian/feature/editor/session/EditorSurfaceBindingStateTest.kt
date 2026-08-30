@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.ui.EditorSurfaceMode
import com.xiwei.sujian.feature.editor.ui.editorSurfaceMode
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #644 评论 5462826712 第3节：编辑器 Surface 绑定状态守卫契约测试。
 *
 * 旧缺陷：beginEdit 在尚无 Android View 时保存 pendingViewBind 后立即调用
 * completeWindowAttach，WindowBindingState 提前进入 Attached，此时 View、
 * InputConnection、session bridge 都尚未绑定；绑定失败也不会回到 Detached/Idle。
 *
 * 修复：Attached 只能从 Attaching 进入（completeWindowAttach 状态守卫）；
 * 绑定失败/导航取消回到 Detached/Idle。本测试验证状态机守卫的纯逻辑行为。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorSurfaceBindingStateTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        // 测试环境无 native：session 创建返回 NotLoaded → prepareSessionForEdit 返回 null，
        // 状态必须回到 Idle（绝不进入没有 View 的 Attached）。
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595",
                    "/tmp/sujian_test_workspace_595",
                ),
            ),
        )
    }

    @Test
    fun completeWindowAttach_fromIdle_isRejected() {
        val coordinator = createCoordinator()
        coordinator.completeWindowAttach("w1", "t1", 1UL)
        assertEquals(
            "completeWindowAttach must NOT transition from Idle to Attached (#595 三)",
            WindowBindingState.Idle,
            coordinator.windowBindingState,
        )
        assertNull(coordinator.activeTargetId)
    }

    @Test
    fun completeWindowAttach_fromDetached_isRejected() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true).apply { updateText("hello") }
        coordinator.registerTarget(target)
        // 模拟：会话存在但窗口未绑定（Detached）。
        // 无法创建真实 session（无 native），直接用状态机验证守卫：
        // 手动进入 Detached 后 completeWindowAttach 必须被拒绝。
        val detached = WindowBindingState.Detached("t1", 99UL, null)
        setWindowBindingState(coordinator, detached)
        coordinator.completeWindowAttach("w1", "t1", 99UL)
        assertEquals(
            "completeWindowAttach must NOT transition from Detached to Attached",
            detached,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun prepareSessionForEdit_failure_returnsToIdleNotAttached() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true).apply { updateText("hello") }
        coordinator.registerTarget(target)

        val bindInfo = coordinator.prepareSessionForEdit("t1", "hello", 5, "w1")
        assertNull("Session creation must fail without native — bindInfo null", bindInfo)
        assertEquals(
            "Failed binding must leave window state Idle, never Attached without a View",
            WindowBindingState.Idle,
            coordinator.windowBindingState,
        )
    }

    @Test
    fun shouldShowEditor_requiresAttachingOrAttached() {
        // 渲染决策：只有 Attaching/Attached（及收尾状态）显示编辑器；
        // Idle/Detaching/Detached 一律显示预览。
        val targetId = "t1"
        assertTrue(
            editorSurfaceMode(
                WindowBindingState.Attaching("w", targetId, 1UL),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertTrue(
            editorSurfaceMode(
                WindowBindingState.Attached("w", targetId, 1UL),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertFalse(
            editorSurfaceMode(
                WindowBindingState.Idle,
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertFalse(
            editorSurfaceMode(
                WindowBindingState.Detaching(null),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
        assertFalse(
            editorSurfaceMode(
                WindowBindingState.Detached(targetId, 1UL, null),
                "w",
                targetId,
                isActivePane = false,
            ) == EditorSurfaceMode.EditorHost,
        )
    }

    @Test
    fun beginEdit_failure_neverLeavesAttachedState() {
        val coordinator = createCoordinator()
        val target = EditableTextTarget("t1", isPersistent = true).apply { updateText("hello") }
        coordinator.registerTarget(target)
        coordinator.prepareSessionForEdit("t1", "hello", 5, "w1")
        // 无 native：session 创建失败 → 状态必须保持 Idle。
        // （回归保护：旧实现即使 session 创建失败也会在 beginEdit 里提前
        //   completeWindowAttach 进入 Attached。）
        assertEquals(WindowBindingState.Idle, coordinator.windowBindingState)
    }

    private fun setWindowBindingState(
        coordinator: EditorSessionCoordinator,
        state: WindowBindingState,
    ) {
        // #595 三：_windowBindingStateFlow 已改为从 _sessionStateFlow 派生，
        // 通过 _sessionStateFlow.copy(bindingState = state) 设置。
        val field = EditorSessionCoordinator::class.java.getDeclaredField("_sessionStateFlow")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val flow = field.get(coordinator) as kotlinx.coroutines.flow.MutableStateFlow<EditorSessionState>
        flow.value = flow.value.copy(bindingState = state)
    }
}
