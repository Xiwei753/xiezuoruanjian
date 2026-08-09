package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #605: CaretRevealPlanner 契约测试 — 验证单字 Insert/Delete、RTL、
 * 多 cluster progress window、Delete 从远到近收缩。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CaretRevealPlannerTest {
    private val planner = CaretRevealPlanner()

    private fun makeCluster(
        caretStartX: Float,
        caretEndX: Float,
        byteStart: Int = 0,
        byteEnd: Int = 1,
    ): LineClusterSnapshot {
        return LineClusterSnapshot(
            clusterId = 0L,
            documentByteStart = byteStart,
            documentByteEndExclusive = byteEnd,
            documentUtf16Start = byteStart,
            documentUtf16EndExclusive = byteEnd,
            sourceRectInLineImage = Rect(0, 0, 10, 20),
            visualRectInDocument = RectF(caretStartX, 0f, caretEndX, 20f),
            shapingFingerprint = "fp",
            shapingIdentityConfident = true,
            caretStartX = caretStartX,
            caretEndX = caretEndX,
        )
    }

    @Test
    fun singleInsertRevealSpec() {
        val cluster = makeCluster(0f, 100f)
        val plans = planner.planRevealSpecs(listOf(cluster))
        assertEquals(1, plans.size)
        assertEquals(TextRevealMode.REVEAL, plans[0].spec.mode)
        assertEquals(0f, plans[0].spec.anchorX, 0.001f)
        assertEquals(0f, plans[0].spec.boundaryFromX, 0.001f)
        assertEquals(100f, plans[0].spec.boundaryToX, 0.001f)
    }

    @Test
    fun singleDeleteSwallowSpec() {
        val cluster = makeCluster(0f, 100f)
        val plans = planner.planSwallowSpecs(listOf(cluster))
        assertEquals(1, plans.size)
        assertEquals(TextRevealMode.SWALLOW, plans[0].spec.mode)
        assertEquals(0f, plans[0].spec.anchorX, 0.001f)
        assertEquals(100f, plans[0].spec.boundaryFromX, 0.001f)
        assertEquals(0f, plans[0].spec.boundaryToX, 0.001f)
    }

    @Test
    fun rtlClusterCaretStartGreaterThanEnd() {
        // RTL: caretStartX > caretEndX
        val cluster = makeCluster(100f, 0f)
        val plans = planner.planRevealSpecs(listOf(cluster))
        assertEquals(1, plans.size)
        assertEquals(TextRevealMode.REVEAL, plans[0].spec.mode)
        assertEquals(100f, plans[0].spec.anchorX, 0.001f)
        assertEquals(100f, plans[0].spec.boundaryFromX, 0.001f)
        assertEquals(0f, plans[0].spec.boundaryToX, 0.001f)
    }

    @Test
    fun multiClusterProgressWindowsAreContiguous() {
        val c1 = makeCluster(0f, 50f, 0, 1)
        val c2 = makeCluster(50f, 100f, 1, 2)
        val plans = planner.planRevealSpecs(listOf(c1, c2))
        assertEquals(2, plans.size)
        // First cluster: progress 0 to 0.5
        assertEquals(0f, plans[0].spec.progressStart, 0.001f)
        assertEquals(0.5f, plans[0].spec.progressEnd, 0.001f)
        // Second cluster: progress 0.5 to 1
        assertEquals(0.5f, plans[1].spec.progressStart, 0.001f)
        assertEquals(1f, plans[1].spec.progressEnd, 0.001f)
    }

    /**
     * #605 WEAK: CaretRevealPlanner 生成的多 cluster specs 在中间 progress 不会同时全显。
     *
     * 两个 cluster（窗口 [0,0.5] 和 [0.5,1]），除 progress=1 外不存在所有 spec
     * 同时 fraction=1 的点 — 锁住 planner 输出的 "逐个揭示" 契约，而非仅检查
     * progress window 连续性。
     */
    @Test
    fun multiClusterNotAllFullyRevealedSimultaneously() {
        val c1 = makeCluster(0f, 50f, 0, 1)
        val c2 = makeCluster(50f, 100f, 1, 2)
        val plans = planner.planRevealSpecs(listOf(c1, c2))
        assertEquals(2, plans.size)
        // 确认 progress window 与 multiClusterProgressWindowsAreContiguous 一致
        assertEquals(0f, plans[0].spec.progressStart, 0.001f)
        assertEquals(0.5f, plans[0].spec.progressEnd, 0.001f)
        assertEquals(0.5f, plans[1].spec.progressStart, 0.001f)
        assertEquals(1f, plans[1].spec.progressEnd, 0.001f)

        // 在 0.25: spec0 部分可见 (0.5), spec1 不可见 (0)
        val f0At25 = plans[0].spec.fraction(0.25f)
        val f1At25 = plans[1].spec.fraction(0.25f)
        assertTrue("spec0 at 0.25 should be partial", f0At25 > 0f && f0At25 < 1f)
        assertEquals("spec1 at 0.25 should be invisible", 0f, f1At25, 0.001f)

        // 在 0.5: spec0 完整 (1), spec1 刚开始 (0)
        assertEquals(1f, plans[0].spec.fraction(0.5f), 0.001f)
        assertEquals(0f, plans[1].spec.fraction(0.5f), 0.001f)

        // 在 0.75: spec0 完整 (1), spec1 部分可见 (0.5)
        assertEquals(1f, plans[0].spec.fraction(0.75f), 0.001f)
        val f1At75 = plans[1].spec.fraction(0.75f)
        assertTrue("spec1 at 0.75 should be partial", f1At75 > 0f && f1At75 < 1f)

        // 关键契约：除 progress=1 外，不存在所有 spec 同时 fraction=1 的点
        for (p in listOf(0f, 0.25f, 0.5f, 0.75f)) {
            val allFull = plans[0].spec.fraction(p) >= 1f && plans[1].spec.fraction(p) >= 1f
            assertFalse("at progress=$p not all specs should be fully revealed", allFull)
        }
    }

    @Test
    fun deleteSwallowOrdersByDistanceFromCaret() {
        // Three clusters at different distances from caret
        val near = makeCluster(0f, 10f, 0, 1) // byte 0, near caret
        val mid = makeCluster(10f, 50f, 1, 2) // byte 1, mid distance
        val far = makeCluster(50f, 150f, 2, 3) // byte 2, far from caret
        val plans = planner.planSwallowSpecs(listOf(near, mid, far))
        assertEquals(3, plans.size)
        // All specs should have SWALLOW mode
        for (plan in plans) {
            assertEquals(TextRevealMode.SWALLOW, plan.spec.mode)
        }
        // By byte descending: far (byte 2) first, mid (byte 1) second, near (byte 0) third
        // far swallow spec: anchorX=50, boundaryFromX=150, boundaryToX=50
        assertEquals(50f, plans[0].spec.anchorX, 0.001f)
        assertEquals(150f, plans[0].spec.boundaryFromX, 0.001f)
        // mid swallow spec: anchorX=10, boundaryFromX=50, boundaryToX=10
        assertEquals(10f, plans[1].spec.anchorX, 0.001f)
        assertEquals(50f, plans[1].spec.boundaryFromX, 0.001f)
        // near swallow spec: anchorX=0, boundaryFromX=10, boundaryToX=0
        assertEquals(0f, plans[2].spec.anchorX, 0.001f)
        assertEquals(10f, plans[2].spec.boundaryFromX, 0.001f)
        // Progress windows should be contiguous
        assertEquals(0f, plans[0].spec.progressStart, 0.001f)
        assertEquals(1f, plans[2].spec.progressEnd, 0.001f)
    }

    @Test
    fun deleteSwallowOrdersByBytePositionNotAdvanceWidth() {
        // Cluster A: byte 0, advance 50 (wide, near final caret)
        // Cluster B: byte 1, advance 10 (narrow, far from final caret)
        // Issue #605 requires "distance from final caret, far to near" = byte descending
        // NOT advance descending. Without this test, the old advance-based sort
        // would order A first (advance 50 > 10), but correct order is B first (byte 1 > 0).
        val wideNearCaret = makeCluster(0f, 50f, 0, 1) // byte 0, advance 50
        val narrowFarFromCaret = makeCluster(50f, 60f, 1, 2) // byte 1, advance 10
        val plans = planner.planSwallowSpecs(listOf(wideNearCaret, narrowFarFromCaret))
        assertEquals(2, plans.size)
        // By byte descending: narrowFarFromCaret (byte 1) swallows first,
        // wideNearCaret (byte 0) swallows second.
        // narrowFarFromCaret swallow spec: anchorX=50, boundaryFromX=60, boundaryToX=50
        assertEquals(50f, plans[0].spec.anchorX, 0.001f)
        assertEquals(60f, plans[0].spec.boundaryFromX, 0.001f)
        assertEquals(50f, plans[0].spec.boundaryToX, 0.001f)
        // wideNearCaret swallow spec: anchorX=0, boundaryFromX=50, boundaryToX=0
        assertEquals(0f, plans[1].spec.anchorX, 0.001f)
        assertEquals(50f, plans[1].spec.boundaryFromX, 0.001f)
        assertEquals(0f, plans[1].spec.boundaryToX, 0.001f)
    }

    @Test
    fun emptyClustersReturnsEmptySpecs() {
        assertEquals(0, planner.planRevealSpecs(emptyList()).size)
        assertEquals(0, planner.planSwallowSpecs(emptyList()).size)
    }

    @Test
    fun hardBreakClustersAreFilteredOut() {
        // #605 评论4 问题3: hard line breaks have no visible glyph and must not
        // consume reveal progress. They are filtered out before planning so visible
        // clusters share the full [0,1] window.
        val visible = makeCluster(0f, 100f, 0, 1)
        val hardBreak =
            LineClusterSnapshot(
                clusterId = 1L,
                documentByteStart = 1,
                documentByteEndExclusive = 2,
                documentUtf16Start = 1,
                documentUtf16EndExclusive = 2,
                sourceRectInLineImage = Rect(0, 0, 1, 20),
                visualRectInDocument = RectF(100f, 0f, 100f, 20f),
                shapingFingerprint = "hardbreak",
                shapingIdentityConfident = true,
                caretStartX = 100f,
                caretEndX = 100f,
                isHardBreak = true,
            )
        // Insert: only the visible cluster gets a plan
        val revealPlans = planner.planRevealSpecs(listOf(visible, hardBreak))
        assertEquals(1, revealPlans.size)
        assertEquals(0f, revealPlans[0].spec.progressStart, 0.001f)
        assertEquals(1f, revealPlans[0].spec.progressEnd, 0.001f)
        // Delete: same filtering
        val swallowPlans = planner.planSwallowSpecs(listOf(visible, hardBreak))
        assertEquals(1, swallowPlans.size)
        // All-hard-break input returns empty
        assertEquals(0, planner.planRevealSpecs(listOf(hardBreak)).size)
        assertEquals(0, planner.planSwallowSpecs(listOf(hardBreak)).size)
    }

    @Test
    fun caretRevealPlanBindsClusterAndSpec() {
        // #605 评论4 问题1: CaretRevealPlan binds cluster+spec so callers never
        // misalign a cluster's Bitmap with another cluster's caret geometry.
        val c1 = makeCluster(0f, 50f, 0, 1)
        val c2 = makeCluster(50f, 100f, 1, 2)
        val plans = planner.planRevealSpecs(listOf(c1, c2))
        assertEquals(2, plans.size)
        // Each plan's cluster is the exact reference passed in (not a copy/reorder)
        assertTrue("plan0 cluster must be c1 by reference", plans[0].cluster === c1)
        assertTrue("plan1 cluster must be c2 by reference", plans[1].cluster === c2)
        // Swallow sorts by byte descending: c2 (byte 1) first, c1 (byte 0) second
        val swallowPlans = planner.planSwallowSpecs(listOf(c1, c2))
        assertEquals(2, swallowPlans.size)
        assertTrue("swallow plan0 cluster must be c2 (far from caret)", swallowPlans[0].cluster === c2)
        assertTrue("swallow plan1 cluster must be c1 (near caret)", swallowPlans[1].cluster === c1)
    }
}
