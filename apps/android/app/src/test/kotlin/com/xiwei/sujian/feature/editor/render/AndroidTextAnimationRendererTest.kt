package com.xiwei.sujian.feature.editor.render

import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
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
}
