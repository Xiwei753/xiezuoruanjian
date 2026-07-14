package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class CompositionOwnershipTest {

    private lateinit var manager: AndroidCompositionManager

    @Before
    fun setUp() {
        manager = AndroidCompositionManager()
    }

    private fun makeRevision(id: Long): AndroidCompositionVisualRevision {
        return AndroidCompositionVisualRevision(
            committedText = "test",
            compositionReplaceRange = 0..0,
            preeditRangeInVirtualText = 0..4,
            preeditText = "test",
            virtualText = "test",
            affectedParagraphRange = 0..0,
            lineSnapshots = emptyList(),
            cursorRect = android.graphics.RectF(),
            decorationRanges = emptyList(),
            revisionId = id,
            sessionId = CompositionSessionId(id)
        )
    }

    @Test
    fun setCurrent_previousReleased() {
        val rev1 = makeRevision(1)
        val rev2 = makeRevision(2)

        manager.setCurrent(rev1)
        manager.setCurrent(rev2)

        assertNull(manager.getPrevious())
    }

    @Test
    fun takeCurrentForTransaction_transfersOwnership() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction()
        assertNotNull(taken)
        assertEquals(1L, taken!!.revisionId)

        assertNull(manager.getCurrent())
    }

    @Test(expected = IllegalStateException::class)
    fun takeCurrentForTransaction_doubleTake_throws() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        manager.takeCurrentForTransaction()
        manager.takeCurrentForTransaction()
    }

    @Test
    fun takePreviousForTransaction_transfersOwnership() {
        val rev1 = makeRevision(1)
        val rev2 = makeRevision(2)

        manager.setCurrent(rev1)
        manager.setCurrent(rev2)

        val taken = manager.takePreviousForTransaction()
        assertNotNull(taken)
        assertEquals(1L, taken!!.revisionId)
    }

    @Test
    fun clear_releasesSessionOwnedRevisions() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        manager.clear()

        assertNull(manager.getCurrent())
        assertNull(manager.getPrevious())
    }

    @Test
    fun clear_doesNotReleaseTransferredRevisions() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        manager.takeCurrentForTransaction()
        manager.clear()

        assertNull(manager.getCurrent())
    }

    @Test
    fun ownershipChain_sessionToTransactionToRelease() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction()
        assertNotNull(taken)

        manager.clear()

        assertNull(manager.getCurrent())
    }

    @Test
    fun consecutiveUpdates_noLeak() {
        for (i in 1..100) {
            val rev = makeRevision(i.toLong())
            manager.setCurrent(rev)
        }
        manager.clear()
        assertNull(manager.getCurrent())
        assertNull(manager.getPrevious())
    }

    @Test
    fun buildVirtualText_zeroLengthReplaceRange() {
        val result = manager.buildVirtualText("你好世界", 2..2, "abc")
        assertEquals("你好abc世界", result)
    }

    @Test
    fun buildVirtualText_nonZeroReplaceRange() {
        val result = manager.buildVirtualText("你好世界", 1..3, "abc")
        assertEquals("你abc界", result)
    }

    @Test
    fun buildVirtualText_emptyPreedit() {
        val result = manager.buildVirtualText("你好世界", 2..2, "")
        assertEquals("你好世界", result)
    }

    @Test
    fun buildVirtualText_preeditLongerThanReplaceRange() {
        val result = manager.buildVirtualText("你好世界", 2..2, "abcdefghij")
        assertEquals("你好abcdefghij世界", result)
    }

    @Test
    fun buildVirtualText_preeditShorterThanReplaceRange() {
        val result = manager.buildVirtualText("你好世界", 1..3, "X")
        assertEquals("你X界", result)
    }
}

class CompositionSessionTest {

    @Test
    fun createNew_zeroLengthReplaceRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        assertTrue(session.isActive)
        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)
        assertEquals(0, session.replaceRangeLength())
        assertEquals("你好abc世界", session.buildVirtualText())
    }

    @Test
    fun createNew_nonZeroReplaceRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 1,
            replaceEndExclusive = 3,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        assertEquals(1, session.replaceStart)
        assertEquals(3, session.replaceEndExclusive)
        assertEquals(2, session.replaceRangeLength())
        assertEquals("你abc界", session.buildVirtualText())
    }

    @Test
    fun preeditRangeInVirtualText_zeroLengthReplace() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val range = session.preeditRangeInVirtualText()
        assertEquals(2, range.first)
        assertEquals(5, range.last)
    }

    @Test
    fun preeditRangeInVirtualText_nonZeroReplace() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 1,
            replaceEndExclusive = 3,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val range = session.preeditRangeInVirtualText()
        assertEquals(1, range.first)
        assertEquals(4, range.last)
    }

    @Test
    fun updatePreedit_doesNotChangeReplaceRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "n",
            preeditCursorOffset = 1
        )

        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)

        session.preeditText = "ni"
        session.preeditCursorOffset = 2

        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)
        assertEquals("你好ni世界", session.buildVirtualText())

        session.preeditText = "nihao"
        session.preeditCursorOffset = 5

        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)
        assertEquals("你好nihao世界", session.buildVirtualText())
    }

    @Test
    fun emptySession_isNotActive() {
        assertFalse(CompositionSession.EMPTY.isActive)
    }

    @Test
    fun replaceRange_returnsCorrectIntRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 1,
            replaceEndExclusive = 3,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        assertEquals(1..3, session.replaceRange())
    }
}
