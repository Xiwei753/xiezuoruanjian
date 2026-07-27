package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class AnimationTimeSourceTest {

    @Test
    fun choreographerTimeSourceReturnsSystemNanoTimeBeforeFrame() {
        val source = ChoreographerAnimationTimeSource()
        val before = System.nanoTime()
        val result = source.nowNanos()
        val after = System.nanoTime()
        assertTrue(result in before..after)
    }

    @Test
    fun choreographerTimeSourceReturnsFrameTimeAfterOnFrame() {
        val source = ChoreographerAnimationTimeSource()
        val frameTime = 1_000_000_000L
        source.onFrameTimeNanos(frameTime)
        assertEquals(frameTime, source.nowNanos())
    }

    @Test
    fun choreographerTimeSourceUpdatesOnEachFrame() {
        val source = ChoreographerAnimationTimeSource()
        source.onFrameTimeNanos(1_000_000_000L)
        assertEquals(1_000_000_000L, source.nowNanos())
        source.onFrameTimeNanos(1_016_000_000L)
        assertEquals(1_016_000_000L, source.nowNanos())
    }

    @Test
    fun choreographerTimeSourcePrefersFrameTimeOverSystemTime() {
        val source = ChoreographerAnimationTimeSource()
        val frameTime = 500_000_000L
        source.onFrameTimeNanos(frameTime)
        assertEquals(frameTime, source.nowNanos())
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

    @Test
    fun manualTimeSourceAdvanceToProgress25Percent() {
        val source = ManualAnimationTimeSource()
        val durationMs = 200L
        source.advanceByMs(durationMs / 4)
        val progress = source.nowNanos() / 1_000_000.toFloat() / durationMs
        assertEquals(0.25f, progress, 0.01f)
    }

    @Test
    fun manualTimeSourceAdvanceToProgress50Percent() {
        val source = ManualAnimationTimeSource()
        val durationMs = 200L
        source.advanceByMs(durationMs / 2)
        val progress = source.nowNanos() / 1_000_000.toFloat() / durationMs
        assertEquals(0.5f, progress, 0.01f)
    }

    @Test
    fun manualTimeSourceAdvanceToProgress75Percent() {
        val source = ManualAnimationTimeSource()
        val durationMs = 200L
        source.advanceByMs((durationMs * 3) / 4)
        val progress = source.nowNanos() / 1_000_000.toFloat() / durationMs
        assertEquals(0.75f, progress, 0.01f)
    }

    @Test
    fun manualTimeSourceAdvanceToEnd() {
        val source = ManualAnimationTimeSource()
        val durationMs = 200L
        source.advanceByMs(durationMs)
        val progress = source.nowNanos() / 1_000_000.toFloat() / durationMs
        assertEquals(1.0f, progress, 0.01f)
    }

    @Test
    fun manualTimeSourceSimulatesRapidConsecutiveInput() {
        val source = ManualAnimationTimeSource()
        val durations = listOf(16L, 16L, 16L, 16L, 16L)
        val timestamps = mutableListOf<Long>()
        for (d in durations) {
            source.advanceByMs(d)
            timestamps.add(source.nowNanos() / 1_000_000)
        }
        assertEquals(listOf(16L, 32L, 48L, 64L, 80L), timestamps)
    }

    @Test
    fun manualTimeSourceAdvanceToProgressAt25() {
        val source = ManualAnimationTimeSource()
        source.advanceToProgress(0.25f, 200L)
        assertEquals(50_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToProgressAt50() {
        val source = ManualAnimationTimeSource()
        source.advanceToProgress(0.5f, 200L)
        assertEquals(100_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToProgressAt75() {
        val source = ManualAnimationTimeSource()
        source.advanceToProgress(0.75f, 200L)
        assertEquals(150_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToProgressAtZero() {
        val source = ManualAnimationTimeSource()
        source.advanceToProgress(0f, 200L)
        assertEquals(0L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToProgressAtOne() {
        val source = ManualAnimationTimeSource()
        source.advanceToProgress(1f, 200L)
        assertEquals(200_000_000L, source.nowNanos())
    }

    @Test(expected = IllegalArgumentException::class)
    fun manualTimeSourceAdvanceToProgressRejectsNegativeProgress() {
        ManualAnimationTimeSource().advanceToProgress(-0.1f, 200L)
    }

    @Test(expected = IllegalArgumentException::class)
    fun manualTimeSourceAdvanceToProgressRejectsProgressAboveOne() {
        ManualAnimationTimeSource().advanceToProgress(1.1f, 200L)
    }

    @Test(expected = IllegalArgumentException::class)
    fun manualTimeSourceAdvanceToProgressRejectsNegativeDuration() {
        ManualAnimationTimeSource().advanceToProgress(0.5f, -1L)
    }

    @Test
    fun manualTimeSourceAdvanceToEndConvenience() {
        val source = ManualAnimationTimeSource()
        source.advanceToEnd(200L)
        assertEquals(200_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToProgressWithStartTime() {
        val source = ManualAnimationTimeSource()
        source.advanceToProgress(0.25f, 200L, startTimeMs = 16L)
        assertEquals(66_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToProgressWithStartTimeSequence() {
        val source = ManualAnimationTimeSource()
        val durationMs = 200L
        val startTimeMs = 16L
        source.advanceToProgress(0f, durationMs, startTimeMs)
        assertEquals(16_000_000L, source.nowNanos())
        source.advanceToProgress(0.25f, durationMs, startTimeMs)
        assertEquals(66_000_000L, source.nowNanos())
        source.advanceToProgress(0.5f, durationMs, startTimeMs)
        assertEquals(116_000_000L, source.nowNanos())
        source.advanceToProgress(0.75f, durationMs, startTimeMs)
        assertEquals(166_000_000L, source.nowNanos())
        source.advanceToProgress(1f, durationMs, startTimeMs)
        assertEquals(216_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToEndWithStartTime() {
        val source = ManualAnimationTimeSource()
        source.advanceToEnd(200L, startTimeMs = 16L)
        assertEquals(216_000_000L, source.nowNanos())
    }

    @Test
    fun manualTimeSourceAdvanceToProgressSequence() {
        val source = ManualAnimationTimeSource()
        val durationMs = 200L
        source.advanceToProgress(0f, durationMs)
        assertEquals(0L, source.nowNanos())
        source.advanceToProgress(0.25f, durationMs)
        assertEquals(50_000_000L, source.nowNanos())
        source.advanceToProgress(0.5f, durationMs)
        assertEquals(100_000_000L, source.nowNanos())
        source.advanceToProgress(0.75f, durationMs)
        assertEquals(150_000_000L, source.nowNanos())
        source.advanceToProgress(1f, durationMs)
        assertEquals(200_000_000L, source.nowNanos())
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

    @Test
    fun transactionIdsAreIndependentOfTimeSource() {
        val timeSource = ManualAnimationTimeSource()
        val idSource = TransactionIdSource()
        timeSource.advanceByMs(0)
        val id1 = idSource.nextId()
        timeSource.advanceByMs(0)
        val id2 = idSource.nextId()
        assertNotEquals(id1, id2)
    }
}

class AnimationTimelineControlledFrameTest {

    @Test
    fun timelineFirstVisibleFrameTimeIsNullBeforeMark() {
        val timeline = AnimationTimeline(200L)
        assertNull(timeline.getFirstVisibleFrameTimeMs())
    }

    @Test
    fun timelineFirstVisibleFrameTimeAfterMark() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(16)
        assertEquals(16L, timeline.getFirstVisibleFrameTimeMs())
    }

    @Test
    fun timelineProgressAtStartFrame() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        assertEquals(0f, timeline.progress(0), 0.01f)
    }

    @Test
    fun timelineProgressAt25Percent() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        assertEquals(0.25f, timeline.progress(50), 0.01f)
    }

    @Test
    fun timelineProgressAt50Percent() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        assertEquals(0.5f, timeline.progress(100), 0.01f)
    }

    @Test
    fun timelineProgressAt75Percent() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        assertEquals(0.75f, timeline.progress(150), 0.01f)
    }

    @Test
    fun timelineProgressAtEndFrame() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        assertEquals(1f, timeline.progress(200), 0.01f)
    }

    @Test
    fun timelineProgressWithManualTimeSource() {
        val timeSource = ManualAnimationTimeSource()
        val timeline = AnimationTimeline(200L)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(50)
        assertEquals(0.25f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeSource.advanceByMs(50)
        assertEquals(0.5f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeSource.advanceByMs(50)
        assertEquals(0.75f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeSource.advanceByMs(50)
        assertEquals(1f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
    }

    @Test
    fun timelineStateTransitionsPendingToRendering() {
        val timeline = AnimationTimeline(200L)
        assertEquals(TransactionState.Pending, timeline.getState())
        timeline.markFirstVisibleFrame(0)
        assertEquals(TransactionState.Rendering, timeline.getState())
    }

    @Test
    fun timelineStateTransitionsToCompleted() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        timeline.complete()
        assertEquals(TransactionState.Completed, timeline.getState())
    }

    @Test
    fun timelineStateTransitionsToCancelled() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        timeline.cancel()
        assertEquals(TransactionState.Cancelled, timeline.getState())
    }

    @Test
    fun timelinePauseAndResume() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        timeline.pause(50)
        assertEquals(TransactionState.Paused, timeline.getState())
        assertEquals(0.25f, timeline.progress(50), 0.01f)
        timeline.resume(100)
        assertEquals(TransactionState.Rendering, timeline.getState())
    }

    @Test
    fun timelineZeroDurationCompletesInstantly() {
        val timeline = AnimationTimeline(0L)
        timeline.markFirstVisibleFrame(0)
        assertEquals(1f, timeline.progress(0), 0.01f)
    }

    @Test
    fun timelineIsCompletedWhenProgressReachesOne() {
        val timeline = AnimationTimeline(200L)
        timeline.markFirstVisibleFrame(0)
        assertFalse(timeline.isCompleted(100))
        assertTrue(timeline.isCompleted(200))
    }

    @Test
    fun timelineSimulateRapidConsecutiveInput() {
        val timeSource = ManualAnimationTimeSource()
        val timeline1 = AnimationTimeline(160L)
        timeSource.advanceByMs(0)
        timeline1.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(80)
        val progress1 = timeline1.progress(timeSource.nowNanos() / 1_000_000)
        assertEquals(0.5f, progress1, 0.01f)
        timeline1.complete()
        val timeline2 = AnimationTimeline(160L)
        timeSource.advanceByMs(0)
        timeline2.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(160)
        val progress2 = timeline2.progress(timeSource.nowNanos() / 1_000_000)
        assertEquals(1f, progress2, 0.01f)
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

    @Test
    fun engineCurrentTimeNanosUsesInjectedTimeSource() {
        val timeSource = ManualAnimationTimeSource()
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            timeSource
        )
        assertEquals(0L, engine.currentTimeNanos())
        timeSource.advanceByMs(16)
        assertEquals(16_000_000L, engine.currentTimeNanos())
    }

    @Test
    fun engineCurrentTimeNanosDefaultUsesChoreographerSource() {
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore()
        )
        val before = System.nanoTime()
        val result = engine.currentTimeNanos()
        val after = System.nanoTime()
        assertTrue(result in before..after)
    }
}

class AnimationStateSnapshotDataTest {

    @Test
    fun snapshotContainsAllRequiredFields() {
        val snapshot = AnimationStateSnapshot(
            transactionId = 42L,
            operationKind = "insert",
            animationMode = "animated",
            oldAffectedRanges = listOf(Pair(0, 5)),
            newAffectedRanges = listOf(Pair(0, 10)),
            progress = 0.5f,
            sliceRoles = listOf(SliceRole.Insert),
            cursorTransition = CursorTransitionSnapshot(
                fromX = 0f, fromY = 0f, fromHeight = 20f,
                toX = 10f, toY = 0f, toHeight = 20f,
                shouldAnimate = true
            ),
            ownedResourceCount = 3,
            transactionState = TransactionState.Rendering
        )
        assertEquals(42L, snapshot.transactionId)
        assertEquals("insert", snapshot.operationKind)
        assertEquals("animated", snapshot.animationMode)
        assertEquals(listOf(Pair(0, 5)), snapshot.oldAffectedRanges)
        assertEquals(listOf(Pair(0, 10)), snapshot.newAffectedRanges)
        assertEquals(0.5f, snapshot.progress, 0.01f)
        assertEquals(listOf(SliceRole.Insert), snapshot.sliceRoles)
        assertNotNull(snapshot.cursorTransition)
        assertEquals(3, snapshot.ownedResourceCount)
        assertEquals(TransactionState.Rendering, snapshot.transactionState)
    }

    @Test
    fun snapshotOperationKindDistinguishesInsertDeleteReplace() {
        val insertSnapshot = AnimationStateSnapshot(
            transactionId = 1L, operationKind = "insert", animationMode = "animated",
            oldAffectedRanges = emptyList(), newAffectedRanges = listOf(Pair(0, 3)),
            progress = 0f, sliceRoles = listOf(SliceRole.Insert), cursorTransition = null,
            ownedResourceCount = 1, transactionState = TransactionState.Pending
        )
        val deleteSnapshot = insertSnapshot.copy(
            operationKind = "delete", newAffectedRanges = emptyList(),
            oldAffectedRanges = listOf(Pair(0, 3)), sliceRoles = listOf(SliceRole.Delete)
        )
        val replaceSnapshot = insertSnapshot.copy(
            operationKind = "replace",
            oldAffectedRanges = listOf(Pair(0, 3)), newAffectedRanges = listOf(Pair(0, 5)),
            sliceRoles = listOf(SliceRole.CrossfadeOld, SliceRole.CrossfadeNew)
        )
        assertNotEquals(insertSnapshot.operationKind, deleteSnapshot.operationKind)
        assertNotEquals(insertSnapshot.operationKind, replaceSnapshot.operationKind)
        assertNotEquals(deleteSnapshot.operationKind, replaceSnapshot.operationKind)
    }

    @Test
    fun snapshotOperationKindDistinguishesCompositionOperations() {
        val updateSnapshot = AnimationStateSnapshot(
            transactionId = 1L, operationKind = "composition_update", animationMode = "animated",
            oldAffectedRanges = listOf(Pair(0, 3)), newAffectedRanges = listOf(Pair(0, 6)),
            progress = 0f, sliceRoles = listOf(SliceRole.CrossfadeOld, SliceRole.CrossfadeNew),
            cursorTransition = null, ownedResourceCount = 2, transactionState = TransactionState.Pending
        )
        val commitSnapshot = updateSnapshot.copy(operationKind = "composition_commit")
        val cancelSnapshot = updateSnapshot.copy(
            operationKind = "composition_cancel",
            newAffectedRanges = emptyList(), sliceRoles = listOf(SliceRole.Delete)
        )
        assertEquals("composition_update", updateSnapshot.operationKind)
        assertEquals("composition_commit", commitSnapshot.operationKind)
        assertEquals("composition_cancel", cancelSnapshot.operationKind)
    }

    @Test
    fun cursorTransitionSnapshotCapturesFromAndTo() {
        val ct = CursorTransitionSnapshot(
            fromX = 10f, fromY = 20f, fromHeight = 30f,
            toX = 40f, toY = 50f, toHeight = 60f,
            shouldAnimate = true
        )
        assertEquals(10f, ct.fromX, 0.01f)
        assertEquals(20f, ct.fromY, 0.01f)
        assertEquals(30f, ct.fromHeight, 0.01f)
        assertEquals(40f, ct.toX, 0.01f)
        assertEquals(50f, ct.toY, 0.01f)
        assertEquals(60f, ct.toHeight, 0.01f)
        assertTrue(ct.shouldAnimate)
    }

    @Test
    fun cursorTransitionSnapshotNoAnimation() {
        val ct = CursorTransitionSnapshot(
            fromX = 10f, fromY = 20f, fromHeight = 30f,
            toX = 10f, toY = 20f, toHeight = 30f,
            shouldAnimate = false
        )
        assertFalse(ct.shouldAnimate)
    }
}

class DeterministicFrameProgressTest {

    @Test
    fun frameProgressSequenceForTypicalInsert() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 200L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        val progressPoints = mutableListOf<Float>()
        val checkpoints = listOf(0L, 50L, 100L, 150L, 200L)
        for (checkpoint in checkpoints) {
            timeSource.advanceTo(checkpoint * 1_000_000L)
            progressPoints.add(timeline.progress(timeSource.nowNanos() / 1_000_000))
        }
        assertEquals(0f, progressPoints[0], 0.01f)
        assertEquals(0.25f, progressPoints[1], 0.01f)
        assertEquals(0.5f, progressPoints[2], 0.01f)
        assertEquals(0.75f, progressPoints[3], 0.01f)
        assertEquals(1f, progressPoints[4], 0.01f)
    }

    @Test
    fun frameProgressSequenceForDelete() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 160L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        val progressPoints = mutableListOf<Float>()
        val checkpoints = listOf(0L, 40L, 80L, 120L, 160L)
        for (checkpoint in checkpoints) {
            timeSource.advanceTo(checkpoint * 1_000_000L)
            progressPoints.add(timeline.progress(timeSource.nowNanos() / 1_000_000))
        }
        assertEquals(0f, progressPoints[0], 0.01f)
        assertEquals(0.25f, progressPoints[1], 0.01f)
        assertEquals(0.5f, progressPoints[2], 0.01f)
        assertEquals(0.75f, progressPoints[3], 0.01f)
        assertEquals(1f, progressPoints[4], 0.01f)
    }

    @Test
    fun frameProgressSequenceForCompositionUpdate() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 160L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        val progressPoints = mutableListOf<Float>()
        val checkpoints = listOf(0L, 40L, 80L, 120L, 160L)
        for (checkpoint in checkpoints) {
            timeSource.advanceTo(checkpoint * 1_000_000L)
            progressPoints.add(timeline.progress(timeSource.nowNanos() / 1_000_000))
        }
        assertEquals(0f, progressPoints[0], 0.01f)
        assertEquals(0.25f, progressPoints[1], 0.01f)
        assertEquals(0.5f, progressPoints[2], 0.01f)
        assertEquals(0.75f, progressPoints[3], 0.01f)
        assertEquals(1f, progressPoints[4], 0.01f)
    }

    @Test
    fun frameProgressSequenceForRapidConsecutiveInput() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 160L
        val timeline1 = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline1.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(80)
        val midProgress = timeline1.progress(timeSource.nowNanos() / 1_000_000)
        assertTrue(midProgress in 0.49f..0.51f)
        timeline1.complete()
        val timeline2 = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline2.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(160)
        val finalProgress = timeline2.progress(timeSource.nowNanos() / 1_000_000)
        assertEquals(1f, finalProgress, 0.01f)
    }

    @Test
    fun frameProgressForCjkInput() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 200L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(50)
        assertEquals(0.25f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeSource.advanceByMs(50)
        assertEquals(0.5f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeSource.advanceByMs(100)
        assertEquals(1f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
    }

    @Test
    fun frameProgressForEmojiInput() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 200L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(100)
        assertEquals(0.5f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
    }

    @Test
    fun frameProgressForCompositionCommit() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 200L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(50)
        assertEquals(0.25f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeSource.advanceByMs(150)
        assertEquals(1f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
    }

    @Test
    fun frameProgressForCompositionCancel() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 160L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(40)
        assertEquals(0.25f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeline.cancel()
        assertEquals(TransactionState.Cancelled, timeline.getState())
    }

    @Test
    fun frameProgressForCrossLineDelete() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 250L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        val progressPoints = mutableListOf<Float>()
        for (ms in listOf(0L, 62L, 125L, 187L, 250L)) {
            timeSource.advanceTo(ms * 1_000_000L)
            progressPoints.add(timeline.progress(timeSource.nowNanos() / 1_000_000))
        }
        assertEquals(0f, progressPoints[0], 0.02f)
        assertEquals(0.25f, progressPoints[1], 0.02f)
        assertEquals(0.5f, progressPoints[2], 0.02f)
        assertEquals(0.75f, progressPoints[3], 0.02f)
        assertEquals(1f, progressPoints[4], 0.02f)
    }

    @Test
    fun frameProgressForMidlineInsert() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 200L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(50)
        assertEquals(0.25f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeSource.advanceByMs(150)
        assertEquals(1f, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
    }

    @Test
    fun pausedTransactionResumesFromCorrectProgress() {
        val timeSource = ManualAnimationTimeSource()
        val durationMs = 200L
        val timeline = AnimationTimeline(durationMs)
        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)
        timeSource.advanceByMs(60)
        val progressBeforePause = timeline.progress(timeSource.nowNanos() / 1_000_000)
        timeline.pause(timeSource.nowNanos() / 1_000_000)
        assertEquals(TransactionState.Paused, timeline.getState())
        timeSource.advanceByMs(40)
        assertEquals(progressBeforePause, timeline.progress(timeSource.nowNanos() / 1_000_000), 0.01f)
        timeline.resume(timeSource.nowNanos() / 1_000_000)
        assertEquals(TransactionState.Rendering, timeline.getState())
    }
}

class VisualFrameSnapshotDataTest {

    @Test
    fun visualFrameSnapshotContainsSliceVisualStates() {
        val snapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(
                SliceVisualState(
                    snapshotId = 1L,
                    role = SliceRole.Insert,
                    lineIndex = 0,
                    currentLeft = 0f,
                    currentTop = 0f,
                    currentRight = 100f,
                    currentBottom = 48f,
                    currentAlpha = 0.5f,
                    destinationLeft = 0f,
                    destinationTop = 0f,
                    destinationRight = 100f,
                    destinationBottom = 48f
                )
            ),
            cursorRect = android.graphics.RectF(50f, 0f, 51f, 48f),
            blockShiftStates = emptyList()
        )
        assertEquals(0.5f, snapshot.progress, 0.01f)
        assertEquals(TransactionState.Rendering, snapshot.state)
        assertEquals(1, snapshot.sliceVisualStates.size)
        assertEquals(SliceRole.Insert, snapshot.sliceVisualStates[0].role)
        assertEquals(0.5f, snapshot.sliceVisualStates[0].currentAlpha, 0.01f)
        assertNotNull(snapshot.cursorRect)
    }

    @Test
    fun visualFrameSnapshotWithBlockShifts() {
        val snapshot = VisualFrameSnapshot(
            progress = 0.25f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 4,
                    startUtf8 = 10,
                    endUtf8Exclusive = 30,
                    currentTranslateY = -36f,
                    targetTranslateY = 0f
                )
            )
        )
        assertEquals(1, snapshot.blockShiftStates.size)
        assertEquals(-36f, snapshot.blockShiftStates[0].currentTranslateY, 0.01f)
        assertEquals(0f, snapshot.blockShiftStates[0].targetTranslateY, 0.01f)
    }

    @Test
    fun sliceVisualStatePositionAndAlphaInRange() {
        val slice = SliceVisualState(
            snapshotId = 1L,
            role = SliceRole.Delete,
            lineIndex = 0,
            currentLeft = 10f,
            currentTop = 0f,
            currentRight = 50f,
            currentBottom = 48f,
            currentAlpha = 0.75f,
            destinationLeft = 10f,
            destinationTop = 0f,
            destinationRight = 50f,
            destinationBottom = 48f
        )
        assertTrue(slice.currentAlpha in 0f..1f)
        assertTrue(slice.currentRight >= slice.currentLeft)
        assertTrue(slice.currentBottom >= slice.currentTop)
    }
}

class FrameTimePropagationTest {

    @Test
    fun timelineUsesProvidedFrameTimeNotSystemTime() {
        val timeline = AnimationTimeline(200L)
        val specificFrameTimeMs = 1000L
        timeline.markFirstVisibleFrame(specificFrameTimeMs)
        assertEquals(specificFrameTimeMs, timeline.getFirstVisibleFrameTimeMs())
        val progressAtHalf = timeline.progress(specificFrameTimeMs + 100)
        assertEquals(0.5f, progressAtHalf, 0.01f)
    }

    @Test
    fun timelineProgressUsesFrameTimeNanosNotCurrentTime() {
        val timeline = AnimationTimeline(200L)
        val frameTime0Ms = 5000L
        timeline.markFirstVisibleFrame(frameTime0Ms)
        val frameTime1Ms = frameTime0Ms + 50
        assertEquals(0.25f, timeline.progress(frameTime1Ms), 0.01f)
        val frameTime2Ms = frameTime0Ms + 100
        assertEquals(0.5f, timeline.progress(frameTime2Ms), 0.01f)
        val frameTime3Ms = frameTime0Ms + 150
        assertEquals(0.75f, timeline.progress(frameTime3Ms), 0.01f)
        val frameTime4Ms = frameTime0Ms + 200
        assertEquals(1f, timeline.progress(frameTime4Ms), 0.01f)
    }

    @Test
    fun timelineIsCompletedUsesProvidedFrameTime() {
        val timeline = AnimationTimeline(200L)
        val startTimeMs = 3000L
        timeline.markFirstVisibleFrame(startTimeMs)
        assertFalse(timeline.isCompleted(startTimeMs + 199))
        assertTrue(timeline.isCompleted(startTimeMs + 200))
    }

    @Test
    fun frameTimeNanosOverridesSystemTimeInDrawFramePath() {
        val timeline = AnimationTimeline(200L)
        val frameTimeNanos = 10_000_000_000L
        val frameTimeMs = frameTimeNanos / 1_000_000
        timeline.markFirstVisibleFrame(frameTimeMs)
        assertEquals(0f, timeline.progress(frameTimeMs), 0.01f)
        assertEquals(0.5f, timeline.progress(frameTimeMs + 100), 0.01f)
        assertEquals(1f, timeline.progress(frameTimeMs + 200), 0.01f)
    }

    @Test
    fun enginePrepareAndSubmitAcceptsFrameTimeMsParameter() {
        val timeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            timeSource,
            transactionIdSource
        )
        engine.setAnimationPolicy(TextAnimationPolicy.SYSTEM_SUPPRESSED)
        timeSource.advanceTo(0L)
        val intent = com.xiwei.sujian.editor.v2.mirror.VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(0, 3)),
            animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
            durationMs = 0L,
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
            frameTimeMs = 500L
        )
        assertFalse(engine.hasActiveAnimation())
    }

    @Test
    fun visualRuntimePrepareAndSubmitAcceptsFrameTimeMsParameter() {
        val timeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime(timeSource, transactionIdSource)
        runtime.setAnimationPolicy(TextAnimationPolicy.SYSTEM_SUPPRESSED)
        val intent = com.xiwei.sujian.editor.v2.mirror.VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(0, 3)),
            animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
            durationMs = 0L,
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
            frameTimeMs = 800L
        )
        assertFalse(runtime.hasActiveAnimation())
    }

    @Test
    fun prepareAndSubmitFrameTimeMsNullFallsBackToTimeSource() {
        val timeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            timeSource,
            transactionIdSource
        )
        engine.setAnimationPolicy(TextAnimationPolicy.SYSTEM_SUPPRESSED)
        timeSource.advanceByMs(100)
        val intent = com.xiwei.sujian.editor.v2.mirror.VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(0, 3)),
            animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
            durationMs = 0L,
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
            frameTimeMs = null
        )
        assertFalse(engine.hasActiveAnimation())
    }

    @Test
    fun productionCallChain_frameTimeNanosFlowsThroughTick() {
        val timeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime(timeSource, transactionIdSource)
        timeSource.advanceByMs(0)
        val frameTimeNanos = 5_000_000_000L
        val frameTimeMs = frameTimeNanos / 1_000_000
        val layout = android.text.StaticLayout(
            "hello", android.text.TextPaint(), 100,
            android.text.Layout.Alignment.ALIGN_NORMAL, 1f, 0f, false
        )
        val frameState = runtime.tick(
            frameTimeMs,
            layout,
            null,
            emptyList(),
            100, 100,
            0f, 0f,
            true, true,
            5, 0, 0
        )
        assertNotNull(frameState)
    }
}
