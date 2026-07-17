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

        if (frameSnapshot.progress > 0f) {
            for (slice in newTransaction.animatedSlices) {
                if (slice.role == SliceRole.Move && slice.fromDestinationRect != null) {
                    val currentLeft = slice.fromDestinationRect.left + (slice.destinationRect.left - slice.fromDestinationRect.left) * frameSnapshot.progress
                    val currentTop = slice.fromDestinationRect.top + (slice.destinationRect.top - slice.fromDestinationRect.top) * frameSnapshot.progress
                    val currentRight = slice.fromDestinationRect.right + (slice.destinationRect.right - slice.fromDestinationRect.right) * frameSnapshot.progress
                    val currentBottom = slice.fromDestinationRect.bottom + (slice.destinationRect.bottom - slice.fromDestinationRect.bottom) * frameSnapshot.progress
                    slice.fromDestinationRect = android.graphics.RectF(currentLeft, currentTop, currentRight, currentBottom)
                } else if (slice.role == SliceRole.Insert) {
                    slice.startAlpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * frameSnapshot.progress
                } else if (slice.role == SliceRole.Delete) {
                    slice.endAlpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * frameSnapshot.progress
                }
            }
        }

        oldTimeline.cancel()
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
                scrollY = scrollY
            )
        }

        return AndroidRenderFrame(
            transaction = null,
            progress = 0f,
            viewportWidth = viewportWidth,
            viewportHeight = viewportHeight,
            scrollX = scrollX,
            scrollY = scrollY
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
