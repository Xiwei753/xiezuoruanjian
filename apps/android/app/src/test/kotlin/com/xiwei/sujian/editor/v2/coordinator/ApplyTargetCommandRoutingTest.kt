package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ApplyTargetCommandRoutingTest {

    @Test
    fun sessionCommandPortHasApplyTargetCommand() {
        val iface = Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
        val method = iface.getDeclaredMethod("applyTargetCommand", String::class.java, Class.forName("com.xiwei.sujian.editor.v2.coordinator.TargetCommand"))
        assertNotNull(method)
    }

    @Test
    fun sessionCommandPortHasSetTargetDecorations() {
        val iface = Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
        val method = iface.getDeclaredMethod("setTargetDecorations", String::class.java, Class.forName("com.xiwei.sujian.editor.v2.coordinator.TargetDecorations"))
        assertNotNull(method)
    }

    @Test
    fun sessionCommandPortHasQueryTargetSnapshot() {
        val iface = Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
        val method = iface.getDeclaredMethod("queryTargetSnapshot", String::class.java)
        assertNotNull(method)
    }

    @Test
    fun targetCommandReplaceHoldsByteRange() {
        val cmd = TargetCommand.Replace(0, 5, "world", "hello")
        assertEquals(0, cmd.byteStart)
        assertEquals(5, cmd.byteEndExclusive)
        assertEquals("world", cmd.replacementText)
        assertEquals("hello", cmd.originalText)
    }

    @Test
    fun targetCommandReplaceAllHoldsSearchAndReplace() {
        val cmd = TargetCommand.ReplaceAll("old", "new")
        assertEquals("old", cmd.searchText)
        assertEquals("new", cmd.replacementText)
    }

    @Test
    fun targetCommandSetSelectionHoldsAnchorAndHead() {
        val cmd = TargetCommand.SetSelection(3, 7)
        assertEquals(3, cmd.anchorUtf8)
        assertEquals(7, cmd.headUtf8)
    }

    @Test
    fun targetDecorationsSupportsSearchHighlightsAndSelection() {
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
    fun targetDecorationsDefaultsToEmpty() {
        val deco = TargetDecorations()
        assertTrue(deco.searchHighlightsUtf8.isEmpty())
        assertEquals(-1, deco.selectionStartUtf8)
        assertEquals(-1, deco.selectionEndUtf8)
    }

    @Test
    fun targetSnapshotHoldsAllFields() {
        val snap = TargetSnapshot("text", 4, 2L, 0, 4)
        assertEquals("text", snap.text)
        assertEquals(4, snap.cursorUtf8)
        assertEquals(2L, snap.revision)
        assertEquals(0, snap.selectionAnchorUtf8)
        assertEquals(4, snap.selectionHeadUtf8)
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

    private fun assertNotNull(obj: Any?) {
        assertTrue("Expected non-null value", obj != null)
    }
}
