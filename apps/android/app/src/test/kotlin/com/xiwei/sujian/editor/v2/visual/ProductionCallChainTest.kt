package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class ProductionCallChainTest {

    @Test
    fun choreographerFrameTimeFlowsThroughEngineTimeline() {
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            timeSource,
            transactionIdSource
        )
        engine.setAnimationPolicy(TextAnimationPolicy.ENABLED)
        val choreographerFrameTimeNanos = 10_000_000_000L
        val choreographerFrameTimeMs = choreographerFrameTimeNanos / 1_000_000
        timeSource.onFrameTimeNanos(choreographerFrameTimeNanos)
        val intent = com.xiwei.sujian.editor.v2.mirror.VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(0, 1)),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 200L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(0, 0, false)
        )
        val layoutEngine = com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine(
            com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror(),
            android.text.TextPaint()
        )
        engine.prepareAndSubmit(
            visualIntent = intent,
            layoutEngine = layoutEngine,
            mirrorUpdate = { layoutEngine.requestLayout() },
            frameTimeMs = choreographerFrameTimeMs
        )
        assertTrue(engine.hasActiveAnimation())
        // The timeline is anchored at the submission frame time (the frame that carried
        // the edit), so progress is the exact elapsed time since the edit. The first
        // draw after submission does not re-anchor it (idempotent).
        val firstDrawFrameTimeMs = choreographerFrameTimeMs + 16L
        engine.markFirstVisibleFrame(firstDrawFrameTimeMs)
        assertEquals(
            choreographerFrameTimeMs,
            engine.getActiveAnimationStartTimeMs()
        )
        assertEquals(
            0f,
            engine.getTimelineProgress(choreographerFrameTimeMs),
            0.01f
        )
        assertEquals(
            0.08f,
            engine.getTimelineProgress(firstDrawFrameTimeMs),
            0.01f
        )
        val midFrameTimeMs = choreographerFrameTimeMs + 100L
        assertEquals(
            0.5f,
            engine.getTimelineProgress(midFrameTimeMs),
            0.01f
        )
        assertFalse(
            engine.isTimelineCompleted(midFrameTimeMs)
        )
        val threeQuarterFrameTimeMs = choreographerFrameTimeMs + 150L
        assertEquals(
            0.75f,
            engine.getTimelineProgress(threeQuarterFrameTimeMs),
            0.01f
        )
        val endFrameTimeMs = choreographerFrameTimeMs + 200L
        assertEquals(
            1f,
            engine.getTimelineProgress(endFrameTimeMs),
            0.01f
        )
        assertTrue(
            engine.isTimelineCompleted(endFrameTimeMs)
        )
    }

    @Test
    fun choreographerFrameTimeNotSystemTime() {
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            timeSource,
            transactionIdSource
        )
        engine.setAnimationPolicy(TextAnimationPolicy.ENABLED)
        val choreographerFrameTimeNanos = 100_000_000_000L
        val choreographerFrameTimeMs = choreographerFrameTimeNanos / 1_000_000
        timeSource.onFrameTimeNanos(choreographerFrameTimeNanos)
        val intent = com.xiwei.sujian.editor.v2.mirror.VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(0, 1)),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 300L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(0, 0, false)
        )
        val layoutEngine = com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine(
            com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror(),
            android.text.TextPaint()
        )
        engine.prepareAndSubmit(
            visualIntent = intent,
            layoutEngine = layoutEngine,
            mirrorUpdate = { layoutEngine.requestLayout() },
            frameTimeMs = choreographerFrameTimeMs
        )
        engine.markFirstVisibleFrame(choreographerFrameTimeMs)
        val systemTimeMs = System.nanoTime() / 1_000_000
        val engineProgress = engine.getTimelineProgress(choreographerFrameTimeMs + 150L)
        assertEquals(
            0.5f,
            engineProgress,
            0.01f
        )
        assertTrue(
            kotlin.math.abs(choreographerFrameTimeMs - systemTimeMs) > 1000L
        )
    }

    @Test
    fun tickFrameTimePropagatesToTimelineThroughVisualRuntime() {
        val timeSource = ChoreographerAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime(timeSource, transactionIdSource)
        runtime.setAnimationPolicy(TextAnimationPolicy.ENABLED)
        val frameTimeNanos = 20_000_000_000L
        val frameTimeMs = frameTimeNanos / 1_000_000
        timeSource.onFrameTimeNanos(frameTimeNanos)
        val intent = com.xiwei.sujian.editor.v2.mirror.VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(0, 1)),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 200L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(0, 0, false)
        )
        val layoutEngine = com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine(
            com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror(),
            android.text.TextPaint()
        )
        runtime.prepareAndSubmit(
            visualIntent = intent,
            layoutEngine = layoutEngine,
            mirrorUpdate = { layoutEngine.requestLayout() },
            frameTimeMs = frameTimeMs
        )
        assertTrue(runtime.hasActiveAnimation())
        val layout = android.text.StaticLayout(
            "hello", android.text.TextPaint(), 100,
            android.text.Layout.Alignment.ALIGN_NORMAL, 1f, 0f, false
        )
        // The timeline is anchored at the submission frame time; the first tick after
        // submission advances progress from there instead of re-anchoring.
        val firstDrawFrameTimeMs = frameTimeMs + 16L
        val frameState = runtime.tick(
            firstDrawFrameTimeMs,
            layout,
            null,
            emptyList(),
            100, 100,
            0f, 0f,
            true, true,
            5, 0, 0
        )
        assertNotNull(frameState)
        assertEquals(
            frameTimeMs,
            runtime.getActiveAnimationStartTimeMs()
        )
        assertEquals(
            0.08f,
            frameState!!.renderInput.timelineProgress,
            0.01f
        )
        val midFrameTimeMs = frameTimeMs + 100L
        val midFrameState = runtime.tick(
            midFrameTimeMs,
            layout,
            null,
            emptyList(),
            100, 100,
            0f, 0f,
            true, true,
            5, 0, 0
        )
        assertNotNull(midFrameState)
        assertEquals(
            0.5f,
            midFrameState!!.renderInput.timelineProgress,
            0.01f
        )
    }
}
