package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.render.AndroidRenderFrame

class VisualTransactionCoordinator(
    private val resourceStore: VisualResourceStore
) {
    private var activeTransaction: PreparedVisualTransaction? = null
    private var timeline: AnimationTimeline? = null
    private var pendingRebaseSnapshot: VisualFrameSnapshot? = null

    fun submitTransaction(transaction: PreparedVisualTransaction?) {
        if (transaction == null) return

        val oldTransaction = activeTransaction
        val oldTimeline = timeline

        if (oldTransaction != null && oldTimeline != null) {
            val frameTimeMs = System.nanoTime() / 1_000_000
            val frameSnapshot = oldTimeline.currentVisualFrame(frameTimeMs)

            if (frameSnapshot != null && frameSnapshot.state == TransactionState.Rendering) {
                val sliceStates = computeSliceVisualStates(oldTransaction, frameSnapshot.progress)
                pendingRebaseSnapshot = VisualFrameSnapshot(
                    progress = frameSnapshot.progress,
                    state = frameSnapshot.state,
                    sliceVisualStates = sliceStates
                )
                transferResourcesToNewTransaction(oldTransaction, transaction)
            } else {
                cancelTransaction(oldTransaction)
                pendingRebaseSnapshot = null
            }
        } else {
            pendingRebaseSnapshot = null
        }

        activeTransaction = transaction
        timeline = AnimationTimeline(transaction.durationMs)
    }

    fun takeRebaseSnapshot(): VisualFrameSnapshot? {
        val snapshot = pendingRebaseSnapshot
        pendingRebaseSnapshot = null
        return snapshot
    }

    private fun computeSliceVisualStates(
        transaction: PreparedVisualTransaction,
        progress: Float
    ): List<SliceVisualState> {
        return transaction.animatedSlices.map { slice ->
            val fromRect = slice.fromDestinationRect ?: slice.destinationRect
            val currentLeft = fromRect.left + (slice.destinationRect.left - fromRect.left) * progress
            val currentTop = fromRect.top + (slice.destinationRect.top - fromRect.top) * progress
            val currentRight = fromRect.right + (slice.destinationRect.right - fromRect.right) * progress
            val currentBottom = fromRect.bottom + (slice.destinationRect.bottom - fromRect.bottom) * progress
            val currentAlpha = slice.startAlpha + (slice.endAlpha - slice.startAlpha) * progress
            SliceVisualState(
                snapshotId = slice.snapshot?.snapshotId ?: -1L,
                role = slice.role,
                currentLeft = currentLeft,
                currentTop = currentTop,
                currentRight = currentRight,
                currentBottom = currentBottom,
                currentAlpha = currentAlpha
            )
        }
    }

    private fun transferResourcesToNewTransaction(
        oldTransaction: PreparedVisualTransaction,
        newTransaction: PreparedVisualTransaction
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

        timeline?.cancel()
    }

    fun computeFrame(
        frameTimeMs: Long,
        cursorUtf16: Int,
        cursorX: Float,
        cursorY: Float,
        cursorHeight: Float,
        selectionStartUtf16: Int,
        selectionEndUtf16: Int,
        compositionStartUtf16: Int,
        compositionEndUtf16: Int,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int = 0,
        viewportHeight: Int = 0,
        scrollX: Float = 0f,
        scrollY: Float = 0f
    ): AndroidRenderFrame {
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
                cursorUtf16 = cursorUtf16,
                cursorX = cursorX,
                cursorY = cursorY,
                cursorHeight = cursorHeight,
                selectionStartUtf16 = selectionStartUtf16,
                selectionEndUtf16 = selectionEndUtf16,
                compositionStartUtf16 = compositionStartUtf16,
                compositionEndUtf16 = compositionEndUtf16,
                searchHighlightsUtf16 = searchHighlightsUtf16
            )
        }

        return AndroidRenderFrame(
            transaction = null,
            progress = 0f,
            viewportWidth = viewportWidth,
            viewportHeight = viewportHeight,
            scrollX = scrollX,
            scrollY = scrollY,
            cursorUtf16 = cursorUtf16,
            cursorX = cursorX,
            cursorY = cursorY,
            cursorHeight = cursorHeight,
            selectionStartUtf16 = selectionStartUtf16,
            selectionEndUtf16 = selectionEndUtf16,
            compositionStartUtf16 = compositionStartUtf16,
            compositionEndUtf16 = compositionEndUtf16,
            searchHighlightsUtf16 = searchHighlightsUtf16
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
        pendingRebaseSnapshot = null
    }
}
