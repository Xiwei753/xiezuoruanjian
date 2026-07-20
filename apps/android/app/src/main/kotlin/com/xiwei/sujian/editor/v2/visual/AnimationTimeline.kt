package com.xiwei.sujian.editor.v2.visual

/**
 * Monotonic timeline for a single animation transaction.
 *
 * Uses re-anchoring rather than accumulated pause duration: on [resume], the start time
 * is shifted so that progress(frameTimeMs) immediately equals the paused progress value.
 * This avoids floating-point drift from repeatedly adding/subtracting pause durations and
 * simplifies the math — accumulatedPausedDurationMs resets to 0 after each re-anchor.
 *
 * [progress] returns values in [0f, 1f]. When [durationMs] == 0, progress is 1f
 * (animation completes instantly — the final state should be shown immediately).
 */
class AnimationTimeline(
    private val durationMs: Long
) {
    private var firstVisibleFrameTimeMs: Long? = null
    private var pauseStartedAtMs: Long? = null
    private var accumulatedPausedDurationMs: Long = 0
    private var pausedProgress: Float = 0f
    private var state: TransactionState = TransactionState.Pending

    fun progress(frameTimeMs: Long): Float {
        val start = firstVisibleFrameTimeMs ?: return 0f
        if (state == TransactionState.Paused) return pausedProgress
        if (durationMs == 0L) return 1f

        val effectiveElapsed = frameTimeMs - start - accumulatedPausedDurationMs
        val p = effectiveElapsed.toFloat() / durationMs.toFloat()
        return p.coerceIn(0f, 1f)
    }

    fun markFirstVisibleFrame(frameTimeMs: Long) {
        if (firstVisibleFrameTimeMs == null) {
            firstVisibleFrameTimeMs = frameTimeMs
            state = TransactionState.Rendering
        }
    }

    fun pause(frameTimeMs: Long) {
        if (state == TransactionState.Paused) return
        pausedProgress = progress(frameTimeMs)
        pauseStartedAtMs = frameTimeMs
        state = TransactionState.Paused
    }

    fun resume(frameTimeMs: Long) {
        if (state != TransactionState.Paused) return
        if (firstVisibleFrameTimeMs == null) {
            // Never rendered before pause: reset to Pending so markFirstVisibleFrame can set it.
            pauseStartedAtMs = null
            pausedProgress = 0f
            state = TransactionState.Pending
            return
        }
        // Re-anchor: shift firstVisibleFrameTimeMs so that progress(frameTimeMs) equals
        // pausedProgress immediately after resume. Solving for newStart:
        //   pausedProgress = (frameTimeMs - newStart - 0) / durationMs
        //   newStart = frameTimeMs - pausedProgress * durationMs
        // Also reset accumulatedPausedDurationMs since the new anchor already accounts
        // for all prior pause time. Truncation to whole milliseconds via toLong() is
        // acceptable because frame timestamps are millisecond-resolution; sub-ms precision
        // would be lost in progress() anyway.
        val newStart = frameTimeMs - (pausedProgress * durationMs).toLong()
        firstVisibleFrameTimeMs = newStart
        accumulatedPausedDurationMs = 0
        pauseStartedAtMs = null
        pausedProgress = 0f
        state = TransactionState.Rendering
    }

    fun complete() {
        state = TransactionState.Completed
    }

    fun cancel() {
        state = TransactionState.Cancelled
    }

    fun isPaused(): Boolean = state == TransactionState.Paused

    fun isCompleted(frameTimeMs: Long): Boolean = progress(frameTimeMs) >= 1f

    fun getState(): TransactionState = state

    /**
     * Minimal frame snapshot containing only progress and state — no slice/cursor/block data.
     *
     * This is the timeline's own view of the animation: it tracks *when* the animation is
     * but not *what* is being animated. The full visual frame (with interpolated slice
     * positions, cursor rect, and block-shift translations) is assembled by
     * [com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine.captureFrame], which
     * combines this timeline's progress with the active transaction's visual data.
     *
     * Separation of concerns: [AnimationTimeline] is a pure temporal controller; the engine
     * is the visual state owner. This ensures the timeline can be tested independently and
     * that visual state computation is centralized in one place rather than split across
     * timeline progress and per-slice interpolation.
     */
    fun currentVisualFrame(frameTimeMs: Long): VisualFrameSnapshot? {
        if (state == TransactionState.Completed || state == TransactionState.Cancelled) return null
        val p = progress(frameTimeMs)
        return VisualFrameSnapshot(progress = p, state = state)
    }
}

data class VisualFrameSnapshot(
    val progress: Float,
    val state: TransactionState,
    val sliceVisualStates: List<SliceVisualState> = emptyList(),
    /** Current cursor rect at [progress]. Used by rebase to set the next transaction's
     *  [CursorTransition.fromX/fromY/fromHeight] from the on-screen position rather than
     *  the old logical endpoint — preventing cursor jumps during rapid consecutive input. */
    val cursorRect: android.graphics.RectF? = null,
    /** Current visual state of block shifts at [progress]. Used by rebase so that the next
     *  transaction's BlockShift starts from the on-screen translateY rather than the full
     *  -deltaY — preventing suffix blocks from jumping back to the old position during
     *  rapid consecutive input. */
    val blockShiftStates: List<BlockShiftVisualState> = emptyList()
)

data class BlockShiftVisualState(
    val startLineIndex: Int,
    /** Exclusive end line index (half-open: [startLineIndex, endLineIndexExclusive)). */
    val endLineIndexExclusive: Int,
    /** UTF-8 byte offset of the first line in this block. Used by rebase matching
     *  instead of [startLineIndex] because line indices shift across revisions when
     *  hard breaks are inserted/deleted — the old transaction's line N may become
     *  line N+1 in the new revision, causing line-index-based matching to fail. */
    val startUtf8: Int,
    /** Current Y translation at the snapshot's progress point.
     *  Negative = text is above its new-layout position (still moving down);
     *  positive = text is below its new-layout position (still moving up);
     *  zero = text is at the new-layout position (animation complete or no shift). */
    val currentTranslateY: Float,
    /** Always 0 — the animation's final state is the new layout with no translation.
     *  Included for API symmetry with [SliceVisualState.destinationLeft/Top/Right/Bottom]. */
    val targetTranslateY: Float
)

data class SliceVisualState(
    val snapshotId: Long,
    val role: SliceRole,
    val lineIndex: Int,
    /** Inclusive UTF-8 byte offset of the entire visual line in the document. */
    val documentByteStart: Int = -1,
    /** Exclusive UTF-8 byte offset of the entire visual line (half-open: [start, end)). */
    val documentByteEndExclusive: Int = -1,
    /** Inclusive UTF-8 byte offset of the specific cluster within the line that this slice animates. */
    val clusterByteStart: Int = -1,
    /** Exclusive UTF-8 byte offset of the cluster within the line (half-open: [start, end)). */
    val clusterByteEndExclusive: Int = -1,
    val currentLeft: Float,
    val currentTop: Float,
    val currentRight: Float,
    val currentBottom: Float,
    val currentAlpha: Float,
    val destinationLeft: Float = currentLeft,
    val destinationTop: Float = currentTop,
    val destinationRight: Float = currentRight,
    val destinationBottom: Float = currentBottom
)
