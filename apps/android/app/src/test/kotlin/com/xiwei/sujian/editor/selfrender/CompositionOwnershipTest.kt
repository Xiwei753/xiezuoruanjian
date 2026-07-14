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

        assertTrue(rev1.isReleased())
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
    fun clear_releasesSessionOwnedRevisions() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        manager.clear()

        assertNull(manager.getCurrent())
        assertTrue(rev1.isReleased())
    }

    @Test
    fun clear_doesNotReleaseTransferredRevisions() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction()
        assertNotNull(taken)
        assertFalse(taken!!.isReleased())

        manager.clear()

        assertNull(manager.getCurrent())
        assertFalse(taken.isReleased())
    }

    @Test
    fun ownershipChain_sessionToTransactionToRelease() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction()
        assertNotNull(taken)

        manager.clear()

        assertNull(manager.getCurrent())
        assertFalse(taken!!.isReleased())

        taken.release()
        assertTrue(taken.isReleased())
    }

    @Test(expected = IllegalStateException::class)
    fun doubleRelease_throws() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        manager.clear()
    }

    @Test
    fun consecutiveUpdates_noLeak() {
        for (i in 1..100) {
            val rev = makeRevision(i.toLong())
            manager.setCurrent(rev)
        }
        val lastRev = manager.getCurrent()
        assertNotNull(lastRev)
        assertFalse(lastRev!!.isReleased())

        manager.clear()
        assertTrue(lastRev.isReleased())
        assertNull(manager.getCurrent())
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

    @Test
    fun revisionRelease_isIdempotentWithinRevision() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        manager.clear()
        assertTrue(rev1.isReleased())
    }

    @Test
    fun takeAndRelease_revisionCannotBeDoubleReleased() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        val taken = manager.takeCurrentForTransaction()
        assertNotNull(taken)
        taken!!.release()
        assertTrue(taken.isReleased())
    }

    @Test
    fun transactionHoldsRevisionOwnership() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        val prevRevision = manager.takeCurrentForTransaction()
        assertNotNull(prevRevision)
        assertFalse(prevRevision!!.isReleased())

        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = com.xiwei.sujian.model.AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 0,
            newRevision = 1,
            slices = mutableListOf(),
            oldLineSnapshots = mutableListOf(),
            newLineSnapshots = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRevision
        )

        assertFalse(prevRevision.isReleased())
        tx.cancel("test")
        assertTrue(prevRevision.isReleased())
    }

    @Test
    fun transactionComplete_releasesOwnedRevisions() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        val prevRevision = manager.takeCurrentForTransaction()

        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = com.xiwei.sujian.model.AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 0,
            newRevision = 1,
            slices = mutableListOf(),
            oldLineSnapshots = mutableListOf(),
            newLineSnapshots = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRevision
        )

        tx.markPrepared()
        tx.markRendering()
        assertFalse(prevRevision!!.isReleased())
        tx.complete()
        assertTrue(prevRevision.isReleased())
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

        val updated = session.updatePreedit("ni", 2)

        assertEquals(2, updated.replaceStart)
        assertEquals(2, updated.replaceEndExclusive)
        assertEquals("你好ni世界", updated.buildVirtualText())

        val updated2 = updated.updatePreedit("nihao", 5)

        assertEquals(2, updated2.replaceStart)
        assertEquals(2, updated2.replaceEndExclusive)
        assertEquals("你好nihao世界", updated2.buildVirtualText())
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

    @Test
    fun consecutiveUpdates_100times_noLeak() {
        var session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "n",
            preeditCursorOffset = 1
        )

        for (i in 1..100) {
            session = session.updatePreedit("n$i", 2)
        }

        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)
        assertTrue(session.isActive)
    }

    @Test
    fun commit_returnsCorrectCommittedText() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val (cleared, committedText) = session.commit("abc")
        assertFalse(cleared.isActive)
        assertEquals("你好abc世界", committedText)
    }

    @Test
    fun cancel_returnsInactiveSession() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val cancelled = session.cancel()
        assertFalse(cancelled.isActive)
    }
}
