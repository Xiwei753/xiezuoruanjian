package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class TransactionStateValidationTest {

    private val activeStates = setOf(
        TransactionState.Rendering,
        TransactionState.Prepared,
        TransactionState.Pending
    )

    @Test
    fun renderingIsActiveState() {
        assertTrue(TransactionState.Rendering in activeStates)
    }

    @Test
    fun preparedIsActiveState() {
        assertTrue(TransactionState.Prepared in activeStates)
    }

    @Test
    fun pendingIsActiveState() {
        assertTrue(TransactionState.Pending in activeStates)
    }

    @Test
    fun completedIsNotActiveState() {
        assertFalse(TransactionState.Completed in activeStates)
    }

    @Test
    fun cancelledIsNotActiveState() {
        assertFalse(TransactionState.Cancelled in activeStates)
    }

    @Test
    fun pausedIsNotActiveState() {
        assertFalse(TransactionState.Paused in activeStates)
    }

    @Test
    fun animationStateSnapshotWithRenderingStateIsValid() {
        val snapshot = AnimationStateSnapshot(
            transactionId = 1L,
            operationKind = "insert",
            animationMode = "slice",
            oldAffectedRanges = emptyList(),
            newAffectedRanges = emptyList(),
            progress = 0.25f,
            sliceRoles = listOf(SliceRole.Insert),
            cursorTransition = null,
            ownedResourceCount = 1,
            transactionState = TransactionState.Rendering
        )
        assertTrue(snapshot.transactionState in activeStates)
        assertTrue(snapshot.transactionState == TransactionState.Rendering)
    }

    @Test
    fun animationStateSnapshotWithCompletedStateIsNotActive() {
        val snapshot = AnimationStateSnapshot(
            transactionId = 1L,
            operationKind = "insert",
            animationMode = "slice",
            oldAffectedRanges = emptyList(),
            newAffectedRanges = emptyList(),
            progress = 1f,
            sliceRoles = listOf(SliceRole.Insert),
            cursorTransition = null,
            ownedResourceCount = 0,
            transactionState = TransactionState.Completed
        )
        assertFalse(snapshot.transactionState in activeStates)
    }
}
