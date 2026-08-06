package com.xiwei.sujian.editor.v2.motion

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
 * #595 三/七：EditorMotionPolicy StateFlow 契约测试 — 验证动画策略通过只读 StateFlow
 * 暴露为单一可观察事实源，applyMotionPolicy 原子更新，reduceMotion 从设置接入。
 * #595 十：EditorAnimationSettings 已删除 — 不再存在第二套可写动画设置类型。
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
    fun editorAnimationSettingsTypeIsDeleted() {
        // #595 十：EditorAnimationSettings 与 EditorMotionPolicy 重复，已删除。
        val deleted = runCatching {
            Class.forName("com.xiwei.sujian.editor.v2.coordinator.EditorAnimationSettings")
        }.isFailure
        assertTrue(
            "EditorAnimationSettings must be deleted after #595 十 — " +
            "EditorMotionPolicy is the only writable animation state source",
            deleted,
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
    fun reduceMotionDisablesViaEffective() {
        val policy = EditorMotionPolicy(reduceMotion = true)
        assertTrue("reduceMotion must disable text via effective()", !policy.effective().textEnabled)
        assertTrue("reduceMotion must disable cursor via effective()", !policy.effective().cursorEnabled)
    }
}
