package com.xiwei.sujian.editor.v2.coordinator

import kotlinx.coroutines.flow.StateFlow
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二：会话层不持有 Compose mutableState 契约测试。
 *
 * Issue #595 要求会话层（EditorSessionCoordinator）只保留 Core session 与纯数据
 * snapshot，不持有 View、Context、Rect、TextPaint、FrameClock、mutableState 或
 * 窗口回调。本测试验证五个可观察状态属性已从 Compose mutableStateOf 迁移到
 * StateFlow，窗口层和 Compose 消费者通过 collectAsState() 观察。
 */
class SessionLayerNoMutableStateContractTest {

    private val flowGetters = listOf(
        "getActiveTargetIdFlow",
        "getEditingStateFlow",
        "getWindowBindingStateFlow",
        "getTargetDecorationsVersionFlow",
        "getLastCommittedTextFlow",
    )

    @Test
    fun sessionCoordinator_exposesStateFlowsForObservableState() {
        val methods = EditorSessionCoordinator::class.java.methods

        for (name in flowGetters) {
            val method = methods.firstOrNull { it.name == name }
            assertNotNull(
                "EditorSessionCoordinator must expose $name as StateFlow after #595 二",
                method
            )
            method?.let {
                assertTrue(
                    "$name must return StateFlow, got ${it.returnType.name}",
                    StateFlow::class.java.isAssignableFrom(it.returnType)
                )
            }
        }
    }

    @Test
    fun sessionCoordinator_doesNotUseComposeMutableStateFields() {
        val composeMutableStateTypes = setOf(
            "androidx.compose.runtime.MutableState",
            "androidx.compose.runtime.MutableIntState",
            "androidx.compose.runtime.MutableLongState",
            "androidx.compose.runtime.MutableFloatState",
            "androidx.compose.runtime.MutableDoubleState",
        )

        val offendingFields = EditorSessionCoordinator::class.java.declaredFields.filter { field ->
            composeMutableStateTypes.any { it == field.type.name }
        }

        assertTrue(
            "EditorSessionCoordinator must not hold Compose mutableState fields after #595 二. " +
            "Found: ${offendingFields.map { it.name + ": " + it.type.name }}",
            offendingFields.isEmpty()
        )
    }

    @Test
    fun editorWindowHost_delegatesStateFlowsFromSession() {
        val methods = EditorWindowHost::class.java.methods

        for (name in flowGetters) {
            val method = methods.firstOrNull { it.name == name }
            assertNotNull(
                "EditorWindowHost must delegate $name from session layer after #595 二",
                method
            )
            method?.let {
                assertTrue(
                    "EditorWindowHost.$name must return StateFlow, got ${it.returnType.name}",
                    StateFlow::class.java.isAssignableFrom(it.returnType)
                )
            }
        }
    }
}
