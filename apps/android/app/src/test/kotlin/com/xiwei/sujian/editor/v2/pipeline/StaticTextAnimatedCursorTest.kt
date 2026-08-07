package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 五：静态文字路径 + 动画光标的渲染行为测试。
 *
 * 结构契约（FrameRenderInput.cursorTransition 字段存在性）已移入
 * [com.xiwei.sujian.arch.StaticTextAnimatedCursorArchitectureTest]；本文件只保留运行时行为：
 * - 文字轨结束、光标轨未结束（progress<1f）时动画绘制光标；
 * - 光标轨结束/取消/无过渡/动画关闭时回落静态光标。
 */
class StaticTextAnimatedCursorTest {
    private fun transition(shouldAnimate: Boolean = true) =
        PreparedVisualTransaction.CursorTransition(
            fromX = 10f,
            fromY = 20f,
            fromHeight = 30f,
            toX = 40f,
            toY = 60f,
            toHeight = 30f,
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
    fun tickKeepsCursorTransitionIndependentOfTextTransaction() {
        val withTextFinished = null
        val cursorStillRunning = transition()
        val renderCursor =
            cursorStillRunning.shouldAnimate &&
                withTextFinished == null
        assertTrue(
            "文字事务为 null（文字轨结束）时光标过渡仍须保留",
            renderCursor,
        )
    }
}
