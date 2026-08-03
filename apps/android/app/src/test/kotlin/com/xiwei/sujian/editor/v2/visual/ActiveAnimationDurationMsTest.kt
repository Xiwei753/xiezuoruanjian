package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class ActiveAnimationDurationMsTest {

    private fun createEngine(timeSource: AnimationTimeSource = ManualAnimationTimeSource()): AndroidTextAnimationEngine {
        val planner = AndroidVisualPlanner()
        val resourceStore = VisualResourceStore()
        return AndroidTextAnimationEngine(planner, resourceStore, timeSource, TransactionIdSource())
    }

    @Test
    fun noActiveTransaction_returnsNull() {
        val engine = createEngine()
        assertNull(engine.getActiveTransaction())
    }

    @Test
    fun afterSubmit_activeTransactionHasCorrectDurationMs() {
        val timeSource = ManualAnimationTimeSource()
        val engine = createEngine(timeSource)
        val durationMs = 250L
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = emptyList(),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = durationMs,
            blockShifts = emptyList(),
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT
        )
        engine.submit(transaction)
        val active = engine.getActiveTransaction()
        assertNotNull("Active transaction should exist after submit", active)
        assertEquals(durationMs, active!!.durationMs)
    }

    @Test
    fun afterComplete_terminalStateRetainedWithNoResources() {
        val timeSource = ManualAnimationTimeSource()
        val engine = createEngine(timeSource)
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = emptyList(),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 100L,
            blockShifts = emptyList(),
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT
        )
        engine.submit(transaction)
        timeSource.advanceByMs(16)
        engine.markFirstVisibleFrame(16)
        timeSource.advanceByMs(200)
        val completed = engine.completeIfFinished(216)
        assertTrue("completeIfFinished must report completion", completed)

        // Terminal state stays queryable (Completed, progress 1.0, no owned resources) so
        // tests and diagnostics can verify the animation actually reached its end.
        val snapshot = engine.captureStateSnapshot(216)
        assertNotNull("Completed terminal state must remain queryable", snapshot)
        assertEquals(TransactionState.Completed, snapshot!!.transactionState)
        assertTrue("Completed progress must be 1.0", snapshot.progress >= 1f)
        assertEquals("Completed transaction must own no resources", 0, snapshot.ownedResourceCount)
        assertFalse("hasActiveAnimation must be false after completion", engine.hasActiveAnimation())
    }

    @Test
    fun afterCancel_activeTransactionIsNull() {
        val timeSource = ManualAnimationTimeSource()
        val engine = createEngine(timeSource)
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = emptyList(),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 100L,
            blockShifts = emptyList(),
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT
        )
        engine.submit(transaction)
        engine.cancel()
        assertNull("Active transaction should be null after cancel", engine.getActiveTransaction())
    }
}
