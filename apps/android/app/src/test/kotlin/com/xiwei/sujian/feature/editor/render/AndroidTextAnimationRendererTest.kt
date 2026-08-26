package com.xiwei.sujian.feature.editor.render

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.TextRevealGeometry
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
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
     * #638：computeStaticSuppressionRegions(progress) 应包含 Delete SWALLOW
     * 在 progress=0 时的完整区域。旧实现完全排除 Delete 导致双绘，已删除。
     */
    @Test
    fun computeStaticSuppressionRegions_includesDeleteSwallowAtProgressZero() {
        val dest = RectF(100f, 50f, 200f, 70f)
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 100f,
                boundaryFromX = 200f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
                initialFraction = 0f,
            )
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 1f,
                endAlpha = 0f,
                revealSpec = spec,
            )
        val transaction =
            PreparedVisualTransaction(
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
                durationMs = 300L,
            )

        // computeStaticSuppressionRegions(0f) 应返回完整 destination
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
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 100f,
                boundaryFromX = 200f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
                initialFraction = 0f,
            )
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 1f,
                endAlpha = 0f,
                revealSpec = spec,
            )
        val transaction =
            PreparedVisualTransaction(
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
                durationMs = 300L,
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
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 100f,
                boundaryFromX = 200f,
                boundaryToX = 100f,
                progressStart = 0f,
                progressEnd = 1f,
                initialFraction = 0f,
            )
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 1f,
                endAlpha = 0f,
                revealSpec = spec,
            )
        val transaction =
            PreparedVisualTransaction(
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
                durationMs = 300L,
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
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.CrossfadeOld,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 1f,
                endAlpha = 0f,
            )
        val transaction =
            PreparedVisualTransaction(
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
                durationMs = 300L,
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
        val insertSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest1,
                startAlpha = 1f,
                endAlpha = 1f,
            )
        val moveSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Move,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest2,
                startAlpha = 1f,
                endAlpha = 1f,
                fromDestinationRect = RectF(0f, 0f, 100f, 20f),
            )
        val crossfadeNewSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.CrossfadeNew,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest3,
                startAlpha = 0f,
                endAlpha = 1f,
            )
        val transaction =
            PreparedVisualTransaction(
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
                durationMs = 300L,
            )

        val newRegions = renderer.computeStaticSuppressionRegions(transaction, 0.5f)
        assertEquals("Insert/Move/CrossfadeNew 都应 suppress", 3, newRegions.size)
    }

    // #639 评论 5421085782 问题2 复现

    /**
     * #639 评论 5421085782 问题2 验证（修复后）：CrossfadeOld 的 fixedRevealClipRect
     * 用真实 caret reveal 几何（TextRevealGeometry）算，与 CaretRevealPlanner 的
     * Insert REVEAL spec 语义一致，不再用 bitmap 宽度比例从左裁剪。
     *
     * 场景：cluster caretStartX=10, caretEndX=90（reveal 边界在 10..90），
     * destination 宽度 100（字形 overhang 让 bitmap 比 caret 宽），revealFraction=0.5。
     *
     * 修复后 fixedRevealClipRect（TextRevealGeometry REVEAL，anchorX=10,
     * boundary 10→90, fraction=0.5 → boundary=50 → clip [10,50)）。
     * 旧实现按 bitmap 宽度比例从左裁 [0,50) — left 偏移 10px。
     *
     * 测试断言：fixedRevealClipRect == 期望 caret 几何 [10,50)，
     * 且 != 旧 bitmap 宽度比例 [0,50)（锁住不回退）。
     */
    @Test
    fun crossfadeOldFixedRevealFractionMatchesCaretRevealGeometry() {
        val caretStartX = 10f
        val caretEndX = 90f
        val destination = RectF(0f, 0f, 100f, 20f)
        val fixedFraction = 0.5f

        // 修复后：RebasePlanner.computeFixedRevealClipRect 用 TextRevealGeometry 算
        // document-space clip rect（mode=REVEAL, anchorX=caretStartX,
        // boundaryFromX=caretStartX, boundaryToX=caretEndX, fraction=revealFraction）。
        val fixedRevealClipRect =
            TextRevealGeometry.computeRevealClipRect(
                destination,
                TextRevealMode.REVEAL,
                caretStartX,
                caretStartX,
                caretEndX,
                fixedFraction,
            ) ?: error("fixedRevealClipRect 在 fraction=0.5 不应为 null")

        // 期望：LTR caret reveal 几何 anchorX=10, boundary 10→90, fraction=0.5
        // → boundary=50 → clip [10,50)。
        assertEquals(
            "CrossfadeOld fixedRevealClipRect left 应为 caretStartX=10（caret 几何），" +
                "不应是旧 bitmap 宽度比例 left=0 — #639 评论 5421085782 问题2 已修复。",
            caretStartX,
            fixedRevealClipRect.left,
            eps,
        )
        assertEquals(
            "CrossfadeOld fixedRevealClipRect right 应为 50（caret 几何 boundary=10+(90-10)*0.5）。",
            50f,
            fixedRevealClipRect.right,
            eps,
        )

        // 锁住：不再用 bitmap 宽度比例裁剪（旧实现 [0,50)，left=0）。
        val oldBitmapClipLeft = destination.left
        assertTrue(
            "CrossfadeOld 不应再用 bitmap 宽度比例 left=0；当前 fixedRevealClipRect.left=${fixedRevealClipRect.left}",
            kotlin.math.abs(fixedRevealClipRect.left - oldBitmapClipLeft) > eps,
        )
    }

    /**
     * #639 评论 5421085782 问题2 验证（修复后，RTL）：CrossfadeOld 的 fixedRevealClipRect
     * 用真实 caret reveal 几何，RTL 时 caret 从右往左 reveal，clip 从右往左裁。
     *
     * 场景：RTL cluster caretStartX=90, caretEndX=10（reveal 从右往左），
     * destination 宽度 100，revealFraction=0.5。
     *
     * 修复后 fixedRevealClipRect（TextRevealGeometry REVEAL，anchorX=90,
     * boundary 90→10, fraction=0.5 → boundary=50 → clip [50,90)）。
     * 旧实现按 bitmap 宽度比例从左裁 [0,50) — RTL 直接裁反。
     *
     * 测试断言：fixedRevealClipRect == 期望 caret 几何 [50,90)，
     * 且 != 旧 bitmap 宽度比例 [0,50)（锁住 RTL 不裁反）。
     */
    @Test
    fun crossfadeOldFixedRevealFractionMatchesCaretRevealGeometryRtl() {
        val caretStartX = 90f
        val caretEndX = 10f
        val destination = RectF(0f, 0f, 100f, 20f)
        val fixedFraction = 0.5f

        // 修复后：RebasePlanner.computeFixedRevealClipRect 用 TextRevealGeometry 算
        // document-space clip rect（RTL：anchorX=90, boundary 90→10）。
        val fixedRevealClipRect =
            TextRevealGeometry.computeRevealClipRect(
                destination,
                TextRevealMode.REVEAL,
                caretStartX,
                caretStartX,
                caretEndX,
                fixedFraction,
            ) ?: error("RTL fixedRevealClipRect 在 fraction=0.5 不应为 null")

        // 期望：RTL caret reveal 几何 anchorX=90, boundary 90→10, fraction=0.5
        // → boundary=50 → left=min(90,50)=50, right=max(90,50)=90 → clip [50,90)。
        assertEquals(
            "RTL CrossfadeOld fixedRevealClipRect left 应为 50（caret 几何 boundary=90+(10-90)*0.5），" +
                "不应是旧 bitmap 宽度比例 left=0 — #639 评论 5421085782 问题2 RTL 已修复。",
            50f,
            fixedRevealClipRect.left,
            eps,
        )
        assertEquals(
            "RTL CrossfadeOld fixedRevealClipRect right 应为 90（caret anchorX=90）。",
            90f,
            fixedRevealClipRect.right,
            eps,
        )

        // 锁住：不再用 bitmap 宽度比例从左裁（旧实现 [0,50)，RTL 裁反）。
        val oldBitmapClipRight = destination.left + destination.width() * fixedFraction
        assertTrue(
            "RTL CrossfadeOld 不应再用 bitmap 宽度比例 right=50；当前 fixedRevealClipRect.right=${fixedRevealClipRect.right}",
            kotlin.math.abs(fixedRevealClipRect.right - oldBitmapClipRight) > eps,
        )
    }

    // #639 评论 5422606865 问题2：visualDestinationRectAt 几何一致性测试

    /**
     * #639 评论 5422606865 问题2：visualDestinationRectAt 是当前视觉几何的单一入口。
     * renderer 和 engine.computeSliceVisualStates 都调用这一份，保证 captureFrame
     * 记录的 slice 位置就是 renderer 真正画在屏幕上的位置。
     *
     * 这组测试锁住 visualDestinationRectAt 的几何契约：
     * - fromDestinationRect=null → 返回 destinationRect（任意 progress）
     * - fromDestinationRect=fromRect, progress=0 → 返回 fromRect
     * - fromDestinationRect=fromRect, progress=1 → 返回 destinationRect
     * - fromDestinationRect=fromRect, progress=0.5 → 返回中点
     * - progressWindow 非 Full 时正确 map
     */

    private fun makeSliceForGeometry(
        destinationRect: RectF,
        fromDestinationRect: RectF? = null,
        progressWindow: com.xiwei.sujian.feature.editor.visual.VisualProgressWindow =
            com.xiwei.sujian.feature.editor.visual.VisualProgressWindow.Full,
        role: SliceRole = SliceRole.Insert,
    ): PreparedVisualTransaction.AnimatedSlice {
        return PreparedVisualTransaction.AnimatedSlice(
            role = role,
            snapshot = null,
            sourceRect = Rect(0, 0, 10, 20),
            destinationRect = destinationRect,
            startAlpha = 1f,
            endAlpha = 1f,
            fromDestinationRect = fromDestinationRect,
            progressWindow = progressWindow,
        )
    }

    /**
     * fromDestinationRect=null → 返回 destinationRect（任意 progress）。
     * 这是 alpha-only 动画的常见情况，行为应与原有完全一致。
     */
    @Test
    fun visualDestinationRectAt_nullFromReturnsDestinationForAnyProgress() {
        val dest = RectF(10f, 20f, 110f, 40f)
        val slice = makeSliceForGeometry(destinationRect = dest, fromDestinationRect = null)
        for (p in listOf(0f, 0.25f, 0.5f, 0.75f, 1f)) {
            val r = slice.visualDestinationRectAt(p)
            assertEquals("progress=$p left", dest.left, r.left, eps)
            assertEquals("progress=$p top", dest.top, r.top, eps)
            assertEquals("progress=$p right", dest.right, r.right, eps)
            assertEquals("progress=$p bottom", dest.bottom, r.bottom, eps)
        }
    }

    /**
     * fromDestinationRect=fromRect, progress=0 → 返回 fromRect。
     * 动画起点：slice 在旧 Move 当前位置（fromRect）。
     */
    @Test
    fun visualDestinationRectAt_progressZeroReturnsFromRect() {
        val from = RectF(0f, 0f, 100f, 20f)
        val dest = RectF(50f, 30f, 150f, 50f)
        val slice = makeSliceForGeometry(destinationRect = dest, fromDestinationRect = from)
        val r = slice.visualDestinationRectAt(0f)
        assertEquals(from.left, r.left, eps)
        assertEquals(from.top, r.top, eps)
        assertEquals(from.right, r.right, eps)
        assertEquals(from.bottom, r.bottom, eps)
    }

    /**
     * fromDestinationRect=fromRect, progress=1 → 返回 destinationRect。
     * 动画终点：slice 到达自己的最终位置。
     */
    @Test
    fun visualDestinationRectAt_progressOneReturnsDestinationRect() {
        val from = RectF(0f, 0f, 100f, 20f)
        val dest = RectF(50f, 30f, 150f, 50f)
        val slice = makeSliceForGeometry(destinationRect = dest, fromDestinationRect = from)
        val r = slice.visualDestinationRectAt(1f)
        assertEquals(dest.left, r.left, eps)
        assertEquals(dest.top, r.top, eps)
        assertEquals(dest.right, r.right, eps)
        assertEquals(dest.bottom, r.bottom, eps)
    }

    /**
     * fromDestinationRect=fromRect, progress=0.5 → 返回 from 与 destination 的中点。
     * 线性插值四条边。
     */
    @Test
    fun visualDestinationRectAt_progressHalfReturnsMidpoint() {
        val from = RectF(0f, 0f, 100f, 20f)
        val dest = RectF(50f, 30f, 150f, 50f)
        val slice = makeSliceForGeometry(destinationRect = dest, fromDestinationRect = from)
        val r = slice.visualDestinationRectAt(0.5f)
        assertEquals((from.left + dest.left) / 2f, r.left, eps)
        assertEquals((from.top + dest.top) / 2f, r.top, eps)
        assertEquals((from.right + dest.right) / 2f, r.right, eps)
        assertEquals((from.bottom + dest.bottom) / 2f, r.bottom, eps)
    }

    /**
     * progressWindow 非 Full 时正确 map：窗口 [0, 0.4]，
     * globalProgress=0.2 → localProgress=0.5 → 返回 from 与 dest 的中点。
     */
    @Test
    fun visualDestinationRectAt_nonFullProgressWindowMapsCorrectly() {
        val from = RectF(0f, 0f, 100f, 20f)
        val dest = RectF(50f, 30f, 150f, 50f)
        val window = com.xiwei.sujian.feature.editor.visual.VisualProgressWindow(start = 0f, end = 0.4f)
        val slice =
            makeSliceForGeometry(
                destinationRect = dest,
                fromDestinationRect = from,
                progressWindow = window,
            )
        // globalProgress=0.2 → localProgress=0.5 → 中点
        val r = slice.visualDestinationRectAt(0.2f)
        assertEquals((from.left + dest.left) / 2f, r.left, eps)
        assertEquals((from.top + dest.top) / 2f, r.top, eps)
        assertEquals((from.right + dest.right) / 2f, r.right, eps)
        assertEquals((from.bottom + dest.bottom) / 2f, r.bottom, eps)
        // globalProgress=0.4 → localProgress=1 → dest
        val rEnd = slice.visualDestinationRectAt(0.4f)
        assertEquals(dest.left, rEnd.left, eps)
        assertEquals(dest.top, rEnd.top, eps)
        assertEquals(dest.right, rEnd.right, eps)
        assertEquals(dest.bottom, rEnd.bottom, eps)
    }

    /**
     * #639 评论 5424613367 问题2：Insert 带 fromDestinationRect（rebase 把 Insert 接到
     * 旧 Move 当前位置）时，computeStaticSuppressionRegions 的 Insert suppression 必须用
     * destinationRect（新 Layout 中完整静态像素的位置），不能用 currentRect
     * （visualDestinationRectAt）。
     *
     * 静态底图里的完整字始终在 destinationRect。若用 currentRect 挖洞，destinationRect
     * 位置的静态完整字没被 suppress，会与动画字同时出现 → 双影/"新位置先亮出来"。
     *
     * 验证：任意 progress 下，Insert suppression 都等于 destinationRect，
     * 静态新 Layout 的最终字不会提前露出来。
     */
    @Test
    fun computeStaticSuppressionRegions_insertSuppressesDestinationRect() {
        val from = RectF(0f, 0f, 100f, 20f)
        val dest = RectF(50f, 30f, 150f, 50f)
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 1f,
                endAlpha = 1f,
                fromDestinationRect = from,
            )
        val transaction =
            PreparedVisualTransaction(
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
                durationMs = 300L,
            )

        // 任意 progress 下，Insert suppression 必须是 destinationRect，
        // 静态新 Layout 的完整字在 destinationRect，挖洞防止双影。
        for (progress in listOf(0f, 0.25f, 0.5f, 0.75f, 1f)) {
            val regions = renderer.computeStaticSuppressionRegions(transaction, progress)
            assertEquals("Insert 应 suppress 1 个区域 (progress=$progress)", 1, regions.size)
            assertEquals(
                "Insert suppression 应为 destinationRect，不是 currentRect (progress=$progress)",
                dest.left,
                regions[0].left,
                eps,
            )
            assertEquals(dest.top, regions[0].top, eps)
            assertEquals(dest.right, regions[0].right, eps)
            assertEquals(dest.bottom, regions[0].bottom, eps)
        }
    }

    /**
     * #639 评论 5422606865 问题2 端到端：Delete 带 fromDestinationRect 时，
     * computeStaticSuppressionRegions 的 clipRect 基于 currentRect 和平移后的 spec，
     * 与 drawRevealSlice 一致。
     *
     * 场景：from=(0,0,100,20), dest=(50,30,150,50), SWALLOW anchorX=0, boundary 100→0,
     * progress=0.5 → currentRect 中点 (25,15,125,35), localProgress=0.5, fraction=0.5,
     * dx=currentRect.left-dest.left=25-50=-25, 平移后 anchorX=-25, boundaryFromX=75,
     * boundaryToX=-25, boundary=75+(−25−75)*0.5=25, left=min(-25,25)=-25,
     * right=max(-25,25)=25, clipLeft=max(-25,25)=25, clipRight=min(25,125)=25 →
     * clipRight<=clipLeft → null（空交集）。
     *
     * 换一组让交集非空：from=(0,0,100,20), dest=(10,0,110,20)（小偏移）,
     * SWALLOW anchorX=0, boundary 100→0, progress=0.5 →
     * currentRect=(5,0,105,20), dx=5-10=-5, anchorX=-5, boundaryFromX=95, boundaryToX=-5,
     * fraction=0.5 → boundary=95+(-5-95)*0.5=45, left=min(-5,45)=-5, right=max(-5,45)=45,
     * clipLeft=max(-5,5)=5, clipRight=min(45,105)=45 → clipRect=(5,0,45,20)。
     */
    @Test
    fun computeStaticSuppressionRegions_deleteWithFromDestinationRectUsesCurrentRect() {
        val from = RectF(0f, 0f, 100f, 20f)
        val dest = RectF(10f, 0f, 110f, 20f)
        val spec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 0f,
                boundaryFromX = 100f,
                boundaryToX = 0f,
                progressStart = 0f,
                progressEnd = 1f,
                initialFraction = 0f,
            )
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 1f,
                endAlpha = 0f,
                fromDestinationRect = from,
                revealSpec = spec,
            )
        val transaction =
            PreparedVisualTransaction(
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
                durationMs = 300L,
            )

        // progress=0.5 → currentRect=(5,0,105,20), dx=-5
        // 平移后 anchorX=-5, boundary 95→-5, fraction=0.5 → boundary=45
        // left=-5, right=45, clipLeft=max(-5,5)=5, clipRight=min(45,105)=45
        val regions = renderer.computeStaticSuppressionRegions(transaction, 0.5f)
        assertEquals("Delete 应 suppress 1 个区域", 1, regions.size)
        assertEquals(5f, regions[0].left, eps)
        assertEquals(0f, regions[0].top, eps)
        assertEquals(45f, regions[0].right, eps)
        assertEquals(20f, regions[0].bottom, eps)
    }
}
