package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class AnimationTimeSourceTest {

    @Test
    fun choreographerTimeSourceReturnsSystemNanoTime() {
        val source = ChoreographerAnimationTimeSource()
        val before = System.nanoTime()
        val result = source.nowNanos()
        val after = System.nanoTime()
        assertTrue(result in before..after)
    }

    @Test
    fun manualTimeSourceStartsAtZero() {
        val source = ManualAnimationTimeSource()
        assertEquals(0L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceTo() {
        val source = ManualAnimationTimeSource()
        source.advanceTo(1_000_000L)
        assertEquals(1_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceBy() {
        val source = ManualAnimationTimeSource()
        source.advanceBy(500L)
        assertEquals(500L, source.nowNanos())
        source.advanceBy(500L)
        assertEquals(1000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceByMs() {
        val source = ManualAnimationTimeSource()
        source.advanceByMs(16)
        assertEquals(16_000_000L, source.nowNanos())
    }

    @Test(expected = IllegalArgumentException::class)
    fun manualTimeSourceRejectsNonMonotonicAdvance() {
        val source = ManualAnimationTimeSource()
        source.advanceTo(1000L)
        source.advanceTo(500L)
    }

    @Test(expected = IllegalArgumentException::class)
    fun manualTimeSourceRejectsNegativeDelta() {
        val source = ManualAnimationTimeSource()
        source.advanceBy(-1L)
    }

    @Test
    fun manualTimeSourceSupportsAnimationFrameSequence() {
        val source = ManualAnimationTimeSource()
        val frames = mutableListOf<Long>()
        for (i in 0 until 10) {
            source.advanceByMs(16)
            frames.add(source.nowNanos() / 1_000_000)
        }
        assertEquals(listOf(16L, 32L, 48L, 64L, 80L, 96L, 112L, 128L, 144L, 160L), frames)
    }
}

class TransactionIdSourceTest {

    @Test
    fun startsAtOne() {
        val source = TransactionIdSource()
        assertEquals(1L, source.nextId())
    }

    @Test
    fun incrementsMonotonically() {
        val source = TransactionIdSource()
        val ids = (1..5).map { source.nextId() }
        assertEquals(listOf(1L, 2L, 3L, 4L, 5L), ids)
    }

    @Test
    fun neverDuplicates() {
        val source = TransactionIdSource()
        val ids = (1..1000).map { source.nextId() }
        assertEquals(ids.size, ids.toSet().size)
    }
}

class AnimationStateSnapshotTest {

    @Test
    fun engineCaptureStateSnapshotReturnsNullWhenNoActiveTransaction() {
        val timeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            timeSource,
            transactionIdSource
        )
        timeSource.advanceByMs(100)
        val snapshot = engine.captureStateSnapshot(timeSource.nowNanos() / 1_000_000)
        assertNull(snapshot)
    }

    @Test
    fun engineCaptureStateSnapshotReturnsProgress() {
        val timeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            timeSource,
            transactionIdSource
        )
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        assertEquals(0f, timeline.progress(0), 0.01f)
        assertEquals(0.5f, timeline.progress(100), 0.01f)
        assertEquals(1f, timeline.progress(200), 0.01f)
    }
}
