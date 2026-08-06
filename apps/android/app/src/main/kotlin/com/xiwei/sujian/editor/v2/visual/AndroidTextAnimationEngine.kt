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
    private var cursorTimeline: AnimationTimeline? = null
    private var animationPolicy: TextAnimationPolicy = TextAnimationPolicy.INHERIT_GLOBAL
    private var smoothCursorEnabled: Boolean = true
    private var smoothCursorDurationMs: Long = 80L
    private var coordinatedEnabled: Boolean = true
    private var reduceMotion: Boolean = false

    /**
     * 平滑光标设置（生产路径：设置页 → Editor Host → 输入事务 → 本引擎）。
     *
     * 关闭时当前事务的光标过渡立即降级为静态（shouldAnimate=false）；
     * 开启时光标使用独立的 [cursorTimeline]，时长由 [smoothCursorDurationMs] 控制
     * （不超过文本事务时长，保证光标与文字同时到达终点）。
     */
    fun setSmoothCursor(enabled: Boolean, durationMs: Long) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
        if (!enabled) {
            cursorTimeline = null
        }
    }

    /**
     * #595 三/四：协同动画设置 — 控制文字和光标是否使用同一视觉事务。
     *
     * - coordinated=true：光标时长 = min(cursorDurationMs, textDurationMs)，
     *   文字和光标同一首帧、同一 rebase snapshot、光标先完成后停在终点。
     * - coordinated=false：光标可使用独立时长（不受文字时长限制），
     *   但仍由同一个 View、同一个 renderer、同一个 VSync 时间源驱动。
     */
    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        coordinatedEnabled = enabled
    }

    fun isCoordinatedAnimationEnabled(): Boolean = coordinatedEnabled

    /**
     * #595 三：reduce-motion 设置 — 降级所有动画为静态更新。
     */
    fun setReduceMotion(enabled: Boolean) {
        reduceMotion = enabled
        if (enabled) {
            animationPolicy = TextAnimationPolicy.SYSTEM_SUPPRESSED
            cursorTimeline = null
        }
    }

    fun isReduceMotion(): Boolean = reduceMotion

    /**
     * #595 六：暂停动画 — 临时失焦时保存当前可见帧，不永久取消事务。
     * 窗口重新获得焦点时从保存帧继续或稳定落到事务终态。
     */
    fun pause(frameTimeMs: Long) {
        timeline?.pause(frameTimeMs)
        cursorTimeline?.pause(frameTimeMs)
    }

    /**
     * #595 六：恢复动画 — 窗口重新获得焦点时从暂停帧继续。
     */
    fun resume(frameTimeMs: Long) {
        timeline?.resume(frameTimeMs)
        cursorTimeline?.resume(frameTimeMs)
    }

    fun isPaused(): Boolean = timeline?.isPaused() == true

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
    fun submit(
        preparedAnimation: PreparedVisualTransaction,
        submittedAtMs: Long = timeSource.nowNanos() / 1_000_000
    ) {
        val effectiveCursorTransition = if (!smoothCursorEnabled) {
            preparedAnimation.cursorTransition?.copy(shouldAnimate = false)
        } else {
            preparedAnimation.cursorTransition
        }
        val effectiveTransaction = if (effectiveCursorTransition !== preparedAnimation.cursorTransition) {
            preparedAnimation.copy(cursorTransition = effectiveCursorTransition)
        } else {
            preparedAnimation
        }
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
            activeTransaction = effectiveTransaction.copy(ownedSnapshotIds = preciseOwnedIds)
            timeline?.complete()
            cursorTimeline?.complete()
            // 连续输入重基：新事务在旧事务完成前接管，旧快照所有权转移给新事务。
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.animationRebase(
                oldTransaction.transactionId,
                preparedAnimation.transactionId,
            )
        } else {
            val preciseOwnedIds = preparedAnimation.ownedSnapshotIds - unreferencedNewIds
            activeTransaction = effectiveTransaction.copy(ownedSnapshotIds = preciseOwnedIds)
        }

        timeline = AnimationTimeline(preparedAnimation.durationMs, submittedAtMs)
        cursorTimeline = if (effectiveCursorTransition?.shouldAnimate == true && preparedAnimation.durationMs > 0L) {
            val cursorDuration = if (coordinatedEnabled) {
                minOf(smoothCursorDurationMs.coerceAtLeast(1L), preparedAnimation.durationMs)
            } else {
                smoothCursorDurationMs.coerceAtLeast(1L)
            }
            AnimationTimeline(cursorDuration, submittedAtMs)
        } else {
            null
        }
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.animationStart(
            preparedAnimation.transactionId,
            preparedAnimation.operationKind.name,
            preparedAnimation.durationMs
        )
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
     */
    fun prepareAndSubmit(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)? = null,
        beforePatch: (() -> Unit)? = null,
        frameTimeMs: Long? = null
    ) {
        val textSuppressed = animationPolicy == TextAnimationPolicy.SYSTEM_SUPPRESSED || reduceMotion
        val effectiveFrameTimeMs = frameTimeMs ?: (timeSource.nowNanos() / 1_000_000)

        if (textSuppressed && !smoothCursorEnabled) {
            // Both text and cursor suppressed (or reduce-motion): static update only.
            // beforePatch runs first (e.g. to hide stale static text), then mirrorUpdate
            // applies the edit, then requestLayout rebuilds the visual layout.
            beforePatch?.invoke()
            mirrorUpdate?.invoke()
            layoutEngine.requestLayout()
            return
        }

        if (textSuppressed && smoothCursorEnabled) {
            // #595 四: CursorOnly transaction — text static, cursor animates via the
            // same FrameClock. Text slices are suppressed (SYSTEM_SUPPRESSED mode → no
            // slices from the planner), but the cursor transition is preserved so the
            // cursor timeline drives smooth cursor movement from the same VSync source.
            submitCursorOnlyTransaction(visualIntent, layoutEngine, mirrorUpdate, beforePatch, effectiveFrameTimeMs)
            return
        }
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
        submit(transaction, effectiveFrameTimeMs)
    }

    /**
     * #595 四: CursorOnly transaction — 文字静态更新，光标由同一 FrameClock 平滑移动。
     *
     * 当 textEnabled=false 但 cursorEnabled=true 时调用。mirror/layout 先静态更新
     * （文字立即落到新状态），然后用 SYSTEM_SUPPRESSED 动画模式构造一个无文字切片
     * 但保留 cursorTransition 的事务，submit 创建 cursorTimeline 驱动光标平滑移动。
     * 事务时长设为光标时长，保证文字时间线不会先于光标完成而提前停止帧请求。
     */
    private fun submitCursorOnlyTransaction(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)?,
        beforePatch: (() -> Unit)?,
        frameTimeMs: Long,
    ) {
        val rebaseSnapshot = captureRebaseSnapshot(frameTimeMs)
        val oldRevision = layoutEngine.captureImmutableRevision()
        beforePatch?.invoke()
        mirrorUpdate?.invoke()
        layoutEngine.requestLayout()
        val newRevision = layoutEngine.getCurrentRevision()
        val cursorDuration = smoothCursorDurationMs.coerceAtLeast(1L)
        val cursorOnlyIntent = visualIntent.copy(
            animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
            durationMs = cursorDuration,
        )
        val transaction = prepare(cursorOnlyIntent, oldRevision, newRevision, emptyMap(), emptyMap(), rebaseSnapshot)
        submit(transaction, frameTimeMs)
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
        val cursorProgress = getCursorTimelineProgress(frameTimeMs) ?: p
        val sliceStates = computeSliceVisualStates(transaction, p)
        val cursorRect = computeCurrentCursorRect(transaction, cursorProgress)
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

    fun hasActiveAnimation(): Boolean {
        if (activeTransaction == null) return false
        val tl = timeline
        if (tl != null && tl.getState() != TransactionState.Completed) return true
        val ctl = cursorTimeline
        if (ctl != null && ctl.getState() != TransactionState.Completed) return true
        return false
    }

    fun currentTimeNanos(): Long = timeSource.nowNanos()

    fun cancel() {
        val transaction = activeTransaction ?: return
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.animationCancel(transaction.transactionId)
        cancelTransaction(transaction)
        activeTransaction = null
        timeline = null
        cursorTimeline = null
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
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.animationPolicy(policy.name)
    }

    fun getAnimationPolicy(): TextAnimationPolicy = animationPolicy

    fun getActiveTransaction(): PreparedVisualTransaction? = activeTransaction

    fun getTimelineProgress(frameTimeMs: Long): Float {
        val tl = timeline ?: return 0f
        return tl.progress(frameTimeMs)
    }

    /**
     * 独立光标时间线进度（生产渲染路径使用）。
     *
     * 与 [captureFrame] 中的光标进度完全一致：平滑光标开启时，屏幕上的光标
     * 由 [cursorTimeline]（时长 = min(smoothCursorDurationMs, 文本事务时长)）驱动；
     * 光标时间线先于文本完成时定格在终点（1f）；关闭（无时间线）时返回 null，
     * 由调用方回退到文本进度/静态光标。
     */
    fun getCursorTimelineProgress(frameTimeMs: Long): Float? {
        val tl = cursorTimeline ?: return null
        return when (tl.getState()) {
            TransactionState.Completed -> 1f
            TransactionState.Cancelled -> null
            else -> tl.progress(frameTimeMs)
        }
    }

    fun isTimelineCompleted(frameTimeMs: Long): Boolean {
        val tl = timeline ?: return true
        return tl.isCompleted(frameTimeMs)
    }

    /**
     * #595 五：光标轨是否已完成 — 光标 timeline 不存在或已结束。
     * 非协同模式下光标时长可长于文字时长，文字完成后光标仍可继续。
     */
    fun isCursorTimelineCompleted(frameTimeMs: Long): Boolean {
        val ctl = cursorTimeline ?: return true
        return ctl.isCompleted(frameTimeMs)
    }

    /**
     * #595 五：文字轨是否已完成 — 文字 timeline 不存在或已结束。
     */
    fun isTextTimelineCompleted(frameTimeMs: Long): Boolean = isTimelineCompleted(frameTimeMs)

    /**
     * #595 五：整个视觉事务是否完成 — 文字轨和光标轨都结束。
     */
    fun isTransactionCompleted(frameTimeMs: Long): Boolean {
        return isTextTimelineCompleted(frameTimeMs) && isCursorTimelineCompleted(frameTimeMs)
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
        cursorTimeline?.markFirstVisibleFrame(frameTimeMs)
    }

    fun completeIfFinished(frameTimeMs: Long): Boolean {
        val transaction = activeTransaction ?: return false
        val tl = timeline ?: return false
        if (tl.getState() == TransactionState.Completed) return false
        // #595 五：只有文字轨和光标轨都完成才结束整个事务。
        // 文字完成后光标仍可继续（非协同模式光标时长 > 文字时长）。
        // 文字完成后释放文字切片 Bitmap，但保留光标过渡所需的不可变几何和事务标识。
        val textFinished = tl.isCompleted(frameTimeMs)
        val cursorFinished = isCursorTimelineCompleted(frameTimeMs)
        if (textFinished && cursorFinished) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.animationComplete(
                transaction.transactionId,
                (frameTimeMs - (tl.getFirstVisibleFrameTimeMs() ?: frameTimeMs)).coerceAtLeast(0L)
            )
            completeTransaction(transaction)
            // #595 六：统一终态 — 事务对象与两条 timeline 全部离开引擎。
            // 旧实现只把 activeTransaction 换成 ownedSnapshotIds=emptySet() 的副本，
            // 事务对象仍留在引擎中：下一次 submit 会误发 rebase 事件，且引擎无法
            // 表达 Completed/Idle 统一终态。完成后 activeTransaction==null，
            // hasActiveAnimation()==false，FrameClock 停止 repost。
            activeTransaction = null
            timeline = null
            cursorTimeline = null
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
     * Release all snapshots owned by this transaction and mark BOTH timelines completed.
     *
     * Invariant: [transaction.ownedSnapshotIds] is the *precise* ownership set computed by
     * [submit] — it contains only (a) newly captured snapshots referenced by slices/patches,
     * and (b) old-transaction snapshots inherited via ownership transfer. Unreferenced snapshots
     * were already released during [submit], so this method releases exactly the right set
     * without scanning slices or patches.
     *
     * #595 七：文字轨和光标轨都必须进入终态。只 complete 文字 timeline 而留下
     * cursorTimeline 非 Completed，会让 hasActiveAnimation() 持续返回 true，
     * FrameClock 永久 repost 造成无意义耗电。
     */
    private fun completeTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.complete()
        cursorTimeline?.complete()
    }

    /** Same ownership invariant as [completeTransaction]; both timelines are cancelled instead. */
    private fun cancelTransaction(transaction: PreparedVisualTransaction) {
        val owner = SnapshotOwner.OwnedByTransaction(transaction.transactionId)
        for (snapshotId in transaction.ownedSnapshotIds) {
            resourceStore.release(snapshotId, owner)
        }
        timeline?.cancel()
        cursorTimeline?.cancel()
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
