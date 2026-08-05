package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 二/三：窗口解绑与业务关闭分离契约测试。
 *
 * 验证 EditorSessionCoordinator 提供 detachWindowBinding(windowId, targetId)，
 * Compose onDispose 只调用它解除窗口绑定；业务关闭走 closeTarget(targetId, reason)，
 * 两者分离，配置变化不关闭持久 Rust session。
 */
class EditorSessionDetachContractTest {

    @Test
    fun detachWindowBinding_existsOnEditorSessionCoordinator() {
        val method: Method? = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "detachWindowBinding" &&
            it.parameterTypes.size == 2 &&
            it.parameterTypes[0] == String::class.java &&
            it.parameterTypes[1] == String::class.java
        }
        assertTrue(
            "EditorSessionCoordinator must have detachWindowBinding(String windowId, String targetId) " +
            "for config-change survival",
            method != null
        )
    }

    @Test
    fun closeTarget_existsWithReason() {
        val method: Method? = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "closeTarget" &&
            it.parameterTypes.size == 2 &&
            it.parameterTypes[0] == String::class.java &&
            it.parameterTypes[1] == SessionCloseReason::class.java
        }
        assertTrue(
            "EditorSessionCoordinator must have closeTarget(String targetId, SessionCloseReason reason) " +
            "for business-level closes (workspace navigation, chapter switch, delete)",
            method != null
        )
    }

    @Test
    fun windowBindingState_hasAllMachineStates() {
        val states: List<WindowBindingState> = listOf(
            WindowBindingState.Idle,
            WindowBindingState.Attached("w", "t", 1UL),
            WindowBindingState.Detaching(null),
            WindowBindingState.Detached("t", 1UL, null),
            WindowBindingState.Attaching("w", "t", 1UL),
            WindowBindingState.Committing("t", 1UL),
            WindowBindingState.Cancelling("t", 1UL),
        )
        assertTrue(states.size == 7)
    }

    @Test
    fun sessionCloseReason_hasBusinessReasons() {
        assertTrue(SessionCloseReason.entries.size == 3)
        assertTrue(SessionCloseReason.entries.contains(SessionCloseReason.WORKSPACE_NAVIGATION))
        assertTrue(SessionCloseReason.entries.contains(SessionCloseReason.CHAPTER_SWITCH))
        assertTrue(SessionCloseReason.entries.contains(SessionCloseReason.DELETE))
    }

    @Test
    fun editorWindowHost_delegatesDetachWindowBindingAndCloseTarget() {
        val detach = EditorWindowHost::class.java.methods.any {
            it.name == "detachWindowBinding" && it.parameterTypes.size == 2
        }
        assertTrue("EditorWindowHost must delegate detachWindowBinding(windowId, targetId)", detach)
        val close = EditorWindowHost::class.java.methods.any {
            it.name == "closeTarget" && it.parameterTypes.size == 2
        }
        assertTrue("EditorWindowHost must delegate closeTarget(targetId, reason)", close)
    }

    @Test
    fun editorWindowHost_exposesWindowId() {
        val windowIdMethod = EditorWindowHost::class.java.methods.firstOrNull { it.name == "getWindowId" }
        assertTrue("EditorWindowHost must expose windowId for detachWindowBinding", windowIdMethod != null)
    }
}
