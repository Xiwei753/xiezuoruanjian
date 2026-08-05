package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision

class AndroidVisualRuntime(
    private val visualPlanner: AndroidVisualPlanner,
    private val animationEngine: AndroidTextAnimationEngine,
    private val resourceStore: VisualResourceStore
) {
    constructor(
        timeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
        transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource()
    ) : this(
        AndroidVisualPlanner(),
        VisualResourceStore(),
        timeSource,
        transactionIdSource
    )

    constructor(visualPlanner: AndroidVisualPlanner, resourceStore: VisualResourceStore) : this(
        visualPlanner,
        AndroidTextAnimationEngine(visualPlanner, resourceStore),
        resourceStore
    )

    constructor(
        visualPlanner: AndroidVisualPlanner,
        resourceStore: VisualResourceStore,
        timeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource,
        transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource
    ) : this(
        visualPlanner,
        AndroidTextAnimationEngine(visualPlanner, resourceStore, timeSource, transactionIdSource),
        resourceStore
    )

    fun prepareAndSubmit(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)? = null,
        beforePatch: (() -> Unit)? = null,
        frameTimeMs: Long? = null
    ) {
        animationEngine.prepareAndSubmit(visualIntent, layoutEngine, mirrorUpdate, beforePatch, frameTimeMs)
    }

    fun cancel() {
        animationEngine.cancel()
    }

    fun release() {
        animationEngine.release()
    }

    fun completeAfterDraw(frameTimeMs: Long) {
        animationEngine.completeIfFinished(frameTimeMs)
    }

    /**
     * Apply timeline state transitions for a dispatched frame without drawing.
     *
     * The frame clock delivers the frame timestamp to the host *before* the draw is
     * rendered (the draw itself is asynchronous via invalidate → vsync). Running the
     * anchor/completion transitions here — with the same timestamp the draw will use —
     * makes the animation state deterministic at dispatch time instead of depending on
     * when the draw happens to be delivered. The draw path's own tick() repeats these
     * transitions idempotently, so production (where dispatch and draw share one vsync
     * timestamp) behaves identically to before.
     */
    fun onFrameTick(frameTimeMs: Long) {
        animationEngine.markFirstVisibleFrame(frameTimeMs)
        animationEngine.completeIfFinished(frameTimeMs)
    }

    fun hasActiveAnimation(): Boolean = animationEngine.hasActiveAnimation()

    fun currentTimeNanos(): Long = animationEngine.currentTimeNanos()

    fun setAnimationPolicy(policy: TextAnimationPolicy) {
        animationEngine.setAnimationPolicy(policy)
    }

    fun setSmoothCursor(enabled: Boolean, durationMs: Long) {
        animationEngine.setSmoothCursor(enabled, durationMs)
    }

    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        animationEngine.setCoordinatedAnimationEnabled(enabled)
    }

    fun setReduceMotion(enabled: Boolean) {
        animationEngine.setReduceMotion(enabled)
    }

    fun pause(frameTimeMs: Long) {
        animationEngine.pause(frameTimeMs)
    }

    fun resume(frameTimeMs: Long) {
        animationEngine.resume(frameTimeMs)
    }

    fun isAnimationPaused(): Boolean = animationEngine.isPaused()

    fun tick(
        frameTimeMs: Long,
        layout: android.text.Layout?,
        layoutRevision: AndroidLayoutRevision?,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float,
        cursorVisible: Boolean,
        selectionAllowed: Boolean,
        cursorUtf16: Int,
        selectionStartUtf16: Int,
        selectionEndUtf16: Int
    ): FrameState? {
        if (layout == null) return null
        val transaction = animationEngine.getActiveTransaction()
        val progress = animationEngine.getTimelineProgress(frameTimeMs)
        val cursorProgress = animationEngine.getCursorTimelineProgress(frameTimeMs)
        animationEngine.markFirstVisibleFrame(frameTimeMs)
        val completed = transaction != null && animationEngine.isTimelineCompleted(frameTimeMs)
        // A completed (terminal) transaction must not be rendered: its slices would be
        // drawn over the static new-layout text, double-drawing glyphs on stray invalidates.
        // The static renderer alone produces the identical final visual state.
        val renderTransaction = if (completed) null else transaction
        val renderInput = FrameRenderInput(
            layout = layout,
            layoutRevision = layoutRevision,
            transaction = renderTransaction,
            timelineProgress = progress,
            cursorProgress = cursorProgress,
            searchHighlightsUtf16 = searchHighlightsUtf16,
            viewportWidth = viewportWidth,
            viewportHeight = viewportHeight,
            scrollX = scrollX,
            scrollY = scrollY,
            cursorVisible = cursorVisible,
            selectionAllowed = selectionAllowed,
            cursorUtf16 = cursorUtf16,
            selectionStartUtf16 = selectionStartUtf16,
            selectionEndUtf16 = selectionEndUtf16
        )
        return FrameState(renderInput, completeAfterDraw = completed)
    }
}
