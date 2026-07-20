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
                    //
                    // sourceRect is always from the NEW layout's Bitmap (the slice's snapshot
                    // belongs to the new revision). This is correct because the new Bitmap
                    // contains the actual glyph pixels at the destination position — the old
                    // Bitmap may have different sub-pixel rendering or font fallback.
                    val fromRect = slice.fromDestinationRect ?: slice.destinationRect
                    val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * progress
                    val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * progress
                    val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * progress
                    val currentBottom = fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * progress
                    val currentDest = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom)
                    canvas.drawBitmap(bitmap, slice.sourceRect, currentDest, slicePaint)
                }
                // Insert/Delete/CrossfadeOld/CrossfadeNew: position does not change during
                // animation — only alpha varies. The bitmap is drawn at its final destination
                // with interpolated alpha; no positional interpolation is needed.
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
