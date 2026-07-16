package com.xiwei.sujian.editor.v2.render

import android.graphics.Canvas
import android.graphics.Paint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.AnimationTimeline
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore

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

        canvas.save()

        val layout = layoutEngine.getLayout()
        if (layout != null) {
            layout.draw(canvas)
        }

        if (transaction != null && tl != null) {
            tl.markFirstVisibleFrame(frameTimeMs)
            val progress = tl.progress(frameTimeMs)

            renderAnimatedSlices(canvas, transaction, progress)
            renderCursorTransition(canvas, transaction, progress)

            if (tl.isCompleted(frameTimeMs)) {
                completeTransaction(transaction)
                activeTransaction = null
                timeline = null
            }
        }

        canvas.restore()
    }

    private fun renderAnimatedSlices(canvas: Canvas, transaction: PreparedVisualTransaction, progress: Float) {
        for (slice in transaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            val bitmap = snapshot.bitmap ?: continue
            val alpha = (slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress)

            val paint = Paint().apply {
                this.alpha = (alpha * 255).toInt().coerceIn(0, 255)
            }

            canvas.drawBitmap(
                bitmap,
                slice.sourceRect,
                slice.destinationRect,
                paint
            )
        }
    }

    private fun renderCursorTransition(canvas: Canvas, transaction: PreparedVisualTransaction, progress: Float) {
        val ct = transaction.cursorTransition ?: return
        if (!ct.shouldAnimate) return

        val currentX = ct.fromX + (ct.toX - ct.fromX) * progress
        val currentY = ct.fromY + (ct.toY - ct.fromY) * progress
        val currentHeight = ct.fromHeight + (ct.toHeight - ct.fromHeight) * progress

        val cursorPaint = Paint().apply {
            color = android.graphics.Color.BLACK
            strokeWidth = 2f
        }
        canvas.drawRect(currentX, currentY, currentX + 2f, currentY + currentHeight, cursorPaint)
    }

    private fun completeTransaction(transaction: PreparedVisualTransaction) {
        for (patch in transaction.staticPatches) {
            resourceStore.release(patch.newSnapshotId)
        }
    }

    private fun cancelTransaction(transaction: PreparedVisualTransaction) {
        for (slice in transaction.animatedSlices) {
            slice.snapshot?.let { resourceStore.release(it.snapshotId) }
        }
    }
}
