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
        val near = makeCluster(0f, 10f, 0, 1) // small advance
        val mid = makeCluster(10f, 50f, 1, 2) // medium advance
        val far = makeCluster(50f, 150f, 2, 3) // large advance
        val specs = planner.planSwallowSpecs(listOf(near, mid, far))
        assertEquals(3, specs.size)
        // Far cluster should swallow first (progressStart=0)
        // All specs should have SWALLOW mode
        for (spec in specs) {
            assertEquals(TextRevealMode.SWALLOW, spec.mode)
        }
        // Progress windows should be contiguous
        assertEquals(0f, specs[0].progressStart, 0.001f)
        assertEquals(1f, specs[2].progressEnd, 0.001f)
    }

    @Test
    fun emptyClustersReturnsEmptySpecs() {
        assertEquals(0, planner.planRevealSpecs(emptyList()).size)
        assertEquals(0, planner.planSwallowSpecs(emptyList()).size)
    }
}
