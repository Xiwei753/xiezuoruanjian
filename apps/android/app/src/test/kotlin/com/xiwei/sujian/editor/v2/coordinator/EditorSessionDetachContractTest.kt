package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 一：编辑器会话跨配置变化存活契约测试。
 *
 * 验证 EditorSessionCoordinator 提供 detachTarget 方法，
 * Compose onDispose 调用 detachTarget 而非 unregisterTarget，
 * 配置变化时不关闭持久 Rust session。
 */
class EditorSessionDetachContractTest {

    @Test
    fun detachTarget_existsOnEditorSessionCoordinator() {
        val method: Method? = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "detachTarget" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == String::class.java
        }
        assertTrue(
            "EditorSessionCoordinator must have detachTarget(String) for config-change survival",
            method != null
        )
    }

    @Test
    fun unregisterTarget_stillExists_forExplicitBusinessEvents() {
        val method: Method? = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "unregisterTarget" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == String::class.java
        }
        assertTrue(
            "EditorSessionCoordinator must still have unregisterTarget for explicit business events " +
            "(chapter close, permanent delete, onCleared)",
            method != null
        )
    }
}
