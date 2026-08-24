package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.layout.AndroidLayoutEngine
import com.xiwei.sujian.feature.editor.layout.LayoutRevisionSource
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.feature.editor.visual.AndroidVisualPlanner
import com.xiwei.sujian.feature.editor.visual.TextAnimationPolicy
import com.xiwei.sujian.feature.editor.visual.VisualResourceStore

class AndroidVisualRuntime(
    private val visualPlanner: AndroidVisualPlanner,
    private val animationEngine: AndroidTextAnimationEngine,
    private val resourceStore: VisualResourceStore,
) {
    constructor(
        timeSource: com.xiwei.sujian.feature.editor.visual.AnimationTimeSource =
            com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource(),
        transactionIdSource: com.xiwei.sujian.feature.editor.visual.TransactionIdSource =
            com.xiwei.sujian.feature.editor.visual.TransactionIdSource(),
    ) : this(
        AndroidVisualPlanner(),
        VisualResourceStore(),
        timeSource,
        transactionIdSource,
    )

    constructor(visualPlanner: AndroidVisualPlanner, resourceStore: VisualResourceStore) : this(
        visualPlanner,
        AndroidTextAnimationEngine(visualPlanner, resourceStore),
        resourceStore,
    )

    /**
     * #606: 以自定义 visualPlanner（含 Core rebase mapping provider）构造。
     * 生产路径由 [AndroidEditorPipeline.create] 注入 Core 计算结果。
     */
    constructor(
        visualPlanner: AndroidVisualPlanner,
        timeSource: com.xiwei.sujian.feature.editor.visual.AnimationTimeSource,
        transactionIdSource: com.xiwei.sujian.feature.editor.visual.TransactionIdSource,
    ) : this(
        visualPlanner,
        VisualResourceStore(),
        timeSource,
        transactionIdSource,
    )

    constructor(
        visualPlanner: AndroidVisualPlanner,
        resourceStore: VisualResourceStore,
        timeSource: com.xiwei.sujian.feature.editor.visual.AnimationTimeSource,
        transactionIdSource: com.xiwei.sujian.feature.editor.visual.TransactionIdSource,
    ) : this(
        visualPlanner,
        AndroidTextAnimationEngine(visualPlanner, resourceStore, timeSource, transactionIdSource),
        resourceStore,
    )

    fun prepareAndSubmit(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)? = null,
        beforePatch: (() -> Unit)? = null,
        frameTimeMs: Long? = null,
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

    fun setSmoothCursor(
        enabled: Boolean,
        durationMs: Long,
    ) {
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

    /**
     * #638：获取当前帧的视觉光标 Rect。
     *
     * 委托给 [AndroidTextAnimationEngine.currentVisualCursorRect]，
     * 无活跃事务/光标未动画时返回 null（表示应使用静态光标）。
     */
    fun currentVisualCursorRect(frameTimeMs: Long): android.graphics.RectF? =
        animationEngine.currentVisualCursorRect(frameTimeMs)

    /**
     * #638：获取当前活跃事务 ID。
     *
     * 无活跃事务时返回 null。供 viewportRetarget 调用方按 transactionId 去重，
     * 避免每帧重复记录。API 前置：origin/fix/issue-638-diagnostics 合入后
     * 可调用 DiagnosticsEvents.viewportRetarget。
     */
    fun getCurrentTransactionId(): Long? =
        animationEngine.getActiveTransaction()?.transactionId

    /**
     * #638：获取静态光标矩形。
     *
     * 查询当前 layout 和光标位置，生成静态光标矩形。
     * 无活跃事务/光标未动画时回退到此静态位置。
     */
    fun getStaticCursorRect(
        layout: android.text.Layout,
        cursorUtf16: Int,
    ): android.graphics.RectF? {
        if (cursorUtf16 < 0 || cursorUtf16 > layout.text.length) return null
        val line = layout.getLineForOffset(cursorUtf16)
        val lineTop = layout.getLineTop(line).toFloat()
        val lineBottom = layout.getLineBottom(line).toFloat()
        return android.graphics.RectF(0f, lineTop, 2f, lineBottom)
    }

    fun tick(
        frameTimeMs: Long,
        layout: android.text.Layout?,
        layoutRevision: LayoutRevisionSource?,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float,
        cursorVisible: Boolean,
        selectionAllowed: Boolean,
        cursorUtf16: Int,
        selectionStartUtf16: Int,
        selectionEndUtf16: Int,
    ): FrameState? {
        if (layout == null) return null
        val transaction = animationEngine.getActiveTransaction()
        val progress = animationEngine.getTimelineProgress(frameTimeMs)
        val cursorProgress = animationEngine.getCursorProgress(frameTimeMs)
        animationEngine.markFirstVisibleFrame(frameTimeMs)
        // #637 评论 5386066978 项3：coordinated=true 时文字和光标共用同一个 visual
        // completion — textFinished == cursorFinished，由同一 timeline progress 决定。
        // 不出现"最新静态文字 + 旧动画光标"：文字切静态当且仅当光标也切静态。
        // coordinated=false 才允许 textFinished/cursorFinished 分开（独立 cursorTimeline）。
        val textFinished = animationEngine.isTextTimelineCompleted(frameTimeMs)
        val cursorFinished = animationEngine.isCursorTimelineCompleted(frameTimeMs)
        val transactionComplete = transaction != null && textFinished && cursorFinished
        val renderTransaction = if (textFinished) null else transaction
        val renderCursorTransition =
            if (cursorFinished) {
                null
            } else {
                transaction?.cursorTransition?.takeIf { it.shouldAnimate }
            }
        val renderInput =
            FrameRenderInput(
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
                selectionEndUtf16 = selectionEndUtf16,
            )
        return FrameState(renderInput, completeAfterDraw = transactionComplete)
    }

    /**
     * #595 五：获取视觉事务文字轨和光标轨的明确终态 — 供宿主查询渲染策略。
     */
    fun visualTrackState(frameTimeMs: Long): com.xiwei.sujian.feature.editor.motion.VisualTrackState {
        val transaction = animationEngine.getActiveTransaction()
        if (transaction == null) return com.xiwei.sujian.feature.editor.motion.VisualTrackState.Idle
        val textFinished = animationEngine.isTextTimelineCompleted(frameTimeMs)
        val cursorFinished = animationEngine.isCursorTimelineCompleted(frameTimeMs)
        val transactionComplete = textFinished && cursorFinished
        return com.xiwei.sujian.feature.editor.motion.VisualTrackState(
            renderTextTransaction = if (textFinished) null else transaction,
            renderCursorTransition = !cursorFinished,
            textProgress = animationEngine.getTimelineProgress(frameTimeMs),
            cursorProgress = animationEngine.getCursorProgress(frameTimeMs),
            textFinished = textFinished,
            cursorFinished = cursorFinished,
            transactionComplete = transactionComplete,
        )
    }
}
