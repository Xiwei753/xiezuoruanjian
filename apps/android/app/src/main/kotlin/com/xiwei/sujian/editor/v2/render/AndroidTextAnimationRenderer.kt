package com.xiwei.sujian.editor.v2.render

import android.graphics.Canvas
import android.graphics.Paint
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.SliceRole

class AndroidTextAnimationRenderer {
    private val slicePaint = Paint().apply {
        isAntiAlias = true
    }

    fun drawAnimatedSlices(
        canvas: Canvas,
        transaction: PreparedVisualTransaction,
        progress: Float
    ) {
        for (slice in transaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            val bitmap = snapshot.bitmap ?: continue
            val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress
            slicePaint.alpha = (alpha * 255).toInt().coerceIn(0, 255)

            when (slice.role) {
                SliceRole.Move -> {
                    // fromDestinationRect is the pre-move position; destinationRect is the
                    // post-move position. For cross-line Moves these can be on different lines;
                    // the interpolation moves the bitmap smoothly between them.
                    val fromRect = slice.fromDestinationRect ?: slice.destinationRect
                    val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * progress
                    val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * progress
                    val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * progress
                    val currentBottom = fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * progress
                    val currentDest = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom)
                    canvas.drawBitmap(bitmap, slice.sourceRect, currentDest, slicePaint)
                }
                else -> {
                    canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, slicePaint)
                }
            }
        }
    }

    fun drawAnimatedCursor(
        canvas: Canvas,
        transaction: PreparedVisualTransaction,
        progress: Float,
        cursorPaint: Paint
    ) {
        val ct = transaction.cursorTransition ?: return
        if (!ct.shouldAnimate) return

        val currentX = ct.fromX + (ct.toX - ct.fromX) * progress
        val currentY = ct.fromY + (ct.toY - ct.fromY) * progress
        val currentHeight = ct.fromHeight + (ct.toHeight - ct.fromHeight) * progress

        canvas.drawRect(currentX, currentY, currentX + 2f, currentY + currentHeight, cursorPaint)
    }

    /**
     * Collect the destination regions of all animated slices for static text hole-punching.
     *
     * Constraint: each region must be as small as possible — ideally the exact cluster
     * bounding box — because the static renderer clips out these regions entirely.
     * When a Move slice spans from one line to the next, merging source and destination
     * into a single bounding rect can cover nearly two full lines, causing non-animated
     * text in between to disappear during the animation.
     *
     * Currently returns only [slice.destinationRect]; for cross-line Moves this is the
     * target position only (not the source), which is safe. Source-position holes are
     * intentionally NOT punched because they would require separate small rects per source
     * cluster — a merged source+destination bounding rect would swallow entire lines of
     * non-animated text between the source and destination positions.
     */
    fun computeAnimatedSliceRegions(transaction: PreparedVisualTransaction): List<android.graphics.RectF> {
        val regions = mutableListOf<android.graphics.RectF>()
        for (slice in transaction.animatedSlices) {
            val srcRect = slice.sourceRect
            if (srcRect.width() <= 0 || srcRect.height() <= 0) continue
            regions.add(android.graphics.RectF(slice.destinationRect))
        }
        return regions
    }
}
