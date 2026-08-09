package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import org.junit.Assert.assertEquals
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
        val specs = planner.planRevealSpecs(listOf(cluster))
        assertEquals(1, specs.size)
        assertEquals(TextRevealMode.REVEAL, specs[0].mode)
        assertEquals(0f, specs[0].anchorX, 0.001f)
        assertEquals(0f, specs[0].boundaryFromX, 0.001f)
        assertEquals(100f, specs[0].boundaryToX, 0.001f)
    }

    @Test
    fun singleDeleteSwallowSpec() {
        val cluster = makeCluster(0f, 100f)
        val specs = planner.planSwallowSpecs(listOf(cluster))
        assertEquals(1, specs.size)
        assertEquals(TextRevealMode.SWALLOW, specs[0].mode)
        assertEquals(0f, specs[0].anchorX, 0.001f)
        assertEquals(100f, specs[0].boundaryFromX, 0.001f)
        assertEquals(0f, specs[0].boundaryToX, 0.001f)
    }

    @Test
    fun rtlClusterCaretStartGreaterThanEnd() {
        // RTL: caretStartX > caretEndX
        val cluster = makeCluster(100f, 0f)
        val specs = planner.planRevealSpecs(listOf(cluster))
        assertEquals(1, specs.size)
        assertEquals(TextRevealMode.REVEAL, specs[0].mode)
        assertEquals(100f, specs[0].anchorX, 0.001f)
        assertEquals(100f, specs[0].boundaryFromX, 0.001f)
        assertEquals(0f, specs[0].boundaryToX, 0.001f)
    }

    @Test
    fun multiClusterProgressWindowsAreContiguous() {
        val c1 = makeCluster(0f, 50f, 0, 1)
        val c2 = makeCluster(50f, 100f, 1, 2)
        val specs = planner.planRevealSpecs(listOf(c1, c2))
        assertEquals(2, specs.size)
        // First cluster: progress 0 to 0.5
        assertEquals(0f, specs[0].progressStart, 0.001f)
        assertEquals(0.5f, specs[0].progressEnd, 0.001f)
        // Second cluster: progress 0.5 to 1
        assertEquals(0.5f, specs[1].progressStart, 0.001f)
        assertEquals(1f, specs[1].progressEnd, 0.001f)
    }

    @Test
    fun deleteSwallowOrdersByDistanceFromCaret() {
        // Three clusters at different distances from caret
        val near = makeCluster(0f, 10f, 0, 1) // byte 0, near caret
        val mid = makeCluster(10f, 50f, 1, 2) // byte 1, mid distance
        val far = makeCluster(50f, 150f, 2, 3) // byte 2, far from caret
        val specs = planner.planSwallowSpecs(listOf(near, mid, far))
        assertEquals(3, specs.size)
        // All specs should have SWALLOW mode
        for (spec in specs) {
            assertEquals(TextRevealMode.SWALLOW, spec.mode)
        }
        // By byte descending: far (byte 2) first, mid (byte 1) second, near (byte 0) third
        // far swallow spec: anchorX=50, boundaryFromX=150, boundaryToX=50
        assertEquals(50f, specs[0].anchorX, 0.001f)
        assertEquals(150f, specs[0].boundaryFromX, 0.001f)
        // mid swallow spec: anchorX=10, boundaryFromX=50, boundaryToX=10
        assertEquals(10f, specs[1].anchorX, 0.001f)
        assertEquals(50f, specs[1].boundaryFromX, 0.001f)
        // near swallow spec: anchorX=0, boundaryFromX=10, boundaryToX=0
        assertEquals(0f, specs[2].anchorX, 0.001f)
        assertEquals(10f, specs[2].boundaryFromX, 0.001f)
        // Progress windows should be contiguous
        assertEquals(0f, specs[0].progressStart, 0.001f)
        assertEquals(1f, specs[2].progressEnd, 0.001f)
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
        val specs = planner.planSwallowSpecs(listOf(wideNearCaret, narrowFarFromCaret))
        assertEquals(2, specs.size)
        // By byte descending: narrowFarFromCaret (byte 1) swallows first,
        // wideNearCaret (byte 0) swallows second.
        // narrowFarFromCaret swallow spec: anchorX=50, boundaryFromX=60, boundaryToX=50
        assertEquals(50f, specs[0].anchorX, 0.001f)
        assertEquals(60f, specs[0].boundaryFromX, 0.001f)
        assertEquals(50f, specs[0].boundaryToX, 0.001f)
        // wideNearCaret swallow spec: anchorX=0, boundaryFromX=50, boundaryToX=0
        assertEquals(0f, specs[1].anchorX, 0.001f)
        assertEquals(50f, specs[1].boundaryFromX, 0.001f)
        assertEquals(0f, specs[1].boundaryToX, 0.001f)
    }

    @Test
    fun emptyClustersReturnsEmptySpecs() {
        assertEquals(0, planner.planRevealSpecs(emptyList()).size)
        assertEquals(0, planner.planSwallowSpecs(emptyList()).size)
    }
}
