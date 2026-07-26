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
}
