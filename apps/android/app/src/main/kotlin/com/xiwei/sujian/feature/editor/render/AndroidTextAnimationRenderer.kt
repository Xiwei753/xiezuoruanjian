package com.xiwei.sujian.feature.editor.render

import android.graphics.Canvas
import android.graphics.Paint
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
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
                // CrossfadeOld/CrossfadeNew/Static: position does not change during
                // animation — only alpha varies. The bitmap is drawn at its final destination
                // with interpolated alpha; no positional interpolation is needed.
                SliceRole.CrossfadeOld, SliceRole.CrossfadeNew, SliceRole.Static -> {
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
     * REVEAL: fraction=0 -> null (not visible), fraction=1 -> full destination.
     * SWALLOW: fraction=0 -> full destination, fraction=1 -> null (not visible).
     *
     * The clip rect is the intersection of [destination] with the region between
     * [spec.anchorX] and the interpolated boundary position. This ensures only
     * the visible portion of the glyph is drawn, while overhang at fraction=1
     * is fully shown (returns complete destination, not clipped to caret X).
     */
    fun computeRevealClipRect(
        destination: android.graphics.RectF,
        spec: TextRevealSpec,
        globalProgress: Float,
    ): android.graphics.RectF? {
        val fraction = spec.fraction(globalProgress)
        return when (spec.mode) {
            TextRevealMode.REVEAL -> {
                if (fraction <= 0f) return null
                if (fraction >= 1f) return android.graphics.RectF(destination)
                val boundary = spec.boundaryFromX + (spec.boundaryToX - spec.boundaryFromX) * fraction
                val left = kotlin.math.min(spec.anchorX, boundary)
                val right = kotlin.math.max(spec.anchorX, boundary)
                val clipLeft = kotlin.math.max(left, destination.left)
                val clipRight = kotlin.math.min(right, destination.right)
                if (clipRight <= clipLeft) return null
                android.graphics.RectF(clipLeft, destination.top, clipRight, destination.bottom)
            }
            TextRevealMode.SWALLOW -> {
                if (fraction >= 1f) return null
                if (fraction <= 0f) return android.graphics.RectF(destination)
                val boundary = spec.boundaryFromX + (spec.boundaryToX - spec.boundaryFromX) * fraction
                val left = kotlin.math.min(spec.anchorX, boundary)
                val right = kotlin.math.max(spec.anchorX, boundary)
                val clipLeft = kotlin.math.max(left, destination.left)
                val clipRight = kotlin.math.min(right, destination.right)
                if (clipRight <= clipLeft) return null
                android.graphics.RectF(clipLeft, destination.top, clipRight, destination.bottom)
            }
        }
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
