package com.xiwei.sujian.editor.v2.coordinator

import kotlinx.coroutines.flow.StateFlow
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二/三：会话层不持有 Compose mutableState 契约测试。
 *
 * Issue #595 要求会话层（EditorSessionCoordinator）只保留 Core session 与纯数据
 * snapshot，不持有 View、Context、Rect、TextPaint、FrameClock、mutableState 或
 * 窗口回调。本测试验证：
 *
 * - 可观察状态通过唯一 [EditorSessionState] 快照 StateFlow 暴露（sessionStateFlow）；
 * - 三个独立 stateIn 派生流（activeTargetIdFlow / editingStateFlow /
 *   windowBindingStateFlow）已删除 — 窗口层和 Compose 只收集 sessionStateFlow，
 *   从同一个快照读取 activeTargetId / editingState / bindingState / sessionId；
 * - 非 Compose 调用方通过 value getter 读取当前值；
 * - 会话层不持有 Compose mutableStateOf 字段。
 */
class SessionLayerNoMutableStateContractTest {

    private val valueGetters = listOf(
        "getActiveTargetId",
        "getEditingState",
        "getWindowBindingState",
    )

    private val removedDerivedFlowGetters = listOf(
        "getActiveTargetIdFlow",
        "getEditingStateFlow",
        "getWindowBindingStateFlow",
    )

    @Test
    fun sessionCoordinator_exposesSingleSessionStateFlow() {
        val methods = EditorSessionCoordinator::class.java.methods

        val sessionStateFlowMethod = methods.firstOrNull { it.name == "getSessionStateFlow" }
        assertNotNull(
            "EditorSessionCoordinator must expose sessionStateFlow as the single state StateFlow",
            sessionStateFlowMethod
        )
        assertTrue(
            "sessionStateFlow must return StateFlow, got ${sessionStateFlowMethod!!.returnType.name}",
            StateFlow::class.java.isAssignableFrom(sessionStateFlowMethod.returnType)
        )
    }

    @Test
    fun sessionCoordinator_derivedFlowsAreRemoved() {
        val methods = EditorSessionCoordinator::class.java.methods.map { it.name }.toSet()

        for (name in removedDerivedFlowGetters) {
            assertNull(
                "Derived stateIn flow $name must be removed — Compose reads the single sessionStateFlow snapshot (#595 三)",
                methods.firstOrNull { it == name },
            )
        }
    }

    @Test
    fun sessionCoordinator_exposesValueGettersForNonComposeCallers() {
        val methods = EditorSessionCoordinator::class.java.methods.map { it.name }.toSet()

        for (name in valueGetters) {
            assertNotNull(
                "EditorSessionCoordinator must expose $name value getter reading sessionStateFlow.value",
                methods.firstOrNull { it == name },
            )
        }
    }

    @Test
    fun sessionCoordinator_hasNoReduceScopeCoroutineScope() {
        val field = EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
            it.name == "reduceScope"
        }
        assertNull(
            "reduceScope (derived stateIn collector scope) must be removed with the derived flows (#595 三)",
            field,
        )
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
    fun editorWindowHost_delegatesSingleSessionStateFlowFromSession() {
        val methods = EditorWindowHost::class.java.methods

        val sessionStateFlowMethod = methods.firstOrNull { it.name == "getSessionStateFlow" }
        assertNotNull(
            "EditorWindowHost must delegate sessionStateFlow from session layer",
            sessionStateFlowMethod
        )
        assertTrue(
            "EditorWindowHost.sessionStateFlow must return StateFlow, got ${sessionStateFlowMethod!!.returnType.name}",
            StateFlow::class.java.isAssignableFrom(sessionStateFlowMethod.returnType)
        )

        val methodNames = methods.map { it.name }.toSet()
        for (name in removedDerivedFlowGetters) {
            assertNull(
                "EditorWindowHost derived flow delegation $name must be removed (#595 三)",
                methodNames.firstOrNull { it == name },
            )
        }
        for (name in valueGetters) {
            assertNotNull(
                "EditorWindowHost must expose $name value getter",
                methodNames.firstOrNull { it == name },
            )
        }
    }
}
