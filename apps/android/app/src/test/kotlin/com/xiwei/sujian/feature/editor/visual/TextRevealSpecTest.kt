package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #605: TextRevealSpec fraction 计算契约测试 — 验证 reveal/swallow 动画的
 * progress → fraction 映射、initialFraction 连续性、多 cluster progress window、
 * RTL clip 方向、多 cluster "不同时全显" 契约。
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

    /**
     * #605 FAIL 1: SWALLOW 在 progress=0.5 时应吞掉一半（fraction=0.5）。
     * fraction 返回动画进度本身，0.5 表示边界已从 boundaryFromX 向 boundaryToX
     * 移动一半，即剩余可见区域为一半。
     */
    @Test
    fun swallowFractionHalfShowsHalf() {
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 0f,
                boundaryFromX = 100f,
                boundaryToX = 0f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(0.5f, spec.fraction(0.5f), 0.001f)
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

    /**
     * #605 FAIL 2: RTL REVEAL spec（boundaryFromX > boundaryToX）的 fraction 契约。
     * fraction() 不依赖 boundary 方向，只依赖 progressStart/progressEnd/initialFraction，
     * 但需显式覆盖 RTL spec 的 0/0.5/1 三点，锁住 "RTL clip 方向正确" 契约。
     */
    @Test
    fun rtlRevealSpecFractionProgressesCorrectly() {
        // RTL: caretStartX > caretEndX, boundary 从右到左
        // REVEAL: anchorX=100, boundaryFromX=100, boundaryToX=0
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = 100f,
                boundaryFromX = 100f,
                boundaryToX = 0f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(0f, spec.fraction(0f), 0.001f)
        assertEquals(0.5f, spec.fraction(0.5f), 0.001f)
        assertEquals(1f, spec.fraction(1f), 0.001f)
    }

    /**
     * #605 FAIL 2: RTL SWALLOW spec（boundaryFromX < boundaryToX）的 fraction 契约。
     */
    @Test
    fun rtlSwallowSpecFractionProgressesCorrectly() {
        // RTL SWALLOW: anchorX=100, boundaryFromX=0, boundaryToX=100
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 100f,
                boundaryFromX = 0f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
            )
        assertEquals(0f, spec.fraction(0f), 0.001f)
        assertEquals(0.5f, spec.fraction(0.5f), 0.001f)
        assertEquals(1f, spec.fraction(1f), 0.001f)
    }

    /**
     * #605 WEAK: 多 cluster "不同时全显" 显式断言。
     *
     * 两个 cluster，窗口 [0,0.5] 和 [0.5,1]。除 progress=1 外，不存在所有 spec
     * 同时 fraction=1 的点 — 锁住 "多 cluster 逐个揭示，不会瞬间全部完整" 契约。
     */
    @Test
    fun multiClusterNotAllFullyRevealedSimultaneously() {
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

        // 在 0.25: spec1 部分可见 (0.5), spec2 不可见 (0)
        val f1At25 = spec1.fraction(0.25f)
        val f2At25 = spec2.fraction(0.25f)
        assertTrue("spec1 at 0.25 should be partial", f1At25 > 0f && f1At25 < 1f)
        assertEquals("spec2 at 0.25 should be invisible", 0f, f2At25, 0.001f)

        // 在 0.5: spec1 完整 (1), spec2 刚开始 (0)
        assertEquals(1f, spec1.fraction(0.5f), 0.001f)
        assertEquals(0f, spec2.fraction(0.5f), 0.001f)

        // 在 0.75: spec1 完整 (1), spec2 部分可见 (0.5)
        assertEquals(1f, spec1.fraction(0.75f), 0.001f)
        val f2At75 = spec2.fraction(0.75f)
        assertTrue("spec2 at 0.75 should be partial", f2At75 > 0f && f2At75 < 1f)

        // 关键契约：除 progress=1 外，不存在所有 spec 同时 fraction=1 的点
        for (p in listOf(0f, 0.25f, 0.5f, 0.75f)) {
            val allFull = spec1.fraction(p) >= 1f && spec2.fraction(p) >= 1f
            assertFalse("at progress=$p not all specs should be fully revealed", allFull)
        }
    }
}
