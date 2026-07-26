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
    fun afterComplete_activeTransactionIsNull() {
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
        engine.completeIfFinished(216)
        assertNull("Active transaction should be null after completion", engine.getActiveTransaction())
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
