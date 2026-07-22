package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidVisualRuntime(
    val visualPlanner: AndroidVisualPlanner,
    val animationEngine: AndroidTextAnimationEngine,
    val resourceStore: VisualResourceStore
) {
    constructor() : this(
        AndroidVisualPlanner(),
        VisualResourceStore()
    )

    constructor(visualPlanner: AndroidVisualPlanner, resourceStore: VisualResourceStore) : this(
        visualPlanner,
        AndroidTextAnimationEngine(visualPlanner, resourceStore),
        resourceStore
    )

    fun prepareAndSubmit(
        visualIntent: VisualIntent,
        layoutEngine: AndroidLayoutEngine,
        mirrorUpdate: (() -> Unit)? = null,
        beforePatch: (() -> Unit)? = null
    ) {
        animationEngine.prepareAndSubmit(visualIntent, layoutEngine, mirrorUpdate, beforePatch)
    }

    fun cancel() {
        animationEngine.cancel()
    }

    fun release() {
        animationEngine.release()
    }

    fun hasActiveAnimation(): Boolean = animationEngine.hasActiveAnimation()

    fun getActiveTransaction(): PreparedVisualTransaction? = animationEngine.getActiveTransaction()

    fun getTimelineProgress(frameTimeMs: Long): Float = animationEngine.getTimelineProgress(frameTimeMs)

    fun markFirstVisibleFrame(frameTimeMs: Long) {
        animationEngine.markFirstVisibleFrame(frameTimeMs)
    }

    fun completeIfFinished(frameTimeMs: Long) {
        animationEngine.completeIfFinished(frameTimeMs)
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
        mirror: DisplayTextMirror
    ): FrameState? {
        if (layout == null) return null
        val transaction = animationEngine.getActiveTransaction()
        val progress = animationEngine.getTimelineProgress(frameTimeMs)
        animationEngine.markFirstVisibleFrame(frameTimeMs)
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
            mirror = mirror
        )
        animationEngine.completeIfFinished(frameTimeMs)
        return FrameState(renderInput)
    }
}
