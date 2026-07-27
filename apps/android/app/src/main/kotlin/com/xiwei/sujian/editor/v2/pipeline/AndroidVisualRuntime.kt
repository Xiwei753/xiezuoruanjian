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

    fun hasActiveAnimation(): Boolean = animationEngine.hasActiveAnimation()

    fun currentTimeNanos(): Long = animationEngine.currentTimeNanos()

    fun captureStateSnapshot(): com.xiwei.sujian.editor.v2.visual.AnimationStateSnapshot? {
        return animationEngine.captureStateSnapshot(currentTimeNanos() / 1_000_000)
    }

    fun captureVisualFrameSnapshot(): com.xiwei.sujian.editor.v2.visual.VisualFrameSnapshot? {
        return animationEngine.captureFrame(currentTimeNanos() / 1_000_000)
    }

    fun getActiveAnimationDurationMs(): Long {
        return animationEngine.getActiveTransaction()?.durationMs ?: 0L
    }

    fun getActiveAnimationStartTimeMs(): Long? {
        return animationEngine.getActiveAnimationStartTimeMs()
    }

    fun setAnimationPolicy(policy: TextAnimationPolicy) {
        animationEngine.setAnimationPolicy(policy)
    }

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
        animationEngine.markFirstVisibleFrame(frameTimeMs)
        val shouldCompleteAfterDraw = transaction != null && animationEngine.isTimelineCompleted(frameTimeMs)
        val renderInput = FrameRenderInput(
            layout = layout,
            layoutRevision = layoutRevision,
            transaction = transaction,
            timelineProgress = progress,
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
        return FrameState(renderInput, completeAfterDraw = shouldCompleteAfterDraw)
    }
}
