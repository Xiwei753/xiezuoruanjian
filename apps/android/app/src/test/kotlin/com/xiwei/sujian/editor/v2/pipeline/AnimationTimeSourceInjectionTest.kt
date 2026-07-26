package com.xiwei.sujian.editor.v2.pipeline

import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.visual.AnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.Assert.*
import org.junit.Test

class AnimationTimeSourceInjectionTest {

    @Test
    fun pipelineCreateWithDefaultTimeSourceUsesChoreographer() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val pipeline = AndroidEditorPipeline.create(mirror, paint)
        assertNotNull(pipeline)
    }

    @Test
    fun pipelineCreateWithManualTimeSourceAcceptsInjection() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, manualTimeSource, transactionIdSource)
        assertNotNull(pipeline)
    }

    @Test
    fun pipelineCreateWithExplicitChoreographerTimeSource() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val choreographerSource = ChoreographerAnimationTimeSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, choreographerSource)
        assertNotNull(pipeline)
    }

    @Test
    fun manualTimeSourceAdvancesIndependentlyOfPipeline() {
        val manualTimeSource = ManualAnimationTimeSource()
        manualTimeSource.advanceByMs(16)
        assertEquals(16_000_000L, manualTimeSource.nowNanos())
        manualTimeSource.advanceByMs(16)
        assertEquals(32_000_000L, manualTimeSource.nowNanos())
    }

    @Test
    fun transactionIdSourceProducesMonotonicallyIncreasingIds() {
        val source = TransactionIdSource()
        val id1 = source.nextId()
        val id2 = source.nextId()
        val id3 = source.nextId()
        assertTrue(id1 < id2)
        assertTrue(id2 < id3)
    }

    @Test
    fun transactionIdSourceStartsAtOne() {
        val source = TransactionIdSource()
        assertEquals(1L, source.nextId())
    }

    @Test
    fun pipelineWithManualTimeSource_reportsInjectedTime() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, manualTimeSource, transactionIdSource)

        assertEquals(0L, pipeline.currentTimeNanos())

        manualTimeSource.advanceByMs(16)
        assertEquals(16_000_000L, pipeline.currentTimeNanos())

        manualTimeSource.advanceByMs(16)
        assertEquals(32_000_000L, pipeline.currentTimeNanos())
    }

    @Test
    fun pipelineWithManualTimeSource_snapshotIsNullWithoutActiveTransaction() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, manualTimeSource, transactionIdSource)

        manualTimeSource.advanceByMs(100)
        assertNull(pipeline.captureAnimationSnapshot())
    }

    @Test
    fun pipelineWithManualTimeSource_noActiveAnimationInitially() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, manualTimeSource, transactionIdSource)

        assertFalse(pipeline.hasActiveAnimation())
    }

    @Test
    fun pipelineWithDefaultTimeSource_reportsSystemTime() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val pipeline = AndroidEditorPipeline.create(mirror, paint)

        val before = System.nanoTime()
        val reported = pipeline.currentTimeNanos()
        val after = System.nanoTime()
        assertTrue("Reported time should be within system nanoTime range", reported in before..after)
    }

    @Test
    fun pipelineWithManualTimeSource_timeAdvancesDeterministically() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, manualTimeSource, transactionIdSource)

        val timestamps = mutableListOf<Long>()
        for (i in 0 until 5) {
            manualTimeSource.advanceByMs(16)
            timestamps.add(pipeline.currentTimeNanos() / 1_000_000)
        }
        assertEquals(listOf(16L, 32L, 48L, 64L, 80L), timestamps)
    }

    @Test
    fun pipelineWithManualTimeSource_visualFrameSnapshotIsNullWithoutActiveTransaction() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, manualTimeSource, transactionIdSource)

        manualTimeSource.advanceByMs(100)
        assertNull(pipeline.captureVisualFrameSnapshot())
    }

    @Test
    fun pipelineWithManualTimeSource_stateAndVisualFrameSnapshotsConsistentWhenNoAnimation() {
        val mirror = DisplayTextMirror()
        val paint = TextPaint()
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val pipeline = AndroidEditorPipeline.create(mirror, paint, manualTimeSource, transactionIdSource)

        manualTimeSource.advanceByMs(100)
        val stateSnapshot = pipeline.captureAnimationSnapshot()
        val visualSnapshot = pipeline.captureVisualFrameSnapshot()
        assertNull(stateSnapshot)
        assertNull(visualSnapshot)
    }
}
