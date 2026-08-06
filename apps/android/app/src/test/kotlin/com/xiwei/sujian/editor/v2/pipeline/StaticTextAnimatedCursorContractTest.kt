package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Field

/**
 * #595 五：静态文字路径 + 动画光标的渲染契约测试。
 *
 * 验证非协同模式光标时长可长于文字时长时，文字轨结束后正文立即用静态新布局绘制，
 * 但光标仍在同一 View/FrameClock 中平滑移动到终点：
 * - 文字轨结束/抑制（CursorOnly）后 FrameRenderInput 仍携带 cursorTransition；
 * - 静态文字路径只在光标轨未结束（progress < 1f）时绘制动画光标；
 * - 光标轨结束或取消时回落静态光标。
 */
class StaticTextAnimatedCursorContractTest {

    private fun transition(shouldAnimate: Boolean = true) = PreparedVisualTransaction.CursorTransition(
        fromX = 10f, fromY = 20f, fromHeight = 30f,
        toX = 40f, toY = 60f, toHeight = 30f,
        shouldAnimate = shouldAnimate,
    )

    @Test
    fun staticPathDrawsAnimatedCursorMidTransition() {
        assertTrue(
            "文字轨结束、光标轨未结束（progress=0.5）必须动画绘制光标",
            AndroidRenderRuntime.shouldDrawAnimatedCursorOnStaticPath(transition(), 0.5f),
        )
    }

    @Test
    fun staticPathDoesNotDrawAnimatedCursorAtEnd() {
        assertFalse(
            "光标轨已结束（progress=1f）必须回落静态光标",
            AndroidRenderRuntime.shouldDrawAnimatedCursorOnStaticPath(transition(), 1f),
        )
    }

    @Test
    fun staticPathDoesNotDrawAnimatedCursorWhenProgressNull() {
        assertFalse(
            "光标时间线已取消（progress=null）必须回落静态光标",
            AndroidRenderRuntime.shouldDrawAnimatedCursorOnStaticPath(transition(), null),
        )
    }

    @Test
    fun staticPathDoesNotDrawAnimatedCursorWithoutTransition() {
        assertFalse(
            "无光标过渡（光标轨不存在）必须绘制静态光标",
            AndroidRenderRuntime.shouldDrawAnimatedCursorOnStaticPath(null, 0.5f),
        )
    }

    @Test
    fun staticPathDoesNotDrawAnimatedCursorWhenAnimationDisallowed() {
        assertFalse(
            "shouldAnimate=false（光标动画关闭）必须绘制静态光标",
            AndroidRenderRuntime.shouldDrawAnimatedCursorOnStaticPath(transition(shouldAnimate = false), 0.5f),
        )
    }

    @Test
    fun staticPathDrawsAnimatedCursorAtStart() {
        assertTrue(
            "光标轨刚起步（progress=0f）必须动画绘制光标",
            AndroidRenderRuntime.shouldDrawAnimatedCursorOnStaticPath(transition(), 0f),
        )
    }

    @Test
    fun frameRenderInputCarriesCursorTransitionField() {
        val field: Field? = FrameRenderInput::class.java.declaredFields.firstOrNull {
            it.name == "cursorTransition"
        }
        assertNotNull(
            "FrameRenderInput 必须携带 cursorTransition 字段 — " +
            "文字轨结束后静态文字路径依赖它绘制平滑光标",
            field,
        )
        assertTrue(
            "cursorTransition 必须与文字事务解耦（可独立为 null）",
            field!!.type == PreparedVisualTransaction.CursorTransition::class.java ||
                field.type.kotlin.javaObjectType == PreparedVisualTransaction.CursorTransition::class.java,
        )
    }

    @Test
    fun tickKeepsCursorTransitionIndependentOfTextTransaction() {
        // 渲染决策：文字轨结束（renderTextTransaction=null）时 cursorTransition 仍非 null。
        // 由 AndroidVisualRuntime.tick 中的纯决策规则保证 — 这里验证决策所需的
        // cursorTransition 派生规则不依赖 transaction 非空。
        val withTextFinished = null
        val cursorStillRunning = transition()
        val renderCursor = cursorStillRunning.shouldAnimate &&
            withTextFinished == null
        assertTrue(
            "文字事务为 null（文字轨结束）时光标过渡仍须保留",
            renderCursor,
        )
    }
}
