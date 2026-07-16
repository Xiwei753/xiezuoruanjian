package com.xiwei.sujian.editor.v2.render

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.AnimationTimeline
import com.xiwei.sujian.editor.v2.visual.SliceRole
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.TransactionState

class AndroidRenderFrame(
    val transaction: PreparedVisualTransaction?,
    val progress: Float,
    val viewportWidth: Int,
    val viewportHeight: Int,
    val scrollX: Float,
    val scrollY: Float
)

class AndroidRenderer(
    private val mirror: DisplayTextMirror,
    private val layoutEngine: AndroidLayoutEngine,
    private val resourceStore: VisualResourceStore
) {
    private var activeTransaction: PreparedVisualTransaction? = null
    private var timeline: AnimationTimeline? = null
    private val cursorPaint = Paint().apply {
        color = Color.BLACK
        strokeWidth = 2f
        isAntiAlias = true
    }
    private val selectionPaint = Paint().apply {
        color = Color.argb(51, 0, 0, 255)
        style = Paint.Style.FILL
        isAntiAlias = true
    }

    fun submitTransaction(transaction: PreparedVisualTransaction?) {
        if (transaction == null) return

        val oldTransaction = activeTransaction
        if (oldTransaction != null) {
            cancelTransaction(oldTransaction)
        }

        activeTransaction = transaction
        timeline = AnimationTimeline(transaction.durationMs)
    }

    fun renderFrame(canvas: Canvas, frameTimeMs: Long) {
        val transaction = activeTransaction
        val tl = timeline

        canvas.drawColor(Color.WHITE)

        val layout = layoutEngine.getLayout()
        if (layout != null) {
            if (transaction != null && tl != null && transaction.animatedSlices.isNotEmpty()) {
                tl.markFirstVisibleFrame(frameTimeMs)
                val progress = tl.progress(frameTimeMs)

                renderStaticBackground(canvas, layout, transaction)
                renderAnimatedSlices(canvas, transaction, progress)
                renderCursorTransition(canvas, transaction, progress)

                if (tl.isCompleted(frameTimeMs)) {
                    completeTransaction(transaction)
                    activeTransaction = null
                    timeline = null
                }
            } else {
                layout.draw(canvas)
                renderCursor(canvas, layout)
            }
        }
    }

    private fun renderStaticBackground(
        canvas: Canvas,
        layout: android.text.Layout,
        transaction: PreparedVisualTransaction
    ) {
        val affectedLines = mutableSetOf<Int>()
        for (slice in transaction.animatedSlices) {
            slice.snapshot?.lineIndex?.let { affectedLines.add(it) }
        }

        for (i in 0 until layout.lineCount) {
            if (i in affectedLines) continue
            val lineTop = layout.getLineTop(i)
            val lineBottom = layout.getLineBottom(i)
            canvas.save()
            canvas.clipRect(
                layout.getLineLeft(i), lineTop.toFloat(),
                layout.getLineRight(i), lineBottom.toFloat()
            )
            layout.draw(canvas)
            canvas.restore()
        }
    }

    private fun renderAnimatedSlices(canvas: Canvas, transaction: PreparedVisualTransaction, progress: Float) {
        for (slice in transaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            val bitmap = snapshot.bitmap ?: continue
            val alpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress

            val paint = Paint().apply {
                this.alpha = (alpha * 255).toInt().coerceIn(0, 255)
                isAntiAlias = true
            }

            when (slice.role) {
                SliceRole.Move -> {
                    val fromRect = slice.fromDestinationRect ?: slice.destinationRect
                    val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * progress
                    val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * progress
                    val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * progress
                    val currentBottom = fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * progress
                    val currentDest = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom)
                    canvas.drawBitmap(bitmap, slice.sourceRect, currentDest, paint)
                }
                else -> {
                    canvas.drawBitmap(bitmap, slice.sourceRect, slice.destinationRect, paint)
                }
            }
        }
    }

    private fun renderCursorTransition(canvas: Canvas, transaction: PreparedVisualTransaction, progress: Float) {
        val ct = transaction.cursorTransition ?: return
        if (!ct.shouldAnimate) return

        val currentX = ct.fromX + (ct.toX - ct.fromX) * progress
        val currentY = ct.fromY + (ct.toY - ct.fromY) * progress
        val currentHeight = ct.fromHeight + (ct.toHeight - ct.fromHeight) * progress

        canvas.drawRect(currentX, currentY, currentX + 2f, currentY + currentHeight, cursorPaint)
    }

    private fun renderCursor(canvas: Canvas, layout: android.text.Layout) {
        val cursorUtf16 = mirror.getCursorUtf16()
        if (cursorUtf16 < 0 || cursorUtf16 > mirror.getLengthUtf16()) return

        val line = layout.getLineForOffset(cursorUtf16)
        val x = layout.getPrimaryHorizontal(cursorUtf16)
        val top = layout.getLineTop(line).toFloat()
        val bottom = layout.getLineBottom(line).toFloat()

        canvas.drawRect(x, top, x + 2f, bottom, cursorPaint)
    }

    private fun completeTransaction(transaction: PreparedVisualTransaction) {
        for (slice in transaction.animatedSlices) {
            slice.snapshot?.let { resourceStore.release(it.snapshotId) }
        }
        timeline?.complete()
    }

    private fun cancelTransaction(transaction: PreparedVisualTransaction) {
        for (slice in transaction.animatedSlices) {
            slice.snapshot?.let { resourceStore.release(it.snapshotId) }
        }
        timeline?.cancel()
    }

    fun hasActiveAnimation(): Boolean = activeTransaction != null && timeline != null
}
