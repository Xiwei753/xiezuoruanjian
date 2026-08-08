package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertFalse
import org.junit.Test

/**
 * #595 四：CursorOnly 事务行为测试 — textEnabled=false + cursorEnabled=true 时
 * 文字静态更新、光标仍由同一 FrameClock 平滑移动。
 *
 * 结构契约（方法存在性）已移入
 * [com.xiwei.sujian.arch.CursorOnlyTransactionArchitectureTest]；本文件只保留运行时行为：
 * - 无事务时 hasActiveAnimation 返回 false；
 * - reduceMotion + cursor off → 纯静态更新。
 */
class CursorOnlyTransactionTest {
    private fun createEngine(): AndroidTextAnimationEngine {
        return AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
    }

    @Test
    fun hasActiveAnimationReturnsFalseWhenNoTransaction() {
        val engine = createEngine()
        assertFalse("No active transaction", engine.hasActiveAnimation())
    }

    @Test
    fun hasActiveAnimationChecksCursorTimelineNotJustText() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        assertFalse("No transaction → not active", engine.hasActiveAnimation())
    }

    @Test
    fun textSuppressedAndCursorDisabledProducesStaticUpdate() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        engine.setSmoothCursor(false, 80L)
        assertFalse("Static update → no active animation", engine.hasActiveAnimation())
    }

    @Test
    fun reduceMotionAndCursorDisabledProducesStaticUpdate() {
        val engine = createEngine()
        engine.setReduceMotion(true)
        engine.setSmoothCursor(false, 80L)
        assertFalse("Reduce-motion + cursor off → no active animation", engine.hasActiveAnimation())
    }
}
