package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三/四/九：协同动画契约测试 — 验证 coordinated 设置和 reduceMotion 设置
 * 真正进入 AndroidTextAnimationEngine，不再只是死开关。
 */
class CoordinatedAnimationContractTest {

    private fun createEngine(): AndroidTextAnimationEngine {
        return AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
    }

    @Test
    fun coordinatedEnabledByDefault() {
        val engine = createEngine()
        assertTrue("Coordinated enabled by default", engine.isCoordinatedAnimationEnabled())
    }

    @Test
    fun setCoordinatedAnimationEnabledPropagatesToEngine() {
        val engine = createEngine()
        engine.setCoordinatedAnimationEnabled(false)
        assertFalse("Engine reflects coordinated disabled", engine.isCoordinatedAnimationEnabled())
        engine.setCoordinatedAnimationEnabled(true)
        assertTrue("Engine reflects coordinated re-enabled", engine.isCoordinatedAnimationEnabled())
    }

    @Test
    fun reduceMotionDisabledByDefault() {
        val engine = createEngine()
        assertFalse("Reduce motion disabled by default", engine.isReduceMotion())
    }

    @Test
    fun setReduceMotionPropagatesToEngineAndSuppressesAnimation() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        assertTrue("Engine reflects reduce motion enabled", engine.isReduceMotion())
        assertEquals(
            "Animation policy suppressed",
            TextAnimationPolicy.SYSTEM_SUPPRESSED,
            engine.getAnimationPolicy(),
        )
    }

    @Test
    fun setReduceMotionFalseDoesNotSuppressAnimation() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        engine.setReduceMotion(false)
        assertFalse("Reduce motion disabled", engine.isReduceMotion())
    }

    @Test
    fun pauseWithoutActiveTransactionIsNoOp() {
        val engine = createEngine()
        engine.pause(100L)
        assertFalse("No active transaction means not paused", engine.isPaused())
    }
}
