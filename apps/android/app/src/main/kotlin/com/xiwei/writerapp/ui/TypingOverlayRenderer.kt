package com.xiwei.writerapp.ui

import android.animation.ValueAnimator
import android.graphics.Canvas
import android.graphics.Paint
import android.text.style.ForegroundColorSpan
import android.util.Log
import android.util.TypedValue
import android.widget.EditText
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.math.sqrt

data class OverlayAnim(
    val insertedStart: Int,
    val insertedText: String,
    val startX: Float,
    val startY: Float,
    var progress: Float,
    val animator: ValueAnimator,
    val span: ForegroundColorSpan
) {
    val codePoints: List<Int> = buildList {
        var i = 0
        while (i < insertedText.length) {
            val cp = Character.codePointAt(insertedText, i)
            add(cp)
            i += Character.charCount(cp)
        }
    }
}

class TypingOverlayRenderer(private val editText: EditText) {
    private val DEBUG_ANIM = false
    private val TAG = "WriterEditorAnim"
    private val activeAnims = CopyOnWriteArrayList<OverlayAnim>()
    private val MAX_ANIMATIONS = 24

    fun addAnim(anim: OverlayAnim) {
        if (activeAnims.size >= MAX_ANIMATIONS) {
            val oldest = activeAnims.removeAt(0)
            oldest.animator.cancel()
            editText.text?.removeSpan(oldest.span)
        }
        activeAnims.add(anim)
    }

    fun removeAnim(anim: OverlayAnim) {
        activeAnims.remove(anim)
    }

    fun clear() {
        for (anim in activeAnims) {
            anim.animator.cancel()
            editText.text?.removeSpan(anim.span)
        }
        activeAnims.clear()
    }

    fun onDraw(canvas: Canvas) {
        if (activeAnims.isEmpty()) return

        val layout = editText.layout ?: return
        val paint = editText.paint
        val originalAlpha = paint.alpha
        val textLength = editText.text?.length ?: 0

        val padX = editText.compoundPaddingLeft.toFloat()
        val padY = editText.compoundPaddingTop.toFloat()

        for (anim in activeAnims) {
            var i = anim.insertedStart
            var drawnCodepoints = 0
            var skippedNewlines = 0

            for (cp in anim.codePoints) {
                val charCount = Character.charCount(cp)
                if (i + charCount > textLength) break

                val isNewline = (cp == '\n'.code || cp == '\r'.code)

                if (isNewline) {
                    skippedNewlines++
                    i += charCount
                    continue
                }

                val textToDraw = String(Character.toChars(cp))
                val destX = layout.getPrimaryHorizontal(i)
                val line = layout.getLineForOffset(i)
                val destY = layout.getLineBaseline(line).toFloat()

                // Calculate animation start position
                var sX = anim.startX
                var sY = anim.startY

                if (sX < 0 || sY < 0) {
                    sX = destX
                    sY = destY
                } else {
                    val dx = destX - sX
                    val dy = destY - sY
                    val distSq = dx * dx + dy * dy
                    val maxDist = 80f
                    if (distSq > maxDist * maxDist) {
                        val dist = sqrt(distSq.toDouble()).toFloat()
                        sX = destX - (dx / dist) * maxDist
                        sY = destY - (dy / dist) * maxDist
                    }
                }

                val currentX = sX + (destX - sX) * anim.progress
                val currentY = sY + (destY - sY) * anim.progress

                paint.alpha = (originalAlpha * anim.progress).toInt().coerceIn(0, 255)
                canvas.drawText(
                    textToDraw,
                    currentX + padX,
                    currentY + padY,
                    paint
                )

                drawnCodepoints++
                i += charCount
            }

            if (DEBUG_ANIM) {
                Log.d(TAG, "onDraw - insertedStart: ${anim.insertedStart}, visibleAnimatedCodepoints: $drawnCodepoints, skippedNewlines: $skippedNewlines, overlayCount: ${activeAnims.size}")
            }
        }

        paint.alpha = originalAlpha
    }
}
