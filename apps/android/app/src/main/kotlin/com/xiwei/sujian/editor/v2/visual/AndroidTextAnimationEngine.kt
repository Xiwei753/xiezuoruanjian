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
    private val resourceStore: VisualResourceStore,
    private val timeSource: AnimationTimeSource = ChoreographerAnimationTimeSource(),
    private val transactionIdSource: TransactionIdSource = TransactionIdSource()
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
        val transactionKey = transactionIdSource.nextId()
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
            // Populate snapshotLookup from the rebase frame's surviving slices. These
            // snapshots belong to the OLD transaction and may not be in oldSnapshots/
            // newSnapshots (which were captured fresh for this transaction). Without this
            // step, surviving slices (e.g. a still-fading Delete) would reference snapshot
            // IDs that the planner can't look up, producing null-bitmap slices that the
            // renderer silently skips — losing the old animation's visual state.
            //
            // If a snapshot was already released (e.g. old transaction completed before
            // this prepare call), resourceStore.get returns null and the entry is skipped —
            // the planner's applyRebaseToSlices will produce a surviving slice with a null
            // snapshot, which the renderer safely ignores (drawAnimatedSlices skips
            // null-bitmap slices).
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

            // Inherit old-transaction snapshots that the new transaction actually references.
            // These were captured by the old transaction but are still needed by the new one
            // (e.g. a surviving Delete slice that continues fading out an old snapshot).
            // Ownership is transferred so they survive the old transaction's release.
            // Only snapshots in referencedSnapshotIds are transferred — unreferenced old
            // snapshots are released below, preventing unbounded accumulation during rapid input.
            //
            // .minus(preparedAnimation.ownedSnapshotIds) excludes snapshots that the new
            // transaction already captured itself — these are already owned by the new
            // transactionKey and must not be "transferred" (which would change their owner
            // from the correct new key to the same new key, a no-op that would also
            // incorrectly include them in inheritedIds, inflating preciseOwnedIds).
            // This exclusion is necessary because preparedAnimation.ownedSnapshotIds contains
            // ALL newly captured snapshots (referenced and unreferenced), while
            // referencedSnapshotIds contains only those actually used. Without .minus, a
            // newly captured but unreferenced snapshot that also happens to be in the old
            // transaction's ownedSnapshotIds would be double-counted: once in the new
            // transaction's ownedSnapshotIds and once in inheritedIds, leading to an
            // inflated preciseOwnedIds and a failed release (owner mismatch) when the new
            // transaction completes.
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
     * [beforePatch] runs between old capture and mirror update (e.g. to hide static text
     * that would otherwise be visible alongside the old-snapshot animation during the
     * brief window between mirror update and first animation frame).
     *
     * Timestamp: When [frameTimeMs] is provided, it is used as the current time (e.g.
     * from a Choreographer frame callback). Otherwise [AnimationTimeSource.nowNanos] / 1_000_000
     * provides a monotonic millisecond clock consistent with [AnimationTimeline]'s internal
     * time base. Sub-millisecond precision is intentionally discarded —
     * [AnimationTimeline.progress] operates in whole milliseconds.
     * The default [ChoreographerAnimationTimeSource] delegates to [System.nanoTime], which is
     * monotonic (unlike [System.currentTimeMillis], which can jump backwards on NTP adjustments).
     * Tests inject [ManualAnimationTimeSource] to control time precisely.
     */
    fun prepareAndSubmit(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)? = null,
        beforePatch: (() -> Unit)? = null,
        frameTimeMs: Long? = null
    ) {
        if (animationPolicy == TextAnimationPolicy.SYSTEM_SUPPRESSED) {
            // Animation suppressed, but mirror/layout must still update so the
            // display reflects the new text state. beforePatch runs first (e.g.
            // to hide stale static text), then mirrorUpdate applies the edit,
            // then requestLayout rebuilds the visual layout. Skipping all three
            // would leave the display showing stale text.
            beforePatch?.invoke()
            mirrorUpdate?.invoke()
            layoutEngine.requestLayout()
            return
        }
        val effectiveFrameTimeMs = frameTimeMs ?: (timeSource.nowNanos() / 1_000_000)
        val rebaseSnapshot = captureRebaseSnapshot(effectiveFrameTimeMs)

        val oldRevision = layoutEngine.captureImmutableRevision()
        // Two-phase affected-line computation invariant:
        // Phase 1 (newRevision=null): determines old snapshot lines BEFORE mirror update,
        // using only the old layout. The new revision does not exist yet — the mirror
        // has not been updated, so layoutEngine still holds the old layout.
        // Phase 2 (both revisions): determines new snapshot lines AND BlockShifts AFTER
        // mirror update and layout rebuild. BlockShifts require both revisions to compare
        // paragraph Y positions; they cannot be computed in Phase 1.
        // This split is essential: capturing old snapshots after mirrorUpdate would
        // produce stale bitmaps (the layout has already changed), and computing
        // BlockShifts with only one revision would miss the Y-delta information.
        val preliminaryResult = visualPlanner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRevision, null)
        val affectedOldLineIndices = preliminaryResult.oldLineIndices
        val oldSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedOldLineIndices)
        beforePatch?.invoke()
        mirrorUpdate?.invoke()
        layoutEngine.requestLayout()
        val newRevision = layoutEngine.getCurrentRevision()
        // Phase 2: compute new affected lines and BlockShifts using both revisions.
        // BlockShifts can only be determined when both old and new revisions are available,
        // because they require comparing paragraph Y positions across revisions.
        // New snapshot lines must be captured AFTER mirrorUpdate (the layout reflects
        // the new text state); capturing them before would produce the old layout's bitmaps.
        val affectedResult = visualPlanner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRevision, newRevision)
        val affectedNewLineIndices = affectedResult.newLineIndices
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
     * Returns null when no transaction is active or the timeline is null.
     *
     * Supports three timeline states:
     * - [TransactionState.Rendering] / [TransactionState.Paused]: normal frame capture at
     *   the current progress, with interpolated slice positions/alphas and cursor rect.
     * - [TransactionState.Pending]: the transaction was just submitted but has not yet
     *   been drawn on screen. Returns a frame at progress=0f with all slices at their
     *   start positions/alphas and the cursor at its start rect. This is essential for
     *   rebase during rapid consecutive input — if a second edit arrives before the first
     *   transaction's first onDraw, the rebase snapshot must capture the first transaction's
     *   initial visual state rather than returning null (which would lose the first animation).
     *
     * Returns null for Completed/Cancelled states — these are terminal and produce no frames.
     */
    fun captureFrame(frameTimeMs: Long): VisualFrameSnapshot? {
        val transaction = activeTransaction ?: return null
        val tl = timeline ?: return null
        val state = tl.getState()
        if (state == TransactionState.Completed || state == TransactionState.Cancelled) return null
        if (state == TransactionState.Pending) {
            val sliceStates = computeSliceVisualStates(transaction, 0f)
            val cursorRect = computeCurrentCursorRect(transaction, 0f)
            val blockStates = computeBlockShiftVisualStates(transaction, 0f)
            return VisualFrameSnapshot(
                progress = 0f,
                state = TransactionState.Pending,
                sliceVisualStates = sliceStates,
                cursorRect = cursorRect,
                blockShiftStates = blockStates
            )
        }
        val p = tl.progress(frameTimeMs)
        val sliceStates = computeSliceVisualStates(transaction, p)
        val cursorRect = computeCurrentCursorRect(transaction, p)
        val blockStates = computeBlockShiftVisualStates(transaction, p)
        return VisualFrameSnapshot(
            progress = p,
            state = state,
            sliceVisualStates = sliceStates,
            cursorRect = cursorRect,
            blockShiftStates = blockStates
        )
    }

    /** Capture the rebase snapshot for the next transaction. Delegates to [captureFrame],
     *  which includes the current cursor rect — essential for continuous cursor animation
     *  across rapid consecutive inputs. */
    fun captureRebaseSnapshot(frameTimeMs: Long): VisualFrameSnapshot? {
        return captureFrame(frameTimeMs)
    }

    fun hasActiveAnimation(): Boolean = activeTransaction != null && timeline != null

    fun currentTimeNanos(): Long = timeSource.nowNanos()

    fun cancel() {
        val transaction = activeTransaction ?: return
        cancelTransaction(transaction)
        activeTransaction = null
        timeline = null
    }

    /** Cancel the active transaction and release ALL resources in [resourceStore] (not just
     *  the active transaction's snapshots). Used for session rebind — [cancel] releases only
     *  the active transaction's [ownedSnapshotIds], but the store may contain snapshots from
     *  completed transactions that were not yet garbage-collected. [resetForSession] ensures
     *  no Bitmaps survive across session boundaries. */
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

    fun getActiveAnimationStartTimeMs(): Long? = timeline?.getFirstVisibleFrameTimeMs()

    fun captureStateSnapshot(frameTimeMs: Long): AnimationStateSnapshot? {
        val transaction = activeTransaction ?: return null
        val tl = timeline ?: return null
        val p = tl.progress(frameTimeMs)
        return AnimationStateSnapshot(
            transactionId = transaction.transactionId,
            operationKind = transaction.operationKind.name.lowercase(),
            animationMode = if (transaction.durationMs == 0L) "instant" else "animated",
            oldAffectedRanges = transaction.animatedSlices
                .filter { it.role == SliceRole.Delete || it.role == SliceRole.CrossfadeOld }
                .mapNotNull { slice ->
                    val start = slice.clusterByteStart
                    val end = slice.clusterByteEndExclusive
                    if (start >= 0 && end > start) Pair(start, end) else null
                },
            newAffectedRanges = transaction.animatedSlices
                .filter { it.role == SliceRole.Insert || it.role == SliceRole.CrossfadeNew || it.role == SliceRole.Move }
                .mapNotNull { slice ->
                    val start = slice.clusterByteStart
                    val end = slice.clusterByteEndExclusive
                    if (start >= 0 && end > start) Pair(start, end) else null
                },
            progress = p,
            sliceRoles = transaction.animatedSlices.map { it.role },
            cursorTransition = transaction.cursorTransition?.let { ct ->
                CursorTransitionSnapshot(
                    fromX = ct.fromX, fromY = ct.fromY, fromHeight = ct.fromHeight,
                    toX = ct.toX, toY = ct.toY, toHeight = ct.toHeight,
                    shouldAnimate = ct.shouldAnimate
                )
            },
            ownedResourceCount = transaction.ownedSnapshotIds.size,
            transactionState = tl.getState()
        )
    }

    fun getTimelineProgress(frameTimeMs: Long): Float {
        val tl = timeline ?: return 0f
        return tl.progress(frameTimeMs)
    }

    fun isTimelineCompleted(frameTimeMs: Long): Boolean {
        val tl = timeline ?: return true
        return tl.isCompleted(frameTimeMs)
    }

    /**
     * Transition the timeline from Pending to Rendering on the first onDraw after submit.
     *
     * Must be called from the host's draw path (e.g. View.onDraw / Compose draw callback)
     * so that [AnimationTimeline.progress] uses a real frame timestamp rather than the
     * submission time. Without this, [captureFrame] returns a Pending-state frame at
     * progress=0f, which is correct for rebase but would never advance the animation.
     *
     * Idempotent: subsequent calls after the first are no-ops — the timeline stays in
     * Rendering until paused, completed, or cancelled.
     */
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

    /**
     * Interpolate each animated slice's position and alpha from its start/end values at [progress].
     * For Move slices, [fromDestinationRect] is the pre-move position; for other roles it falls
     * back to [destinationRect] (alpha-only animation).
     *
     * Exception after rebase: Insert/CrossfadeNew slices that were rebased onto a Move slice
     * inherit [fromDestinationRect] from the Move's current position (see [applyRebaseState]).
     * In this case the slice animates both position and alpha — it slides from the old Move's
     * current position to its own destination while fading in.
     */
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

    /**
     * Release all snapshots owned by this transaction and mark the timeline completed.
     *
     * Invariant: [transaction.ownedSnapshotIds] is the *precise* ownership set computed by
     * [submit] — it contains only (a) newly captured snapshots referenced by slices/patches,
     * and (b) old-transaction snapshots inherited via ownership transfer. Unreferenced snapshots
     * were already released during [submit], so this method releases exactly the right set
     * without scanning slices or patches.
     */
    private fun completeTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.complete()
    }

    /** Same ownership invariant as [completeTransaction]; timeline is cancelled instead. */
    private fun cancelTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.cancel()
    }

    /**
     * Interpolate the cursor rectangle from [CursorTransition] at [progress].
     * Width is hardcoded to 2px (visual cursor bar width, not derived from layout).
     */
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

    /**
     * Interpolate each BlockShift's vertical translation at [progress].
     *
     * Interpolation: translateY = deltaY * (progress - 1).
     * - progress=0 → translateY = -deltaY (text at its old position, above the new layout).
     * - progress=1 → translateY = 0 (text at the new layout position).
     *
     * Sign convention for deltaY: positive when the block moved downward in the new layout
     * (newTop > oldTop). During animation, currentTranslateY = deltaY * (progress - 1) is
     * negative (text is above its final position, still moving down). At progress=0,
     * currentTranslateY = -deltaY, which is the offset from the new-layout position to the
     * old-layout position. This sign convention is consistent throughout the rebase chain:
     * currentTranslateY < 0 means "text has not yet reached the new position" (animating
     * downward), currentTranslateY > 0 means "text has overshot the new position" (animating
     * upward), currentTranslateY = 0 means "text is at the new-layout position".
     *
     * [targetTranslateY] is always 0 because the animation's final state is the new layout
     * with no translation. The rebase consumer uses [currentTranslateY] to adjust the next
     * transaction's deltaY so that consecutive inputs continue from the on-screen position.
     *
     * Rebase invariant: after [AndroidVisualPlanner.applyRebaseToBlockShifts] adjusts
     * deltaY to (newDeltaY - oldCurrentTranslateY), the formula still produces the correct
     * on-screen position at progress=0: translateY = -(newDeltaY - oldCurrentTranslateY).
     * The old on-screen position = layout_1_Y + currentTranslateY_old. The new layout
     * position = layout_2_Y. Continuity requires layout_2_Y - adjustedDeltaY =
     * layout_1_Y + currentTranslateY_old, so adjustedDeltaY = newDeltaY - currentTranslateY_old.
     * At progress=1 the text reaches the new layout position (translateY = 0) regardless of rebase.
     */
    private fun computeBlockShiftVisualStates(
        transaction: PreparedVisualTransaction,
        progress: Float
    ): List<BlockShiftVisualState> {
        return transaction.blockShifts.map { shift ->
            val currentTranslateY = shift.deltaY * (progress - 1f)
            BlockShiftVisualState(
                startLineIndex = shift.startLineIndex,
                endLineIndexExclusive = shift.endLineIndexExclusive,
                startUtf8 = shift.startUtf8,
                endUtf8Exclusive = shift.endUtf8Exclusive,
                currentTranslateY = currentTranslateY,
                targetTranslateY = 0f
            )
        }
    }
}

enum class TextAnimationPolicy {
    INHERIT_GLOBAL,
    ENABLED,
    SYSTEM_SUPPRESSED
}
