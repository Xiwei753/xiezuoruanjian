package com.xiwei.sujian.arch

import com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy
import kotlinx.coroutines.flow.StateFlow
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 三/七：EditorMotionPolicy StateFlow 结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证字段/方法/类存在性：
 * - EditorSessionCoordinator 暴露 motionPolicyFlow: StateFlow；
 * - EditorWindowHost 暴露 motionPolicyFlow getter 和 applyMotionPolicy；
 * - EditorMotionPolicy 有 reduceMotion 字段；
 * - EditorAnimationSettings 已删除。
 */
class MotionPolicyStateFlowArchitectureTest {
    @Test
    fun sessionCoordinator_exposesMotionPolicyFlow() {
        val field =
            EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
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
        val getter: Method? =
            EditorWindowHost::class.java.methods.firstOrNull {
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
        val method: Method? =
            EditorWindowHost::class.java.methods.firstOrNull {
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
        val field =
            EditorMotionPolicy::class.java.declaredFields.firstOrNull {
                it.name == "reduceMotion"
            }
        assertNotNull(
            "EditorMotionPolicy must have reduceMotion field",
            field,
        )
    }

    @Test
    fun editorAnimationSettingsTypeIsDeleted() {
        val deleted =
            runCatching {
                Class.forName("com.xiwei.sujian.editor.v2.coordinator.EditorAnimationSettings")
            }.isFailure
        assertTrue(
            "EditorAnimationSettings must be deleted after #595 十 — " +
                "EditorMotionPolicy is the only writable animation state source",
            deleted,
        )
    }
}
