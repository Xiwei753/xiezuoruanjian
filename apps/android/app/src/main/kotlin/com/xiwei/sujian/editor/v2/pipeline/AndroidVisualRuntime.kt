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
        // #595 五：文字轨和光标轨分别判断终态。
        // 文字完成后用静态新布局继续绘制，但光标仍在同一个 View 和 FrameClock 中
        // 平滑移动到终点；只有文字轨和光标轨都结束，整个视觉事务才进入终态。
        val textFinished = animationEngine.isTextTimelineCompleted(frameTimeMs)
        val cursorFinished = animationEngine.isCursorTimelineCompleted(frameTimeMs)
        val transactionComplete = transaction != null && textFinished && cursorFinished
        // 文字完成后不渲染文字切片（避免 double-draw），但光标仍可继续动画。
        // #595 五：cursorTransition 独立于文字切片 — 文字轨结束/抑制（CursorOnly）时
        // 静态文字路径仍能绘制平滑光标；光标轨结束才置 null（回到静态光标）。
        val renderTransaction = if (textFinished) null else transaction
        val renderCursorTransition = if (cursorFinished) null
        else transaction?.cursorTransition?.takeIf { it.shouldAnimate }
        val renderInput = FrameRenderInput(
            layout = layout,
            layoutRevision = layoutRevision,
            transaction = renderTransaction,
            cursorTransition = renderCursorTransition,
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
        return FrameState(renderInput, completeAfterDraw = transactionComplete)
    }

    /**
     * #595 五：获取视觉事务文字轨和光标轨的明确终态 — 供宿主查询渲染策略。
     */
    fun visualTrackState(frameTimeMs: Long): com.xiwei.sujian.editor.v2.motion.VisualTrackState {
        val transaction = animationEngine.getActiveTransaction()
        if (transaction == null) return com.xiwei.sujian.editor.v2.motion.VisualTrackState.Idle
        val textFinished = animationEngine.isTextTimelineCompleted(frameTimeMs)
        val cursorFinished = animationEngine.isCursorTimelineCompleted(frameTimeMs)
        val transactionComplete = textFinished && cursorFinished
        return com.xiwei.sujian.editor.v2.motion.VisualTrackState(
            renderTextTransaction = if (textFinished) null else transaction,
            renderCursorTransition = !cursorFinished,
            textProgress = animationEngine.getTimelineProgress(frameTimeMs),
            cursorProgress = animationEngine.getCursorTimelineProgress(frameTimeMs),
            textFinished = textFinished,
            cursorFinished = cursorFinished,
            transactionComplete = transactionComplete,
        )
    }
}
