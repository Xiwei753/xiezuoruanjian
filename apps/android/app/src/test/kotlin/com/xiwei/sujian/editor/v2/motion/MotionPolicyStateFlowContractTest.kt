package com.xiwei.sujian.editor.v2.motion

import com.xiwei.sujian.editor.v2.coordinator.EditorAnimationSettings
import com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import kotlinx.coroutines.flow.StateFlow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 三：EditorMotionPolicy StateFlow 契约测试 — 验证动画策略通过只读 StateFlow
 * 暴露为单一可观察事实源，applyMotionPolicy 原子更新，reduceMotion 从设置接入。
 */
class MotionPolicyStateFlowContractTest {

    @Test
    fun sessionCoordinator_exposesMotionPolicyFlow() {
        val field = EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
            it.name == "motionPolicyFlow"
        }
        assertNotNull(
            "EditorSessionCoordinator must expose motionPolicyFlow: StateFlow<EditorMotionPolicy>",
            field,
        )
        assertTrue(
            "motionPolicyFlow must be a StateFlow",
            field != null && StateFlow::class.java.isAssignableFrom(field.type),
        )
    }

    @Test
    fun windowHost_exposesMotionPolicyFlow() {
        val getter: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "getMotionPolicyFlow"
        }
        assertNotNull(
            "EditorWindowHost must expose motionPolicyFlow getter",
            getter,
        )
        if (getter != null) {
            assertTrue(
                "motionPolicyFlow must return a StateFlow",
                StateFlow::class.java.isAssignableFrom(getter.returnType),
            )
        }
    }

    @Test
    fun windowHost_applyMotionPolicyExists() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "applyMotionPolicy" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == EditorMotionPolicy::class.java
        }
        assertNotNull(
            "EditorWindowHost must have applyMotionPolicy(EditorMotionPolicy) for atomic policy application",
            method,
        )
    }

    @Test
    fun reduceMotionFieldExistsOnMotionPolicy() {
        val field = EditorMotionPolicy::class.java.declaredFields.firstOrNull {
            it.name == "reduceMotion"
        }
        assertNotNull(
            "EditorMotionPolicy must have reduceMotion field",
            field,
        )
    }

    @Test
    fun reduceMotionFieldExistsOnAnimationSettings() {
        val field = EditorAnimationSettings::class.java.declaredFields.firstOrNull {
            it.name == "reduceMotion"
        }
        assertNotNull(
            "EditorAnimationSettings must have reduceMotion field for settings wiring",
            field,
        )
    }

    @Test
    fun effectiveReduceMotionDisablesEverything() {
        val policy = EditorMotionPolicy(
            textEnabled = true,
            cursorEnabled = true,
            coordinated = true,
            reduceMotion = true,
        )
        val effective = policy.effective()
        assertFalse(effective.textEnabled)
        assertFalse(effective.cursorEnabled)
        assertFalse(effective.coordinated)
    }

    @Test
    fun fromMotionPolicyPreservesReduceMotion() {
        val policy = EditorMotionPolicy(reduceMotion = true)
        val settings = EditorAnimationSettings.fromMotionPolicy(policy)
        assertTrue("reduceMotion must round-trip through EditorAnimationSettings", settings.reduceMotion)
    }
}
