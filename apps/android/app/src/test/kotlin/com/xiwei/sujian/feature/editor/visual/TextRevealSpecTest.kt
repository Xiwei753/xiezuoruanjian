package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #605: TextRevealSpec fraction 计算契约测试 — 验证 reveal/swallow 动画的
 * progress → fraction 映射、initialFraction 连续性、多 cluster progress window。
 */
class TextRevealSpecTest {
    @Test
    fun revealFractionZeroIsInvisible() {
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = 0f,
                boundaryFromX = 0f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(0f, spec.fraction(0f), 0.001f)
    }

    @Test
    fun revealFractionHalfShowsHalf() {
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = 0f,
                boundaryFromX = 0f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(0.5f, spec.fraction(0.5f), 0.001f)
    }

    @Test
    fun revealFractionOneIsComplete() {
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = 0f,
                boundaryFromX = 0f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(1f, spec.fraction(1f), 0.001f)
    }

    @Test
    fun swallowFractionZeroIsComplete() {
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 0f,
                boundaryFromX = 100f,
                boundaryToX = 0f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(0f, spec.fraction(0f), 0.001f)
    }

    @Test
    fun swallowFractionOneIsInvisible() {
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 0f,
                boundaryFromX = 100f,
                boundaryToX = 0f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(1f, spec.fraction(1f), 0.001f)
    }

    @Test
    fun initialFractionContinuesFromCurrentPosition() {
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = 0f,
                boundaryFromX = 0f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
                initialFraction = 0.5f,
            )
        // At progress=0, fraction should be initialFraction (0.5)
        assertEquals(0.5f, spec.fraction(0f), 0.001f)
        // At progress=1, fraction should be 1f
        assertEquals(1f, spec.fraction(1f), 0.001f)
    }

    @Test
    fun progressWindowMultiCluster() {
        val spec1 =
            TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = 0f,
                boundaryFromX = 0f,
                boundaryToX = 50f,
                progressStart = 0f,
                progressEnd = 0.5f,
            )
        val spec2 =
            TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = 50f,
                boundaryFromX = 50f,
                boundaryToX = 100f,
                progressStart = 0.5f,
                progressEnd = 1f,
            )
        // At global progress 0.25, spec1 is at local 0.5, spec2 hasn't started
        assertEquals(0.5f, spec1.fraction(0.25f), 0.001f)
        assertEquals(0f, spec2.fraction(0.25f), 0.001f)
        // At global progress 0.75, spec1 is complete, spec2 is at local 0.5
        assertEquals(1f, spec1.fraction(0.75f), 0.001f)
        assertEquals(0.5f, spec2.fraction(0.75f), 0.001f)
    }
}
