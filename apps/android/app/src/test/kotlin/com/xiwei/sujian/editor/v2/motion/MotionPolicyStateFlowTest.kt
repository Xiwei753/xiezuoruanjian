package com.xiwei.sujian.editor.v2.motion

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三/七：EditorMotionPolicy StateFlow 行为测试。
 *
 * 结构契约（字段/方法/类存在性）已移入
 * [com.xiwei.sujian.arch.MotionPolicyStateFlowArchitectureTest]；本文件只保留运行时行为：
 * - reduceMotion=true 时 effective() 关闭所有动画；
 * - effective() 正确派生。
 */
class MotionPolicyStateFlowTest {
    @Test
    fun effectiveReduceMotionDisablesEverything() {
        val policy =
            EditorMotionPolicy(
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
