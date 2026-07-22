package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionCommandPortContractTest {

    @Test
    fun targetSnapshotHoldsAllFields() {
        val snap = TargetSnapshot(
            text = "hello",
            cursorUtf8 = 5,
            revision = 1L,
            selectionAnchorUtf8 = 3,
            selectionHeadUtf8 = 5
        )
        assertEquals("hello", snap.text)
        assertEquals(5, snap.cursorUtf8)
        assertEquals(1L, snap.revision)
        assertEquals(3, snap.selectionAnchorUtf8)
        assertEquals(5, snap.selectionHeadUtf8)
    }

    @Test
    fun targetDecorationsDefaultsEmpty() {
        val deco = TargetDecorations()
        assertTrue(deco.searchHighlightsUtf8.isEmpty())
        assertEquals(-1, deco.selectionStartUtf8)
        assertEquals(-1, deco.selectionEndUtf8)
    }

    @Test
    fun targetDecorationsWithHighlightsAndSelection() {
        val deco = TargetDecorations(
            searchHighlightsUtf8 = listOf(Pair(0, 3), Pair(5, 8)),
            selectionStartUtf8 = 0,
            selectionEndUtf8 = 3
        )
        assertEquals(2, deco.searchHighlightsUtf8.size)
        assertEquals(0, deco.selectionStartUtf8)
        assertEquals(3, deco.selectionEndUtf8)
    }

    @Test
    fun targetCommandReplaceHoldsFields() {
        val cmd = TargetCommand.Replace(0, 5, "world", "hello")
        assertEquals(0, cmd.byteStart)
        assertEquals(5, cmd.byteEndExclusive)
        assertEquals("world", cmd.replacementText)
        assertEquals("hello", cmd.originalText)
    }

    @Test
    fun targetCommandReplaceAllHoldsFields() {
        val cmd = TargetCommand.ReplaceAll("old", "new")
        assertEquals("old", cmd.searchText)
        assertEquals("new", cmd.replacementText)
    }

    @Test
    fun targetCommandSetSelectionHoldsFields() {
        val cmd = TargetCommand.SetSelection(3, 7)
        assertEquals(3, cmd.anchorUtf8)
        assertEquals(7, cmd.headUtf8)
    }

    @Test
    fun targetCommandResultSuccessHoldsSnapshot() {
        val snap = TargetSnapshot("text", 4, 2L, 0, 4)
        val result = TargetCommandResult.Success(snap)
        assertEquals("text", result.snapshot.text)
    }

    @Test
    fun targetCommandResultFailedHoldsReason() {
        val result = TargetCommandResult.Failed("no session")
        assertEquals("no session", result.reason)
    }

    @Test
    fun editingStateTransitions() {
        val states = EditingState.values()
        assertTrue(states.contains(EditingState.IDLE))
        assertTrue(states.contains(EditingState.BINDING))
        assertTrue(states.contains(EditingState.EDITING))
        assertTrue(states.contains(EditingState.COMMITTING))
        assertTrue(states.contains(EditingState.CANCELLING))
        assertTrue(states.contains(EditingState.REBINDING))
        assertTrue(states.contains(EditingState.RELEASED))
    }

    @Test
    fun secretPolicyValues() {
        assertEquals(2, SecretPolicy.values().size)
        assertTrue(SecretPolicy.values().contains(SecretPolicy.NONE))
        assertTrue(SecretPolicy.values().contains(SecretPolicy.MASK_AND_CLEAR_ON_COMMIT))
    }

    @Test
    fun sessionResetSourceValues() {
        assertEquals(3, SessionResetSource.values().size)
    }
}
