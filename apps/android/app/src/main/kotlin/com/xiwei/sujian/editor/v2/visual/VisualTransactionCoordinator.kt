package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.render.AndroidRenderFrame

class VisualTransactionCoordinator(
    private val resourceStore: VisualResourceStore
) {
    private var activeTransaction: PreparedVisualTransaction? = null
    private var timeline: AnimationTimeline? = null
    private var lastFrameSnapshot: VisualFrameSnapshot? = null

    fun submitTransaction(transaction: PreparedVisualTransaction?) {
        if (transaction == null) return

        val oldTransaction = activeTransaction
        val oldTimeline = timeline

        if (oldTransaction != null && oldTimeline != null) {
            val frameTimeMs = System.nanoTime() / 1_000_000
            val frameSnapshot = oldTimeline.currentVisualFrame(frameTimeMs)

            if (frameSnapshot != null && frameSnapshot.state == TransactionState.Rendering) {
                lastFrameSnapshot = frameSnapshot
                val rebased = rebaseFromOldTransaction(oldTransaction, frameSnapshot, transaction, oldTimeline)
                activeTransaction = rebased
            } else {
                cancelTransaction(oldTransaction)
                activeTransaction = transaction
            }
        } else {
            activeTransaction = transaction
        }

        timeline = AnimationTimeline(transaction.durationMs)
    }

    private fun rebaseFromOldTransaction(
        oldTransaction: PreparedVisualTransaction,
        frameSnapshot: VisualFrameSnapshot,
        newTransaction: PreparedVisualTransaction,
        oldTimeline: AnimationTimeline?
    ): PreparedVisualTransaction {
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

        val rebasedSlices = if (frameSnapshot.progress > 0f) {
            newTransaction.animatedSlices.map { slice ->
                when (slice.role) {
                    SliceRole.Move -> {
                        val fromRect = slice.fromDestinationRect ?: slice.destinationRect
                        val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * frameSnapshot.progress
                        val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * frameSnapshot.progress
                        val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * frameSnapshot.progress
                        val currentBottom = fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * frameSnapshot.progress
                        slice.copy(fromDestinationRect = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom))
                    }
                    SliceRole.Insert -> {
                        slice.copy(startAlpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * frameSnapshot.progress)
                    }
                    SliceRole.Delete -> {
                        slice.copy(endAlpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * frameSnapshot.progress)
                    }
                    else -> slice.copy()
                }
            }
        } else {
            newTransaction.animatedSlices.map { it.copy() }
        }

        oldTimeline.cancel()

        return newTransaction.copy(animatedSlices = rebasedSlices)
    }

    fun computeFrame(frameTimeMs: Long, viewportWidth: Int = 0, viewportHeight: Int = 0, scrollX: Float = 0f, scrollY: Float = 0f): AndroidRenderFrame {
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
                viewportWidth = viewportWidth,
                viewportHeight = viewportHeight,
                scrollX = scrollX,
                scrollY = scrollY,
                cursorUtf16 = 0,
                cursorX = 0f,
                cursorY = 0f,
                cursorHeight = 0f,
                selectionStartUtf16 = 0,
                selectionEndUtf16 = 0,
                compositionStartUtf16 = -1,
                compositionEndUtf16 = -1,
                searchHighlightsUtf16 = emptyList()
            )
        }

        return AndroidRenderFrame(
            transaction = null,
            progress = 0f,
            viewportWidth = viewportWidth,
            viewportHeight = viewportHeight,
            scrollX = scrollX,
            scrollY = scrollY,
            cursorUtf16 = 0,
            cursorX = 0f,
            cursorY = 0f,
            cursorHeight = 0f,
            selectionStartUtf16 = 0,
            selectionEndUtf16 = 0,
            compositionStartUtf16 = -1,
            compositionEndUtf16 = -1,
            searchHighlightsUtf16 = emptyList()
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

    fun cancelActiveTransaction() {
        val transaction = activeTransaction ?: return
        cancelTransaction(transaction)
        activeTransaction = null
        timeline = null
    }
}
