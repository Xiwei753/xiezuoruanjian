package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction

class AndroidVisualRuntime(
    val visualPlanner: AndroidVisualPlanner,
    val animationEngine: AndroidTextAnimationEngine,
    val resourceStore: VisualResourceStore
) {
    constructor() : this(
        AndroidVisualPlanner(),
        AndroidTextAnimationEngine(AndroidVisualPlanner(), VisualResourceStore()),
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
}
