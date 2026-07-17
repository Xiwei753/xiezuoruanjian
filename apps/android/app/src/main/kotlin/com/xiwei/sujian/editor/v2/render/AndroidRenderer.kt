package com.xiwei.sujian.editor.v2.render

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.visual.AnimationTimeline
import com.xiwei.sujian.editor.v2.visual.SliceRole
import com.xiwei.sujian.editor.v2.visual.SnapshotOwner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.TransactionState
import com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot

class AndroidRenderFrame(
    val transaction: PreparedVisualTransaction?,
    val progress: Float,
    val viewportWidth: Int,
    val viewportHeight: Int,
    val scrollX: Float,
    val scrollY: Float
)

class VisualTransactionCoordinator(
    private val resourceStore: VisualResourceStore
) {
    private var activeTransaction: PreparedVisualTransaction? = null
    private var timeline: AnimationTimeline? = null

    fun submitTransaction(transaction: PreparedVisualTransaction?) {
        if (transaction == null) return

        val oldTransaction = activeTransaction
        val oldTimeline = timeline

        if (oldTransaction != null && oldTimeline != null) {
            val frameTimeMs = System.nanoTime() / 1_000_000
            val frameSnapshot = oldTimeline.currentVisualFrame(frameTimeMs)

            if (frameSnapshot != null && frameSnapshot.state == TransactionState.Rendering) {
                rebaseFromOldTransaction(oldTransaction, frameSnapshot, transaction, oldTimeline)
            } else {
                cancelTransaction(oldTransaction)
            }
        }

        activeTransaction = transaction
        timeline = AnimationTimeline(transaction.durationMs)
    }

    private fun rebaseFromOldTransaction(
        oldTransaction: PreparedVisualTransaction,
        frameSnapshot: VisualFrameSnapshot,
        newTransaction: PreparedVisualTransaction,
        oldTimeline: AnimationTimeline?
    ) {
        val newOwner = SnapshotOwner.OwnedByTransaction(newTransaction.transactionId)
        val oldOwner = SnapshotOwner.OwnedByTransaction(oldTransaction.transactionId)

        for (slice in oldTransaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            val currentOwner = resourceStore.getOwner(snapshot.snapshotId)
            if (currentOwner == oldOwner) {
                resourceStore.transferOwnership(snapshot.snapshotId, newOwner)
            }
        }

        for (slice in oldTransaction.animatedSlices) {
            val snapshot = slice.snapshot ?: continue
            if (!newTransaction.animatedSlices.any { it.snapshot?.snapshotId == snapshot.snapshotId }) {
                resourceStore.release(snapshot.snapshotId, newOwner)
            }
        }

        oldTimeline.cancel()
    }

    fun computeFrame(frameTimeMs: Long): AndroidRenderFrame {
        val transaction = activeTransaction
        val tl = timeline

        if (transaction != null && tl != null && transaction.animatedSlices.isNotEmpty()) {
            tl.markFirstVisibleFrame(frameTimeMs)
            val progress = tl.progress(frameTimeMs)

            if (tl.isCompleted(frameTimeMs)) {
                completeTransaction(transaction)
                activeTransaction = null
                timeline = null
            }

            return AndroidRenderFrame(
                transaction = transaction,
                progress = progress,
                viewportWidth = 0,
                viewportHeight = 0,
                scrollX = 0f,
                scrollY = 0f
            )
        }

        return AndroidRenderFrame(
            transaction = null,
            progress = 0f,
            viewportWidth = 0,
            viewportHeight = 0,
            scrollX = 0f,
            scrollY = 0f
        )
    }

    private fun completeTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (slice in transaction.animatedSlices) {
            slice.snapshot?.let { resourceStore.release(it.snapshotId, owner) }
        }
        timeline?.complete()
    }

    private fun cancelTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (slice in transaction.animatedSlices) {
            slice.snapshot?.let { resourceStore.release(it.snapshotId, owner) }
        }
        timeline?.cancel()
    }

    fun hasActiveAnimation(): Boolean = activeTransaction != null && timeline != null
}

