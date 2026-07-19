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

    fun computeAnimatedSliceRegions(transaction: PreparedVisualTransaction): List<android.graphics.RectF> {
        val regions = mutableListOf<android.graphics.RectF>()
        for (slice in transaction.animatedSlices) {
            val srcRect = slice.sourceRect
            if (srcRect.width() <= 0 || srcRect.height() <= 0) continue
            val fromRect = slice.fromDestinationRect ?: slice.destinationRect
            val unionLeft = minOf(fromRect.left, slice.destinationRect.left)
            val unionTop = minOf(fromRect.top, slice.destinationRect.top)
            val unionRight = maxOf(fromRect.right, slice.destinationRect.right)
            val unionBottom = maxOf(fromRect.bottom, slice.destinationRect.bottom)
            regions.add(android.graphics.RectF(unionLeft, unionTop, unionRight, unionBottom))
        }
        return regions
    }
}
