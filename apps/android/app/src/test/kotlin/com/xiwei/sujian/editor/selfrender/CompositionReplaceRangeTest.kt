package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class CompositionReplaceRangeTest {

    private lateinit var buffer: SujianEditorBuffer

    @Before
    fun setUp() {
        buffer = SujianEditorBuffer()
    }

    @Test
    fun zeroLengthReplaceRange_preeditInsertedCorrectly() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2)
        buffer.setComposingText("abc", 1)

        assertEquals("你好世界", buffer.text)
        assertTrue(buffer.hasActiveCompositionSession)
        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)

        val virtualText = buffer.compositionSession.buildVirtualText()
        assertEquals("你好abc世界", virtualText)
    }

    @Test
    fun preeditUpdateDoesNotChangeReplaceRange() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2)
        buffer.setComposingText("a", 1)

        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)

        buffer.setComposingText("abcdef", 1)

        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)
        assertEquals("你好世界", buffer.text)

        val virtualText = buffer.compositionSession.buildVirtualText()
        assertEquals("你好abcdef世界", virtualText)
    }

    @Test
    fun setComposingRegion_updatesReplaceRange() {
        buffer.loadText("你好世界")
        buffer.setComposingRegion(1, 3)

        assertTrue(buffer.hasActiveCompositionSession)
        assertEquals(1, buffer.compositionReplaceStart)
        assertEquals(3, buffer.compositionReplaceEndExclusive)

        val virtualText = buffer.compositionSession.buildVirtualText()
        assertEquals("你" + "好世" + "界", virtualText)
    }

    @Test
    fun setComposingRegionThenUpdate_preeditReplacesRegion() {
        buffer.loadText("你好世界")
        buffer.setComposingRegion(1, 3)
        buffer.setComposingText("abc", 1)

        assertEquals(1, buffer.compositionReplaceStart)
        assertEquals(3, buffer.compositionReplaceEndExclusive)

        val virtualText = buffer.compositionSession.buildVirtualText()
        assertEquals("你abc界", virtualText)
    }

    @Test
    fun preeditLengthDiffersFromReplaceRangeLength() {
        buffer.loadText("你好世界")
        buffer.setComposingRegion(1, 3)
        buffer.setComposingText("abcdefghij", 1)

        val replaceRangeLen = buffer.compositionReplaceEndExclusive - buffer.compositionReplaceStart
        val preeditLen = buffer.composingText.length

        assertNotEquals(replaceRangeLen, preeditLen)
        assertEquals(2, replaceRangeLen)
        assertEquals(10, preeditLen)

        val virtualText = buffer.compositionSession.buildVirtualText()
        assertEquals("你abcdefghij界", virtualText)
    }

    @Test
    fun emoji_replaceRangeBoundaryCorrect() {
        buffer.loadText("A😀B")
        buffer.setSelection(1, 1)
        buffer.setComposingText("xyz", 1)

        assertEquals(1, buffer.compositionReplaceStart)
        assertEquals(1, buffer.compositionReplaceEndExclusive)
        assertEquals("A😀B", buffer.text)

        val virtualText = buffer.compositionSession.buildVirtualText()
        assertEquals("Axyz😀B", virtualText)
    }

    @Test
    fun commitTextWithZeroReplaceRange_insertsAtCursor() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2)
        buffer.setComposingText("abc", 1)

        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)

        buffer.replaceRange(
            buffer.compositionReplaceStart,
            buffer.compositionReplaceEndExclusive,
            "你好",
            SujianEditCause.TypingCommit
        )

        assertEquals("你好你好世界", buffer.text)
    }

    @Test
    fun commitTextWithNonZeroReplaceRange_replacesCommittedText() {
        buffer.loadText("你好世界")
        buffer.setComposingRegion(1, 3)
        buffer.setComposingText("abc", 1)

        assertEquals(1, buffer.compositionReplaceStart)
        assertEquals(3, buffer.compositionReplaceEndExclusive)

        buffer.replaceRange(
            buffer.compositionReplaceStart,
            buffer.compositionReplaceEndExclusive,
            "ABC",
            SujianEditCause.TypingCommit
        )

        assertEquals("你ABC界", buffer.text)
    }

    @Test
    fun finishComposing_clearsSession() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2)
        buffer.setComposingText("abc", 1)

        assertTrue(buffer.hasActiveCompositionSession)

        buffer.finishComposing()

        assertFalse(buffer.hasActiveCompositionSession)
        assertEquals("你好世界", buffer.text)
    }

    @Test
    fun composingStartEnd_reflectPreeditRangeInVirtualText() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2)
        buffer.setComposingText("abc", 1)

        assertEquals(2, buffer.composingStart)
        assertEquals(5, buffer.composingEnd)
        assertEquals("abc", buffer.composingText)
    }

    @Test
    fun composingStartEnd_withNonZeroReplaceRange() {
        buffer.loadText("你好世界")
        buffer.setComposingRegion(1, 3)
        buffer.setComposingText("abc", 1)

        assertEquals(1, buffer.composingStart)
        assertEquals(4, buffer.composingEnd)
    }

    @Test
    fun sessionPreeditRangeInVirtualText() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2)
        buffer.setComposingText("abc", 1)

        val range = buffer.compositionSession.preeditRangeInVirtualText()
        assertEquals(2, range.first)
        assertEquals(5, range.last)
    }

    @Test
    fun sessionPreeditRangeInVirtualText_withNonZeroReplaceRange() {
        buffer.loadText("你好世界")
        buffer.setComposingRegion(1, 3)
        buffer.setComposingText("abc", 1)

        val range = buffer.compositionSession.preeditRangeInVirtualText()
        assertEquals(1, range.first)
        assertEquals(4, range.last)
    }

    @Test
    fun multipleSetComposingText_maintainsOriginalReplaceRange() {
        buffer.loadText("你好世界")
        buffer.setSelection(2, 2)

        buffer.setComposingText("n", 1)
        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)
        assertEquals("你好n世界", buffer.compositionSession.buildVirtualText())

        buffer.setComposingText("ni", 1)
        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)
        assertEquals("你好ni世界", buffer.compositionSession.buildVirtualText())

        buffer.setComposingText("nih", 1)
        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)
        assertEquals("你好nih世界", buffer.compositionSession.buildVirtualText())

        buffer.setComposingText("你好", 1)
        assertEquals(2, buffer.compositionReplaceStart)
        assertEquals(2, buffer.compositionReplaceEndExclusive)
        assertEquals("你好你好世界", buffer.compositionSession.buildVirtualText())
    }
}