class AndroidRenderer(
    private val mirror: DisplayTextMirror,
    private val layoutEngine: AndroidLayoutEngine,
    private val resourceStore: VisualResourceStore
) {
    private val coordinator = VisualTransactionCoordinator(resourceStore)
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
    private val preeditUnderlinePaint = Paint().apply {
        color = Color.BLACK
        strokeWidth = 2f
        isAntiAlias = true
    }
    private val slicePaint = Paint().apply {
        isAntiAlias = true
    }

    fun submitTransaction(transaction: PreparedVisualTransaction?) {
        coordinator.submitTransaction(transaction)
    }

    fun renderFrame(canvas: Canvas, frameTimeMs: Long) {
        val frame = coordinator.computeFrame(frameTimeMs)

        canvas.drawColor(Color.WHITE)

        val layout = layoutEngine.getLayout()
        if (layout != null) {
            val transaction = frame.transaction
            if (transaction != null && transaction.animatedSlices.isNotEmpty()) {
                renderStaticBackground(canvas, layout, transaction)
                renderSelectionDecoration(canvas, layout, transaction)
                renderAnimatedSlices(canvas, transaction, frame.progress)
                renderPreeditDecoration(canvas, layout, transaction)
                renderCursorTransition(canvas, transaction, frame.progress)
            } else {
                renderSelectionHighlight(canvas, layout)
                layout.draw(canvas)
                renderPreeditUnderline(canvas, layout)
                renderCursor(canvas, layout)
            }
        }
    }

    private fun renderStaticBackground(
        canvas: Canvas,
        layout: android.text.Layout,
        transaction: PreparedVisualTransaction
    ) {
        val hiddenLines = mutableSetOf<Int>()
        for (patch in transaction.staticPatches) {
            if (patch.visibleSourceRects.isEmpty()) {
                hiddenLines.add(patch.lineIndex)
            }
        }
        for (slice in transaction.animatedSlices) {
            slice.snapshot?.lineIndex?.let { hiddenLines.add(it) }
        }

        for (i in 0 until layout.lineCount) {
            if (i in hiddenLines) continue
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

    private fun renderSelectionDecoration(
        canvas: Canvas,
        layout: android.text.Layout,
        transaction: PreparedVisualTransaction
    ) {
        val sel = transaction.selectionDecoration ?: return
        for (rect in sel.rects) {
            canvas.drawRect(rect, selectionPaint)
        }
    }

    private fun renderPreeditDecoration(
        canvas: Canvas,
        layout: android.text.Layout,
        transaction: PreparedVisualTransaction
    ) {
        val preedit = transaction.preeditDecoration ?: return
        val startLine = layout.getLineForOffset(preedit.startUtf16)
        val endLine = layout.getLineForOffset(preedit.endUtf16)
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) preedit.startUtf16 else layout.getLineStart(line)
            val lineEnd = if (line == endLine) preedit.endUtf16 else layout.getLineEnd(line)
            val startX = layout.getPrimaryHorizontal(lineStart)
            val endX = layout.getPrimaryHorizontal(lineEnd - 1)
            val bottom = layout.getLineBottom(line).toFloat()
            canvas.drawLine(startX, bottom, endX, bottom, preeditUnderlinePaint)
        }
    }

    private fun renderSelectionHighlight(canvas: Canvas, layout: android.text.Layout) {
        val selStart = mirror.getSelectionStartUtf16()
        val selEnd = mirror.getSelectionEndUtf16()
        if (selStart == selEnd) return

        val startLine = layout.getLineForOffset(selStart)
        val endLine = layout.getLineForOffset(selEnd)
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) selStart else layout.getLineStart(line)
            val lineEnd = if (line == endLine) selEnd else layout.getLineEnd(line)
            val top = layout.getLineTop(line).toFloat()
            val bottom = layout.getLineBottom(line).toFloat()
            val left = layout.getPrimaryHorizontal(lineStart)
            val right = layout.getPrimaryHorizontal(lineEnd - 1) + layout.getLineWidth(line)
            canvas.drawRect(
                layout.getLineLeft(line), top,
                layout.getLineRight(line), bottom,
                selectionPaint
            )
        }
    }

    private fun renderPreeditUnderline(canvas: Canvas, layout: android.text.Layout) {
        val compRange = mirror.getCompositionRangeUtf16() ?: return
        val startLine = layout.getLineForOffset(compRange.first)
        val endLine = layout.getLineForOffset(compRange.second)
        for (line in startLine..endLine) {
            val lineStart = if (line == startLine) compRange.first else layout.getLineStart(line)
            val lineEnd = if (line == endLine) compRange.second else layout.getLineEnd(line)
            val startX = layout.getPrimaryHorizontal(lineStart)
            val endX = layout.getPrimaryHorizontal(lineEnd - 1)
            val bottom = layout.getLineBottom(line).toFloat()
            canvas.drawLine(startX, bottom, endX, bottom, preeditUnderlinePaint)
        }
    }

    fun hasActiveAnimation(): Boolean = coordinator.hasActiveAnimation()

    fun setThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int) {
        cursorPaint.color = cursorColor
        selectionPaint.color = selectionColor
        preeditUnderlinePaint.color = preeditColor
    }
}
