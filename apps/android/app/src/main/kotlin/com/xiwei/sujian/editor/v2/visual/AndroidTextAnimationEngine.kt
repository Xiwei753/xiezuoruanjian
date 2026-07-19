package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.mirror.VisualIntent

class AndroidTextAnimationEngine(
    private val visualPlanner: AndroidVisualPlanner,
    private val resourceStore: VisualResourceStore
) {
    private var activeTransaction: PreparedVisualTransaction? = null
    private var timeline: AnimationTimeline? = null
    private var animationPolicy: TextAnimationPolicy = TextAnimationPolicy.INHERIT_GLOBAL

    fun prepare(
        visualIntent: VisualIntent,
        oldRevision: AndroidLayoutRevision?,
        newRevision: AndroidLayoutRevision?,
        oldSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        newSnapshots: Map<Int, AndroidLineSnapshot> = emptyMap(),
        rebaseFrame: VisualFrameSnapshot? = null
    ): PreparedVisualTransaction {
        val transactionKey = System.nanoTime()
        val owner = SnapshotOwner.OwnedByTransaction(transactionKey)
        val ownedSnapshotIds = mutableSetOf<Long>()
        val snapshotLookup = mutableMapOf<Long, AndroidLineSnapshot>()
        for ((_, snapshot) in oldSnapshots) {
            resourceStore.put(snapshot, owner)
            ownedSnapshotIds.add(snapshot.snapshotId)
            snapshotLookup[snapshot.snapshotId] = snapshot
        }
        for ((_, snapshot) in newSnapshots) {
            resourceStore.put(snapshot, owner)
            ownedSnapshotIds.add(snapshot.snapshotId)
            snapshotLookup[snapshot.snapshotId] = snapshot
        }
        if (rebaseFrame != null) {
            for (state in rebaseFrame.sliceVisualStates) {
                val snapshot = resourceStore.get(state.snapshotId)
                if (snapshot != null) {
                    snapshotLookup[state.snapshotId] = snapshot
                }
            }
        }
        return visualPlanner.prepare(
            visualIntent, oldRevision, newRevision,
            oldSnapshots, newSnapshots, rebaseFrame, transactionKey, ownedSnapshotIds,
            snapshotLookup
        )
    }

    fun submit(preparedAnimation: PreparedVisualTransaction) {
        val oldTransaction = activeTransaction

        if (oldTransaction != null) {
            val inheritedIds = oldTransaction.ownedSnapshotIds - preparedAnimation.ownedSnapshotIds
            val newOwner = SnapshotOwner.OwnedByTransaction(preparedAnimation.transactionId)
            for (snapshotId in inheritedIds) {
                resourceStore.transferOwnership(snapshotId, newOwner)
            }
            val mergedIds = if (inheritedIds.isNotEmpty()) {
                preparedAnimation.ownedSnapshotIds + inheritedIds
            } else {
                preparedAnimation.ownedSnapshotIds
            }
            activeTransaction = if (mergedIds != preparedAnimation.ownedSnapshotIds) {
                preparedAnimation.copy(ownedSnapshotIds = mergedIds)
            } else {
                preparedAnimation
            }
            timeline?.complete()
        } else {
            activeTransaction = preparedAnimation
        }

        timeline = AnimationTimeline(preparedAnimation.durationMs)
    }

    fun prepareAndSubmit(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)? = null,
        beforePatch: (() -> Unit)? = null
    ) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val rebaseSnapshot = captureRebaseSnapshot(frameTimeMs)

        val oldRevision = layoutEngine.captureImmutableRevision()
        val affectedOldLineIndices = visualPlanner.computeAffectedLineIndices(visualIntent, oldRevision, useNewRanges = false)
        val oldSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedOldLineIndices)
        beforePatch?.invoke()
        mirrorUpdate?.invoke()
        layoutEngine.requestLayout()
        val newRevision = layoutEngine.getCurrentRevision()
        val affectedNewLineIndices = visualPlanner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRevision, newRevision)
        val newSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedNewLineIndices)
        val transaction = prepare(visualIntent, oldRevision, newRevision, oldSnapshots, newSnapshots, rebaseSnapshot)
        submit(transaction)
    }

    fun registerSnapshots(snapshots: Map<Int, AndroidLineSnapshot>, owner: SnapshotOwner) {
        for ((_, snapshot) in snapshots) {
            resourceStore.put(snapshot, owner)
        }
    }

    fun captureFrame(frameTimeMs: Long): VisualFrameSnapshot? {
        val transaction = activeTransaction ?: return null
        val tl = timeline ?: return null
        if (tl.getState() != TransactionState.Rendering && tl.getState() != TransactionState.Paused) return null
        val p = tl.progress(frameTimeMs)
        val sliceStates = computeSliceVisualStates(transaction, p)
        val cursorRect = computeCurrentCursorRect(transaction, p)
        return VisualFrameSnapshot(
            progress = p,
            state = tl.getState(),
            sliceVisualStates = sliceStates,
            cursorRect = cursorRect
        )
    }

    fun captureRebaseSnapshot(frameTimeMs: Long): VisualFrameSnapshot? {
        return captureFrame(frameTimeMs)
    }

    fun hasActiveAnimation(): Boolean = activeTransaction != null && timeline != null

    fun cancel() {
        val transaction = activeTransaction ?: return
        cancelTransaction(transaction)
        activeTransaction = null
        timeline = null
    }

    fun resetForSession() {
        cancel()
        resourceStore.releaseAll()
    }

    fun release() {
        cancel()
        resourceStore.releaseAll()
    }

    fun setAnimationPolicy(policy: TextAnimationPolicy) {
        animationPolicy = policy
    }

    fun getAnimationPolicy(): TextAnimationPolicy = animationPolicy

    fun getActiveTransaction(): PreparedVisualTransaction? = activeTransaction

    fun getTimelineProgress(frameTimeMs: Long): Float {
        val tl = timeline ?: return 0f
        return tl.progress(frameTimeMs)
    }

    fun isTimelineCompleted(frameTimeMs: Long): Boolean {
        val tl = timeline ?: return true
        return tl.isCompleted(frameTimeMs)
    }

    fun markFirstVisibleFrame(frameTimeMs: Long) {
        timeline?.markFirstVisibleFrame(frameTimeMs)
    }

    fun completeIfFinished(frameTimeMs: Long): Boolean {
        val transaction = activeTransaction ?: return false
        val tl = timeline ?: return false
        if (tl.isCompleted(frameTimeMs)) {
            completeTransaction(transaction)
            activeTransaction = null
            timeline = null
            return true
        }
        return false
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
                lineIndex = slice.snapshot?.lineIndex ?: -1,
                documentByteStart = slice.snapshot?.documentByteStart ?: -1,
                documentByteEndExclusive = slice.snapshot?.documentByteEndExclusive ?: -1,
                clusterByteStart = slice.clusterByteStart,
                clusterByteEndExclusive = slice.clusterByteEndExclusive,
                currentLeft = currentLeft,
                currentTop = currentTop,
                currentRight = currentRight,
                currentBottom = currentBottom,
                currentAlpha = currentAlpha,
                destinationLeft = slice.destinationRect.left,
                destinationTop = slice.destinationRect.top,
                destinationRight = slice.destinationRect.right,
                destinationBottom = slice.destinationRect.bottom
            )
        }
    }

    private fun completeTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.complete()
    }

    private fun cancelTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.cancel()
    }

    private fun computeCurrentCursorRect(
        transaction: PreparedVisualTransaction,
        progress: Float
    ): android.graphics.RectF? {
        val ct = transaction.cursorTransition ?: return null
        if (!ct.shouldAnimate) return null
        val currentX = ct.fromX + (ct.toX - ct.fromX) * progress
        val currentY = ct.fromY + (ct.toY - ct.fromY) * progress
        val currentHeight = ct.fromHeight + (ct.toHeight - ct.fromHeight) * progress
        return android.graphics.RectF(currentX, currentY, currentX + 2f, currentY + currentHeight)
    }
}

enum class TextAnimationPolicy {
    INHERIT_GLOBAL,
    ENABLED,
    SYSTEM_SUPPRESSED
}
