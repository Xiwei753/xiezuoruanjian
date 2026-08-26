package com.xiwei.sujian.feature.editor.visual

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
 *
 * Anchor semantics: when the transaction is submitted with a real timestamp, the
 * timeline is anchored at the submission moment and immediately enters
 * [TransactionState.Rendering]. Progress is therefore the exact
 * elapsed time since the edit — deterministic and independent of when the first frame
 * happens to be drawn. This matters for the manual-frame test protocol (which advances a
 * manual clock and reads progress directly) and prevents a delayed or coalesced first
 * draw from re-anchoring the animation late. Without a submission timestamp (unit-test
 * construction), the timeline stays [TransactionState.Pending] and anchors at the first
 * visible frame as before.
 */
class AnimationTimeline(
    private val durationMs: Long,
    private val submittedAtMs: Long = Long.MIN_VALUE,
) {
    /** Sentinel for "no submission timestamp provided". */
    private val noSubmission: Boolean = submittedAtMs == Long.MIN_VALUE
    private var firstVisibleFrameTimeMs: Long? = if (noSubmission) null else submittedAtMs
    private var pauseStartedAtMs: Long? = null
    private var accumulatedPausedDurationMs: Long = 0
    private var pausedProgress: Float = 0f
    private var state: TransactionState = if (noSubmission) TransactionState.Pending else TransactionState.Rendering

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
            // Anchoring rules:
            // - With a real submission timestamp the anchor is the submission moment
            //   itself: progress counts elapsed time since the edit, and neither a stale
            //   pending frame time (from before the submit) nor a delayed first draw
            //   (after the submit) can mis-anchor the animation.
            // - Without one (unit-test construction) the first visible frame stamps the
            //   anchor, clamped to never precede a known submission.
            firstVisibleFrameTimeMs = if (noSubmission) frameTimeMs else submittedAtMs
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

    fun getFirstVisibleFrameTimeMs(): Long? = firstVisibleFrameTimeMs

    /**
     * Minimal frame snapshot containing only progress and state — no slice/cursor/block data.
     *
     * This is the timeline's own view of the animation: it tracks *when* the animation is
     * but not *what* is being animated. The full visual frame (with interpolated slice
     * positions, cursor rect, and block-shift translations) is assembled by
     * [com.xiwei.sujian.feature.editor.visual.AndroidTextAnimationEngine.captureFrame], which
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

/**
 * Immutable snapshot of the visual state at a specific animation progress point.
 *
 * Primary consumer: [AndroidTextAnimationEngine.captureRebaseSnapshot], which captures
 * the current frame before submitting a new transaction. The rebase snapshot carries
 * interpolated positions/alphas/translations so that the next transaction can continue
 * from the on-screen state rather than from the logical start/end — this is essential
 * for visual continuity during rapid consecutive input.
 *
 * [AnimationTimeline.currentVisualFrame] produces a minimal version with only [progress]
 * and [state]; the full version with slice/cursor/block data is assembled by
 * [AndroidTextAnimationEngine.captureFrame].
 */
data class VisualFrameSnapshot(
    val progress: Float,
    val state: TransactionState,
    val sliceVisualStates: List<SliceVisualState> = emptyList(),
    /** Current cursor rect at [progress]. Used by rebase to set the next transaction's
     *  [CursorTransition.fromX/fromY/fromHeight] from the on-screen position rather than
     *  the old logical endpoint — preventing cursor jumps during rapid consecutive input. */
    val cursorRect: android.graphics.RectF? = null,
    /** #637 评论 5386573878：光标在当前帧之后还剩多少基准时长（fraction）。
     *  由 cursorTransition.progressWindow.remainingFractionAt(cursorProgress) 算出。
     *  rebase continuation 用 [VisualProgressWindow.fromRemainingFraction] 直接消费，
     *  连续 rebase 不会反复减速（旧 localProgress 会在旧 window [0,0.4] 走到 0.2 时
     *  得 0.5，下一次又放大回 50ms；remainingFraction 得 0.2，保持 20ms）。 */
    val cursorRemainingFraction: Float = 1f,
    /** Current visual state of block shifts at [progress]. Used by rebase so that the next
     *  transaction's BlockShift starts from the on-screen translateY rather than the full
     *  -deltaY — preventing suffix blocks from jumping back to the old position during
     *  rapid consecutive input. */
    val blockShiftStates: List<BlockShiftVisualState> = emptyList(),
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
    val endUtf8Exclusive: Int = -1,
    /** Current Y translation at the snapshot's progress point.
     *  Negative = text is above its new-layout position (still moving down);
     *  positive = text is below its new-layout position (still moving up);
     *  zero = text is at the new-layout position (animation complete or no shift). */
    val currentTranslateY: Float,
    /** Always 0 — the animation's final state is the new layout with no translation.
     *  Included for API symmetry with [SliceVisualState.destinationLeft/Top/Right/Bottom]. */
    val targetTranslateY: Float,
    /** #637 评论 5386573878：本 BlockShift 在当前帧之后还剩多少基准时长（fraction）。
     *  由 progressWindow.remainingFractionAt(globalProgress) 算出。rebase continuation
     *  用 [VisualProgressWindow.fromRemainingFraction] 直接消费，连续 rebase 保持匀速。 */
    val remainingFraction: Float = 1f,
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
    val destinationBottom: Float = currentBottom,
    /** #639 评论 5427183226 缺口1：本 slice 在 line bitmap 中的 source crop。
     *  从 active slice.sourceRect 原样保存，未映射 continuation 优先用这份 sourceRect，
     *  不再退到 snapshot.sourceRect（整行 bitmap 的 source crop）。
     *  null 仅作旧状态兼容 fallback（旧 SliceVisualState 没这字段）。 */
    val sourceRect: android.graphics.Rect? = null,
    /** #639 评论 5427183226 缺口2：本 slice 的目标 alpha（active slice.endAlpha）。
     *  rebase 未映射 continuation 用 abs(currentAlpha - targetAlpha) > EPS 判断 alpha 轨
     *  是否还有剩余，不再按 SliceRole 猜。默认 currentAlpha 保持向后兼容（旧状态没这字段
     *  时退化为"alpha 已完成"）。 */
    val targetAlpha: Float = currentAlpha,
    /** #639 评论 5427183226 缺口2：本 slice 的 reveal 模式（REVEAL/SWALLOW）。
     *  从 active slice.revealSpec?.mode 原样保存，未映射 continuation 用 state.revealMode +
     *  state.revealFraction + caretRevealGeometry 重建 revealSpec，不再只认 Insert/Delete。 */
    val revealMode: TextRevealMode? = null,
    /** Current reveal fraction for Insert/Delete slices with revealSpec.
     *  null for Move/Crossfade/Static slices or slices without revealSpec.
     *  Used by rebase to set the next transaction's revealSpec.initialFraction
     *  so the animation continues from the on-screen position. */
    val revealFraction: Float? = null,
    /** #637 评论 5386573878：本 slice 在当前帧之后还剩多少基准时长（fraction）。
     *  由 progressWindow.remainingFractionAt(globalProgress) 算出。rebase continuation
     *  用 [VisualProgressWindow.fromRemainingFraction] 直接消费，连续 rebase 不会
     *  反复减速（旧 localProgress 会在旧 window [0,0.4] 走到 0.2 时得 0.5，下一次
     *  又放大回 50ms；remainingFraction 得 0.2，保持 20ms）。 */
    val remainingFraction: Float = 1f,
    /** #639 评论 5427183226 缺口2：rebase 冻结的 document-space clip rect。
     *  从 active slice.fixedRevealClipRect 原样保存。旧 Insert -> CrossfadeOld 冻结出来
     *  的半截 clip，下一次 mapped/unmapped rebase 后不再消失。renderer 把 fixed clip 提到
     *  drawOrthogonalSlice 正交化，优先级 fixedRevealClipRect > revealSpec clip > no clip。 */
    val fixedRevealClipRect: android.graphics.RectF? = null,
    /** #639 评论 5425871530 第二部分：本 slice 的 caret/reveal 几何，从 active slice
     *  直接复制进来，把外观状态一起带进下一次 rebase。
     *
     *  rebase 时 [RebasePlanner] 直接消费这份几何重建 REVEAL continuation，不再从
     *  [AndroidLineSnapshot.clusters] 按 byte range 反查 — 这同时修掉 RunAnimation
     *  synthetic run 不在原始 clusters 里的漏洞。 */
    val caretRevealGeometry: PreparedVisualTransaction.CaretRevealGeometry? = null,
    /** #639 评论 5427812180 缺陷4：本 slice 的静态底图 suppression 模式，从 active slice
     *  原样保存（slice.staticSuppressionMode ?: defaultStaticSuppressionModeForRole(slice.role)）。
     *  mapped/unmapped continuation 继续此 mode，不因新 role 变了瞬间切换底图 ownership。 */
    val staticSuppressionMode: StaticSuppressionMode? = null,
    /** #639 评论 5427812180 缺陷5：fixed clip 的 base rect，从 active slice.fixedClipBaseRect 原样保存。
     *  mapped rebase 后若 slice 位置会移动，renderer 每帧用 currentRect - fixedClipBaseRect 平移
     *  fixedRevealClipRect，让 clip 跟 bitmap 一起移动。 */
    val fixedClipBaseRect: android.graphics.RectF? = null,
)
