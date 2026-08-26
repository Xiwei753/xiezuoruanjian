package com.xiwei.sujian.feature.editor.render

import android.graphics.Canvas
import android.graphics.Paint
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.TextRevealGeometry
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec

class AndroidTextAnimationRenderer {
    private val slicePaint =
        Paint().apply {
            isAntiAlias = true
        }

    fun drawAnimatedSlices(
        canvas: Canvas,
        transaction: PreparedVisualTransaction,
        progress: Float,
    ) {
        for (slice in transaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            val bitmap = snapshot.bitmap ?: continue
            // #637 评论 5386066978 项2：每个 slice 先把全局 progress 映射到
            // 本 slice 的 local progress（rebase continuation 时已走部分不重新计时）。
            val localProgress = slice.progressWindow.map(progress)

            when (slice.role) {
                SliceRole.Move -> {
                    // fromDestinationRect is the pre-move position; destinationRect is the
                    // post-move position. For cross-line Moves these can be on different lines;
                    // the interpolation moves the bitmap smoothly between them.
                    //
                    // sourceRect is always from the NEW layout's Bitmap (the slice's snapshot
                    // belongs to the new revision). This is correct because:
                    // (a) The new Bitmap contains the actual glyph pixels at the destination
                    //     position — the old Bitmap may have different sub-pixel rendering or
                    //     font fallback.
                    // (b) Move is only generated when shapingIdentityConfident is true on BOTH
                    //     old and new clusters, meaning the glyph pixels are visually identical.
                    //     Using the new Bitmap therefore produces no visual difference from using
                    //     the old Bitmap, while avoiding the need to store both snapshots.
                    // (c) Move slices always have startAlpha = endAlpha = 1f (fully opaque
                    //     throughout the animation) because the text content is unchanged — only
                    //     its position transitions. Alpha variation would imply content change,
                    //     which contradicts the Move invariant (same shaping identity).
                    val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * localProgress
                    slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)
                    val fromRect = slice.fromDestinationRect ?: slice.destinationRect
                    val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * localProgress
                    val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * localProgress
                    val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * localProgress
                    val currentBottom =
                        fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * localProgress
                    val currentDest = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom)
                    canvas.drawBitmap(bitmap, slice.sourceRect, currentDest, slicePaint)
                }
                SliceRole.Insert, SliceRole.Delete -> {
                    drawRevealSlice(canvas, bitmap, slice, localProgress)
                }
                // CrossfadeOld: position does not change during animation — only alpha varies.
                // #639 评论 5421085782 问题2：若 fixedRevealClipRect 非空（旧 Insert 只
                // reveal 到一半被 rebase 成 CrossfadeOld），canvas.save()+clipRect(冻结的
                // document-space clip rect)+drawBitmap(完整 bitmap, sourceRect,
                // destinationRect)+restoreToCount()，再做 alpha 淂出。clip rect 在本次
                // CrossfadeOld 期间保持不动，只让 alpha 变化，不把半个字突然变成完整字
                // 再淡出。clip rect 用真实 caret reveal 几何（TextRevealGeometry）算出，
                // 与正常 Insert/Delete 的 computeRevealClipRect 共用同一份几何 — 字形
                // overhang 和 RTL 都自动正确，不再用 bitmap 宽度比例近似。
                SliceRole.CrossfadeOld -> {
                    val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * localProgress
                    slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)
                    val fixedClip = slice.fixedRevealClipRect
                    if (fixedClip != null) {
                        val save = canvas.save()
                        canvas.clipRect(fixedClip)
                        canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, slicePaint)
                        canvas.restoreToCount(save)
                    } else {
                        canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, slicePaint)
                    }
                }
                // CrossfadeNew/Static: position does not change during animation — only alpha
                // varies. The bitmap is drawn at its final destination with interpolated alpha;
                // no positional interpolation is needed.
                SliceRole.CrossfadeNew, SliceRole.Static -> {
                    val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * localProgress
                    slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)
                    canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, slicePaint)
                }
            }
        }
    }

    /**
     * Draw an Insert/Delete slice using clip-rect reveal/swallow animation.
     * Falls back to alpha-based drawing when [slice.revealSpec] is null
     * (e.g. whole-line fallback without cluster caret geometry).
     */
    private fun drawRevealSlice(
        canvas: Canvas,
        bitmap: android.graphics.Bitmap,
        slice: PreparedVisualTransaction.AnimatedSlice,
        progress: Float,
    ) {
        val spec = slice.revealSpec
        if (spec != null) {
            val clipRect = computeRevealClipRect(slice.destinationRect, spec, progress) ?: return
            val save = canvas.save()
            canvas.clipRect(clipRect)
            slicePaint.alpha = 255
            canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, slicePaint)
            canvas.restoreToCount(save)
        } else {
            val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress
            slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)
            canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, slicePaint)
        }
    }

    /**
     * Compute the clip rect for reveal/swallow animation at [globalProgress].
     *
     * #639 评论 5421085782 问题2：纯几何部分抽到 [TextRevealGeometry.computeRevealClipRect]，
     * 正常 Insert/Delete renderer 和 rebase 冻结（CrossfadeOld 的 fixedRevealClipRect）
     * 共用同一份几何，不再有第二套"按 bitmap 宽度乘 fraction"的近似。
     *
     * REVEAL: fraction=0 -> null (not visible), fraction=1 -> full destination.
     * SWALLOW: fraction=0 -> full destination, fraction=1 -> null (not visible).
     */
    fun computeRevealClipRect(
        destination: android.graphics.RectF,
        spec: TextRevealSpec,
        globalProgress: Float,
    ): android.graphics.RectF? {
        val fraction = spec.fraction(globalProgress)
        return TextRevealGeometry.computeRevealClipRect(
            destination,
            spec.mode,
            spec.anchorX,
            spec.boundaryFromX,
            spec.boundaryToX,
            fraction,
        )
    }

    fun drawAnimatedCursor(
        canvas: Canvas,
        transaction: PreparedVisualTransaction,
        progress: Float,
        cursorPaint: Paint,
    ) {
        val ct = transaction.cursorTransition ?: return
        drawAnimatedCursor(canvas, ct, progress, cursorPaint)
    }

    /**
     * #595 五：按独立光标过渡几何绘制动画光标 — 供静态文字路径（文字轨结束或
     * CursorOnly 抑制）在光标轨未结束时继续绘制平滑光标。
     *
     * #637 评论 5386066978 项2：先映射 [transition.progressWindow] 再插值，
     * rebase continuation 时光标已走部分不重新计时。
     *
     * #637 评论 5389230907：几何计算调用 [CursorTransition.rectAt] — 与
     * [AndroidTextAnimationEngine.computeCurrentCursorRect] 共用同一份实现，
     * 不会再次漂移。
     */
    fun drawAnimatedCursor(
        canvas: Canvas,
        transition: PreparedVisualTransaction.CursorTransition,
        progress: Float,
        cursorPaint: Paint,
    ) {
        if (!transition.shouldAnimate) return

        val rect = transition.rectAt(progress)
        canvas.drawRect(rect.left, rect.top, rect.right, rect.bottom, cursorPaint)
    }

    /**
     * #638: Compute static suppression regions for hole-punching at [progress].
     *
     * - Insert/Move/CrossfadeNew/Static: suppress [slice.destinationRect] (new-layout content).
     * - CrossfadeOld: alpha-mixed, NOT suppressed (no hole).
     * - Delete + SWALLOW: suppress only the currently visible portion via
     *   [computeRevealClipRect]. The destination is the old position; new layout text
     *   may have shifted there, so we only hide pixels the Delete slice still occupies.
     *
     * This ensures the same pixel is never double-rendered in the same frame:
     * static text with holes renders the new layout minus suppressed regions, then
     * animated slices draw on top. For Delete SWALLOW, the visible shrink-away region
     * is suppressed from the static draw, so the old snapshot doesn't overlap new text.
     */
    fun computeStaticSuppressionRegions(
        transaction: PreparedVisualTransaction,
        progress: Float,
    ): List<android.graphics.RectF> {
        val regions = mutableListOf<android.graphics.RectF>()
        for (slice in transaction.animatedSlices) {
            val srcRect = slice.sourceRect
            if (srcRect.width() <= 0 || srcRect.height() <= 0) continue

            when (slice.role) {
                SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew, SliceRole.Static -> {
                    regions.add(android.graphics.RectF(slice.destinationRect))
                }
                SliceRole.Delete -> {
                    val revealSpec = slice.revealSpec ?: continue
                    val localProgress = slice.progressWindow.map(progress)
                    val clipRect = computeRevealClipRect(slice.destinationRect, revealSpec, localProgress)
                    if (clipRect != null) {
                        regions.add(clipRect)
                    }
                }
                SliceRole.CrossfadeOld -> {
                    // CrossfadeOld uses alpha blending over the new layout — no hole.
                }
            }
        }
        return regions
    }
}
