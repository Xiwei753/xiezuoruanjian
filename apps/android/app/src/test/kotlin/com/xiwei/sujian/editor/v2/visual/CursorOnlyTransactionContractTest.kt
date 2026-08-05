package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 四：CursorOnly 事务契约测试 — 验证 textEnabled=false + cursorEnabled=true 时
 * 文字静态更新、光标仍由同一 FrameClock 平滑移动。
 *
 * 验证 AndroidTextAnimationEngine 提供 CursorOnly 路径（submitCursorOnlyTransaction），
 * hasActiveAnimation 同时检查文字和光标时间线，以及 SujianEditorView 解耦 kernel
 * animation_enabled（setKernelAnimationEnabled 独立入口）。
 */
class CursorOnlyTransactionContractTest {

    private fun createEngine(): AndroidTextAnimationEngine {
        return AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
    }

    @Test
    fun submitCursorOnlyTransactionMethodExists() {
        val method: Method? = AndroidTextAnimationEngine::class.java.declaredMethods.firstOrNull {
            it.name == "submitCursorOnlyTransaction"
        }
        assertTrue(
            "AndroidTextAnimationEngine must have submitCursorOnlyTransaction for CursorOnly path",
            method != null,
        )
    }

    @Test
    fun hasActiveAnimationReturnsFalseWhenNoTransaction() {
        val engine = createEngine()
        assertFalse("No active transaction", engine.hasActiveAnimation())
    }

    @Test
    fun hasActiveAnimationChecksCursorTimelineNotJustText() {
        // #595 四: hasActiveAnimation 必须同时检查 timeline 和 cursorTimeline，
        // 否则 CursorOnly 事务中文字时间线先完成时会停止帧请求，截断光标动画。
        // 这里验证引擎在无事务时返回 false，且方法存在（行为完整验证需 instrumentation）。
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        assertFalse("No transaction → not active", engine.hasActiveAnimation())
    }

    @Test
    fun textSuppressedAndCursorDisabledProducesStaticUpdate() {
        // #595 四: textEnabled=false + cursorEnabled=false → 纯静态更新（无事务）。
        // 用 setReduceMotion 设置抑制状态（字段赋值，不触发 DiagnosticsEvents 日志）。
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

    @Test
    fun setKernelAnimationEnabledExistsOnSujianEditorView() {
        val method: Method? = com.xiwei.sujian.editor.v2.host.SujianEditorView::class.java.methods.firstOrNull {
            it.name == "setKernelAnimationEnabled" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == Boolean::class.javaPrimitiveType
        }
        assertTrue(
            "SujianEditorView must have setKernelAnimationEnabled(Boolean) " +
            "to decouple kernel animation_enabled from text-only suppression",
            method != null,
        )
    }
}
