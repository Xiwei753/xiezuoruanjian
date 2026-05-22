package com.xiwei.writerapp.ui

import android.graphics.Canvas
import android.graphics.Paint
import android.text.style.ForegroundColorSpan
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.math.sqrt

data class OverlayAnim(
    val insertedStart: Int,
    val insertedText: String,
    val startX: Float,
    val startY: Float,
    var progress: Float = 0f,
    var startTimeNanos: Long = -1L,
    val durationMs: Long,
    val hiddenSpan: ForegroundColorSpan? = null
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

class TypingOverlayRenderer(private val editText: WriterEditText) : EditorAnimationRuntime.Animatable {
    private val DEBUG_ANIM = false
    private val TAG = "WriterEditorAnim"
    private val activeAnims = CopyOnWriteArrayList<OverlayAnim>()
    private val MAX_ANIMATIONS = 24

    fun addAnim(anim: OverlayAnim) {
        if (activeAnims.size >= MAX_ANIMATIONS) {
            val oldest = activeAnims.removeAt(0)
            removeSpan(oldest)
        }
        activeAnims.add(anim)
        editText.animationRuntime?.register(this)
    }

    private fun removeSpan(anim: OverlayAnim) {
        val span = anim.hiddenSpan ?: return
        val editable = editText.text ?: return
        if (editable.getSpanStart(span) >= 0) {
            editText.isUpdatingSpanWrapper = true
            editable.removeSpan(span)
            editText.isUpdatingSpanWrapper = false
        }
    }

    fun removeAnim(anim: OverlayAnim) {
        removeSpan(anim)
        activeAnims.remove(anim)
        if (activeAnims.isEmpty()) {
            editText.animationRuntime?.unregister(this)
        }
    }

    fun clear() {
        for (anim in activeAnims) {
            removeSpan(anim)
        }
        activeAnims.clear()
        editText.animationRuntime?.unregister(this)
    }

    override fun onAnimationStep(frameTimeNanos: Long): Boolean {
        if (activeAnims.isEmpty()) return false

        val iterator = activeAnims.iterator()
        var hasMore = false
        val currentEditable = editText.text

        while (iterator.hasNext()) {
            val anim = iterator.next()
            if (anim.startTimeNanos == -1L) {
                anim.startTimeNanos = frameTimeNanos
            }
            val elapsedNanos = frameTimeNanos - anim.startTimeNanos
            val animDurationNanos = anim.durationMs * 1_000_000f
            if (animDurationNanos <= 0) {
                anim.progress = 1f
            } else {
                anim.progress = (elapsedNanos / animDurationNanos).coerceIn(0f, 1f)
            }

            if (anim.progress >= 1f) {
                removeSpan(anim)
                activeAnims.remove(anim)
            } else {
                hasMore = true
            }
        }

        if (currentEditable != null) {
            val invalidAnims = activeAnims.filter { anim ->
                val span = anim.hiddenSpan
                if (span != null) {
                    val start = currentEditable.getSpanStart(span)
                    val end = currentEditable.getSpanEnd(span)
                    start < 0 || end < 0 || (end - start) != anim.insertedText.length
                } else {
                    false
                }
            }
            if (invalidAnims.isNotEmpty()) {
                for (invalid in invalidAnims) {
                    removeSpan(invalid)
                }
                activeAnims.removeAll(invalidAnims)
            }
        }

        return hasMore || activeAnims.isNotEmpty()
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

            // Apply decelerate interpolation dynamically
            val interpolatedProgress = 1f - (1f - anim.progress) * (1f - anim.progress)

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

                val currentX = sX + (destX - sX) * interpolatedProgress
                val currentY = sY + (destY - sY) * interpolatedProgress

                paint.alpha = (originalAlpha * interpolatedProgress).toInt().coerceIn(0, 255)
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
                android.util.Log.d(TAG, "onDraw - insertedStart: ${anim.insertedStart}, visibleAnimatedCodepoints: $drawnCodepoints, skippedNewlines: $skippedNewlines, overlayCount: ${activeAnims.size}")
            }
        }

        paint.alpha = originalAlpha
    }
}
