package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.mirror.VisualIntent

/**
 * Unified owner of the Android text animation runtime.
 *
 * Holds [AndroidVisualPlanner], [AnimationTimeline], [VisualResourceStore], and the current
 * [PreparedVisualTransaction] with its rebase/continuation state. All animation cancellation,
 * continuation, session switching, and resource release must go through this object — callers
 * must not directly touch [VisualResourceStore] or [AnimationTimeline].
 *
 * Lifecycle: [cancel] is the only legal way to abort an active transaction (releases its
 * snapshots and resets the timeline). [resetForSession] cancels + releases *all* session-owned
 * resources (for session rebind). [release] is equivalent to [resetForSession] and is called
 * when the host is permanently destroyed.
 */
class AndroidTextAnimationEngine(
    private val visualPlanner: AndroidVisualPlanner,
    private val resourceStore: VisualResourceStore
) {
    private var activeTransaction: PreparedVisualTransaction? = null
    private var timeline: AnimationTimeline? = null
    private var animationPolicy: TextAnimationPolicy = TextAnimationPolicy.INHERIT_GLOBAL

    /**
     * Create a prepared visual transaction from old/new layout revisions and line snapshots.
     *
     * Invariant: [transactionKey] is generated exactly once here and passed to the planner.
     * All snapshots registered in this call are owned by [OwnedByTransaction(transactionKey)];
     * the planner must use the same key in [PreparedVisualTransaction.transactionId] so that
     * [completeTransaction]/[cancelTransaction] can release resources under the correct owner.
     *
     * Two-phase ownership: this method optimistically registers ALL old/new snapshots into
     * [ownedSnapshotIds]. [submit] (Phase 2) trims this set to only the snapshots actually
     * referenced by slices or static patches, releasing unreferenced ones immediately.
     */
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

    /**
     * Submit a prepared animation as the new active transaction.
     *
     * Ownership invariant: after this method, [activeTransaction.ownedSnapshotIds] is the
     * *precise* set of snapshot IDs whose Bitmaps this transaction must release on
     * complete/cancel — it contains only (a) newly captured snapshots that are actually
     * referenced by slices or static patches, and (b) old-transaction snapshots whose
     * ownership was transferred because the new transaction references them.
     *
     * Unreferenced snapshots (newly captured but unused, or old snapshots no longer needed)
     * are released immediately, preventing unbounded growth during rapid consecutive input.
     */
    fun submit(preparedAnimation: PreparedVisualTransaction) {
        val oldTransaction = activeTransaction
        val newOwner = SnapshotOwner.OwnedByTransaction(preparedAnimation.transactionId)

        val unreferencedNewIds = preparedAnimation.ownedSnapshotIds - preparedAnimation.referencedSnapshotIds
        for (snapshotId in unreferencedNewIds) {
            resourceStore.release(snapshotId, newOwner)
        }

        if (oldTransaction != null) {
            val oldOwner = SnapshotOwner.OwnedByTransaction(oldTransaction.transactionId)

            // Snapshots referenced by the new transaction that were owned by the old transaction
            // but not newly captured: transfer ownership so they survive the old transaction's release.
            val referencedIds = preparedAnimation.referencedSnapshotIds
            val inheritedIds = oldTransaction.ownedSnapshotIds
                .intersect(referencedIds)
                .minus(preparedAnimation.ownedSnapshotIds)
            for (snapshotId in inheritedIds) {
                resourceStore.transferOwnership(snapshotId, newOwner)
            }

            // Snapshots from the old transaction that are no longer referenced: release now.
            val unreferencedOldIds = oldTransaction.ownedSnapshotIds - referencedIds - preparedAnimation.ownedSnapshotIds
            for (snapshotId in unreferencedOldIds) {
                resourceStore.release(snapshotId, oldOwner)
            }

            // preciseOwnedIds = (newly captured & referenced) + (inherited from old transaction).
            // Subtracting unreferencedNewIds prevents Bitmaps from the old transaction that
            // the new transaction never references from accumulating — without this, rapid
            // consecutive inputs would grow ownedSnapshotIds unboundedly until the final
            // transaction completes.
            val preciseOwnedIds = (preparedAnimation.ownedSnapshotIds - unreferencedNewIds) + inheritedIds
            activeTransaction = preparedAnimation.copy(ownedSnapshotIds = preciseOwnedIds)
            timeline?.complete()
        } else {
            val preciseOwnedIds = preparedAnimation.ownedSnapshotIds - unreferencedNewIds
            activeTransaction = preparedAnimation.copy(ownedSnapshotIds = preciseOwnedIds)
        }

        timeline = AnimationTimeline(preparedAnimation.durationMs)
    }

    /**
     * Capture old layout → apply mirror update → capture new layout → prepare → submit.
     *
     * Ordering invariant: old snapshots must be captured *before* the mirror update,
     * and new snapshots *after*, so that old/new represent the exact before/after states.
     * [beforePatch] runs between old capture and mirror update (e.g. to hide static text).
     */
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

    /**
     * Capture the current visual state of the active transaction for rendering or rebase.
     *
     * During rapid consecutive input, the returned snapshot preserves the current cursor rect
     * and per-slice visual states (position, alpha) so the next transaction's rebase can
     * continue from the on-screen position rather than the logical endpoint — preventing
     * cursor jumps and slice discontinuities.
     */
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

    /** Capture the rebase snapshot for the next transaction. Delegates to [captureFrame],
     *  which includes the current cursor rect — essential for continuous cursor animation
     *  across rapid consecutive inputs. */
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

    /** Cancel the active transaction and release ALL session-owned resources (snapshots,
     *  timeline state). Used for session rebind — different from [cancel] which only aborts
     *  the active transaction without releasing completed-but-not-yet-released snapshots. */
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
