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
                    val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress
                    slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)
                    val fromRect = slice.fromDestinationRect ?: slice.destinationRect
                    val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * progress
                    val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * progress
                    val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * progress
                    val currentBottom = fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * progress
                    val currentDest = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom)
                    canvas.drawBitmap(bitmap, slice.sourceRect, currentDest, slicePaint)
                }
                SliceRole.Insert, SliceRole.Delete -> {
                    drawRevealSlice(canvas, bitmap, slice, progress)
                }
                // CrossfadeOld/CrossfadeNew/Static: position does not change during
                // animation — only alpha varies. The bitmap is drawn at its final destination
                // with interpolated alpha; no positional interpolation is needed.
                SliceRole.CrossfadeOld, SliceRole.CrossfadeNew, SliceRole.Static -> {
                    val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress
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
     */
    fun drawAnimatedCursor(
        canvas: Canvas,
        transition: PreparedVisualTransaction.CursorTransition,
        progress: Float,
        cursorPaint: Paint,
    ) {
        if (!transition.shouldAnimate) return

        val currentX = transition.fromX + (transition.toX - transition.fromX) * progress
        val currentY = transition.fromY + (transition.toY - transition.fromY) * progress
        val currentHeight = transition.fromHeight + (transition.toHeight - transition.fromHeight) * progress

        canvas.drawRect(currentX, currentY, currentX + 2f, currentY + currentHeight, cursorPaint)
    }

    /**
     * Collect the destination regions of animated slices for static text hole-punching.
     *
     * Only "appearing" roles (Insert, Move, CrossfadeNew) punch holes in the static
     * new-layout text. These slices draw new-layout content at [slice.destinationRect],
     * so the static renderer must not also draw there (double-rendering).
     *
     * "Disappearing" roles (Delete, CrossfadeOld) do NOT punch holes. Their
     * [slice.destinationRect] is the *old* position; the new layout may have shifted
     * text there that must remain visible behind the fading-out slice. Punching a hole
     * would hide that shifted text, creating a gap once the fade completes.
     *
     * Constraint: each region must be as small as possible — ideally the exact cluster
     * bounding box. For Move slices, ONLY [slice.destinationRect] (the target position)
     * punches a hole — [slice.fromDestinationRect] (the source/pre-move position) is NOT
     * punched because the new layout's text there should remain visible and will be
     * gradually revealed as the slice slides away.
     *
     * DO NOT merge fromDestinationRect and destinationRect into a single bounding rect.
     * For cross-line Moves, the source and target can be on different visual lines; their
     * axis-aligned bounding rect would cover nearly two full lines, erasing non-animated
     * text in between during the animation. Each position must remain an independent hole.
     */
    fun computeAnimatedSliceRegions(transaction: PreparedVisualTransaction): List<android.graphics.RectF> {
        val regions = mutableListOf<android.graphics.RectF>()
        for (slice in transaction.animatedSlices) {
            if (slice.role == SliceRole.Delete || slice.role == SliceRole.CrossfadeOld) continue
            val srcRect = slice.sourceRect
            if (srcRect.width() <= 0 || srcRect.height() <= 0) continue
            regions.add(android.graphics.RectF(slice.destinationRect))
        }
        return regions
    }
}
