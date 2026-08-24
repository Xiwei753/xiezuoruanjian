package com.xiwei.sujian.feature.editor.render

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.VisualProgressWindow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #605 评论4: AndroidTextAnimationRenderer.computeRevealClipRect 契约测试 —
 * 验证 LTR/RTL REVEAL/SWALLOW 在 progress=0.5 的实际裁剪矩形，以及
 * fraction=0/1 边界与空交集返回 null 的行为。
 *
 * computeRevealClipRect 的几何契约：
 * - boundary = boundaryFromX + (boundaryToX - boundaryFromX) * fraction
 * - left = min(anchorX, boundary), right = max(anchorX, boundary)
 * - clipLeft = max(left, destination.left), clipRight = min(right, destination.right)
 * - clipRight <= clipLeft → null
 * - REVEAL: fraction=0 → null, fraction=1 → 完整 destination
 * - SWALLOW: fraction=0 → 完整 destination, fraction=1 → null
 *
 * 这些测试锁住裁剪几何，防止未来重构（如把 anchorX/boundary 顺序搞反、
 * 或在 fraction=0/1 边界返回错误矩形）破坏揭示/收缩动画的视觉连续性。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AndroidTextAnimationRendererTest {
    private val renderer = AndroidTextAnimationRenderer()
    private val eps = 0.001f

    private fun makeSpec(
        mode: TextRevealMode,
        anchorX: Float,
        boundaryFromX: Float,
        boundaryToX: Float,
    ): TextRevealSpec {
        // progressStart=0, progressEnd=1, initialFraction=0 让 globalProgress 直接映射到 fraction
        return TextRevealSpec(
            mode = mode,
            anchorX = anchorX,
            boundaryFromX = boundaryFromX,
            boundaryToX = boundaryToX,
            progressStart = 0f,
            progressEnd = 1f,
            initialFraction = 0f,
        )
    }

    /**
     * LTR REVEAL progress=0.5: anchorX=0, boundary 0→100, destination=(0,0,100,20)。
     * fraction=0.5 → boundary=50 → clipRect=(0,0,50,20)。
     * 文字从左 caret 向右揭示一半。
     */
    @Test
    fun ltrRevealAtHalfProgress() {
        val spec = makeSpec(TextRevealMode.REVEAL, anchorX = 0f, boundaryFromX = 0f, boundaryToX = 100f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0.5f)
        assertNotNull("REVEAL fraction=0.5 应返回非 null clip", clip)
        assertEquals(0f, clip!!.left, eps)
        assertEquals(0f, clip.top, eps)
        assertEquals(50f, clip.right, eps)
        assertEquals(20f, clip.bottom, eps)
    }

    /**
     * LTR SWALLOW progress=0.5: anchorX=0, boundary 100→0, destination=(0,0,100,20)。
     * fraction=0.5 → boundary=50 → clipRect=(0,0,50,20)。
     * 文字从右向左 caret 收缩一半。
     */
    @Test
    fun ltrSwallowAtHalfProgress() {
        val spec = makeSpec(TextRevealMode.SWALLOW, anchorX = 0f, boundaryFromX = 100f, boundaryToX = 0f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0.5f)
        assertNotNull("SWALLOW fraction=0.5 应返回非 null clip", clip)
        assertEquals(0f, clip!!.left, eps)
        assertEquals(0f, clip.top, eps)
        assertEquals(50f, clip.right, eps)
        assertEquals(20f, clip.bottom, eps)
    }

    /**
     * RTL REVEAL progress=0.5: anchorX=100, boundary 100→0, destination=(0,0,100,20)。
     * fraction=0.5 → boundary=50 → left=50, right=100 → clipRect=(50,0,100,20)。
     * 文字从右 caret 向左揭示一半。
     */
    @Test
    fun rtlRevealAtHalfProgress() {
        val spec = makeSpec(TextRevealMode.REVEAL, anchorX = 100f, boundaryFromX = 100f, boundaryToX = 0f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0.5f)
        assertNotNull("RTL REVEAL fraction=0.5 应返回非 null clip", clip)
        assertEquals(50f, clip!!.left, eps)
        assertEquals(0f, clip.top, eps)
        assertEquals(100f, clip.right, eps)
        assertEquals(20f, clip.bottom, eps)
    }

    /**
     * RTL SWALLOW progress=0.5: anchorX=100, boundary 0→100, destination=(0,0,100,20)。
     * fraction=0.5 → boundary=50 → left=50, right=100 → clipRect=(50,0,100,20)。
     * 文字从左向右 caret 收缩一半。
     */
    @Test
    fun rtlSwallowAtHalfProgress() {
        val spec = makeSpec(TextRevealMode.SWALLOW, anchorX = 100f, boundaryFromX = 0f, boundaryToX = 100f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0.5f)
        assertNotNull("RTL SWALLOW fraction=0.5 应返回非 null clip", clip)
        assertEquals(50f, clip!!.left, eps)
        assertEquals(0f, clip.top, eps)
        assertEquals(100f, clip.right, eps)
        assertEquals(20f, clip.bottom, eps)
    }

    /**
     * REVEAL fraction=0 → null（不可见）。揭示动画起点无可见区域。
     */
    @Test
    fun revealAtFractionZeroReturnsNull() {
        val spec = makeSpec(TextRevealMode.REVEAL, anchorX = 0f, boundaryFromX = 0f, boundaryToX = 100f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0f)
        assertNull("REVEAL fraction=0 应返回 null（不可见）", clip)
    }

    /**
     * REVEAL fraction=1 → 完整 destination。揭示动画终点完全可见，包括 overhang。
     */
    @Test
    fun revealAtFractionOneReturnsFullDestination() {
        val spec = makeSpec(TextRevealMode.REVEAL, anchorX = 0f, boundaryFromX = 0f, boundaryToX = 100f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 1f)
        assertNotNull("REVEAL fraction=1 应返回完整 destination", clip)
        assertEquals(0f, clip!!.left, eps)
        assertEquals(0f, clip.top, eps)
        assertEquals(100f, clip.right, eps)
        assertEquals(20f, clip.bottom, eps)
    }

    /**
     * SWALLOW fraction=0 → 完整 destination。收缩动画起点完全可见。
     */
    @Test
    fun swallowAtFractionZeroReturnsFullDestination() {
        val spec = makeSpec(TextRevealMode.SWALLOW, anchorX = 0f, boundaryFromX = 100f, boundaryToX = 0f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0f)
        assertNotNull("SWALLOW fraction=0 应返回完整 destination", clip)
        assertEquals(0f, clip!!.left, eps)
        assertEquals(0f, clip.top, eps)
        assertEquals(100f, clip.right, eps)
        assertEquals(20f, clip.bottom, eps)
    }

    /**
     * SWALLOW fraction=1 → null（不可见）。收缩动画终点无可见区域。
     */
    @Test
    fun swallowAtFractionOneReturnsNull() {
        val spec = makeSpec(TextRevealMode.SWALLOW, anchorX = 0f, boundaryFromX = 100f, boundaryToX = 0f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 1f)
        assertNull("SWALLOW fraction=1 应返回 null（不可见）", clip)
    }

    /**
     * 空交集 → null: anchorX 和 boundary 都在 destination 之外（右侧）。
     * clipLeft 超过 destination.right → clipRight <= clipLeft → null。
     */
    @Test
    fun emptyIntersectionReturnsNull() {
        val spec = makeSpec(TextRevealMode.REVEAL, anchorX = 200f, boundaryFromX = 200f, boundaryToX = 300f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0.5f)
        assertNull("anchorX/boundary 在 destination 之外应返回 null", clip)
    }

    /**
     * 空交集（左侧）→ null: anchorX 和 boundary 都在 destination 左侧。
     */
    @Test
    fun emptyIntersectionOnLeftReturnsNull() {
        val spec = makeSpec(TextRevealMode.REVEAL, anchorX = -200f, boundaryFromX = -200f, boundaryToX = -100f)
        val dest = RectF(0f, 0f, 100f, 20f)
        val clip = renderer.computeRevealClipRect(dest, spec, 0.5f)
        assertNull("anchorX/boundary 在 destination 左侧应返回 null", clip)
    }

    // #638: computeStaticSuppressionRegions 测试 — 验证 Delete SWALLOW 按当前帧可见区域 suppress

    /**
     * 旧逻辑：computeAnimatedSliceRegions 完全排除 Delete，导致双绘。
     * 新逻辑：computeStaticSuppressionRegions(progress) 应包含 Delete SWALLOW 在 progress=0 时的完整区域。
     */
    @Test
    fun computeStaticSuppressionRegions_includesDeleteSwallowAtProgressZero() {
        val dest = RectF(100f, 50f, 200f, 70f)
        val spec = TextRevealSpec(
            mode = TextRevealMode.SWALLOW,
            anchorX = 100f,
            boundaryFromX = 200f,
            boundaryToX = 100f,
            progressStart = 0f,
            progressEnd = 1f,
            initialFraction = 0f
        )
        val slice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Delete,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = dest,
            startAlpha = 1f,
            endAlpha = 0f,
            revealSpec = spec
        )
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = listOf(slice),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 300L
        )

        // 旧逻辑：computeAnimatedSliceRegions 会跳过 Delete，返回空列表
        val oldRegions = renderer.computeAnimatedSliceRegions(transaction)
        assertTrue("旧逻辑应排除 Delete，返回空列表", oldRegions.isEmpty())

        // 新逻辑：computeStaticSuppressionRegions(0f) 应返回完整 destination
        val newRegions = renderer.computeStaticSuppressionRegions(transaction, 0f)
        assertEquals("progress=0 时应 suppress 完整 Delete 区域", 1, newRegions.size)
        assertEquals(100f, newRegions[0].left, eps)
        assertEquals(50f, newRegions[0].top, eps)
        assertEquals(200f, newRegions[0].right, eps)
        assertEquals(70f, newRegions[0].bottom, eps)
    }

    /**
     * Delete SWALLOW 在 progress=0.5 时应只 suppress 一半区域（从 anchor 到 boundary）。
     */
    @Test
    fun computeStaticSuppressionRegions_suppressHalfOfDeleteSwallowAtProgressHalf() {
        val dest = RectF(100f, 50f, 200f, 70f)
        val spec = TextRevealSpec(
            mode = TextRevealMode.SWALLOW,
            anchorX = 100f,
            boundaryFromX = 200f,
            boundaryToX = 100f,
            progressStart = 0f,
            progressEnd = 1f,
            initialFraction = 0f
        )
        val slice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Delete,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = dest,
            startAlpha = 1f,
            endAlpha = 0f,
            revealSpec = spec
        )
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = listOf(slice),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 300L
        )

        val newRegions = renderer.computeStaticSuppressionRegions(transaction, 0.5f)
        assertEquals("progress=0.5 时应 suppress 一半 Delete 区域", 1, newRegions.size)
        // SWALLOW fraction=0.5: boundary=150, clipRect=(100,0,150,20) 在 destination 内
        assertEquals(100f, newRegions[0].left, eps)
        assertEquals(50f, newRegions[0].top, eps)
        assertEquals(150f, newRegions[0].right, eps)
        assertEquals(70f, newRegions[0].bottom, eps)
    }

    /**
     * Delete SWALLOW 在 progress=1 时应返回空列表（完全消失，不 suppress 任何区域）。
     */
    @Test
    fun computeStaticSuppressionRegions_returnsEmptyForDeleteSwallowAtProgressOne() {
        val dest = RectF(100f, 50f, 200f, 70f)
        val spec = TextRevealSpec(
            mode = TextRevealMode.SWALLOW,
            anchorX = 100f,
            boundaryFromX = 200f,
            boundaryToX = 100f,
            progressStart = 0f,
            progressEnd = 1f,
            initialFraction = 0f
        )
        val slice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Delete,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = dest,
            startAlpha = 1f,
            endAlpha = 0f,
            revealSpec = spec
        )
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = listOf(slice),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 300L
        )

        val newRegions = renderer.computeStaticSuppressionRegions(transaction, 1f)
        assertTrue("progress=1 时应返回空列表（Delete 已完全消失）", newRegions.isEmpty())
    }

    /**
     * CrossfadeOld 不应 suppress 任何区域（保持 alpha 混合语义）。
     */
    @Test
    fun computeStaticSuppressionRegions_doesNotSuppressCrossfadeOld() {
        val dest = RectF(100f, 50f, 200f, 70f)
        val slice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.CrossfadeOld,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = dest,
            startAlpha = 1f,
            endAlpha = 0f
        )
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = listOf(slice),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 300L
        )

        val newRegions = renderer.computeStaticSuppressionRegions(transaction, 0.5f)
        assertTrue("CrossfadeOld 不应 suppress 任何区域", newRegions.isEmpty())
    }

    /**
     * Insert/Move/CrossfadeNew 继续 suppress destinationRect。
     */
    @Test
    fun computeStaticSuppressionRegions_includesInsertAndMoveAndCrossfadeNew() {
        val dest1 = RectF(0f, 0f, 100f, 20f)
        val dest2 = RectF(0f, 20f, 100f, 40f)
        val dest3 = RectF(0f, 40f, 100f, 60f)
        val insertSlice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Insert,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = dest1,
            startAlpha = 1f,
            endAlpha = 1f
        )
        val moveSlice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Move,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = dest2,
            startAlpha = 1f,
            endAlpha = 1f,
            fromDestinationRect = RectF(0f, 0f, 100f, 20f)
        )
        val crossfadeNewSlice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.CrossfadeNew,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = dest3,
            startAlpha = 0f,
            endAlpha = 1f
        )
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = listOf(insertSlice, moveSlice, crossfadeNewSlice),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 300L
        )

        val newRegions = renderer.computeStaticSuppressionRegions(transaction, 0.5f)
        assertEquals("Insert/Move/CrossfadeNew 都应 suppress", 3, newRegions.size)
    }
}
