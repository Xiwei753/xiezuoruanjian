package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.mirror.DisplayPatch
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import org.junit.Assert.*
import org.junit.Test

class DisplayTextMirrorTest {

    @Test
    fun loadText_setsInitialContent() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        assertEquals("Hello", mirror.getText())
        assertEquals(5, mirror.getCursorUtf8())
        assertEquals(0, mirror.getRevision())
    }

    @Test
    fun applyPatches_insertsText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        val patches = listOf(DisplayPatch(
            baseRevision = 0,
            newRevision = 1,
            replaceByteStart = 2,
            replaceByteEndExclusive = 2,
            insertedText = "c",
            resultingSelectionStart = 3,
            resultingSelectionEnd = 3
        ))

        mirror.applyPatches(patches)
        assertEquals("abc", mirror.getText())
        assertEquals(3, mirror.getCursorUtf8())
        assertEquals(1, mirror.getRevision())
    }

    @Test
    fun applyPatches_deletesText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("abc", 3)

        val patches = listOf(DisplayPatch(
            baseRevision = 0,
            newRevision = 1,
            replaceByteStart = 2,
            replaceByteEndExclusive = 3,
            insertedText = "",
            resultingSelectionStart = 2,
            resultingSelectionEnd = 2
        ))

        mirror.applyPatches(patches)
        assertEquals("ab", mirror.getText())
        assertEquals(2, mirror.getCursorUtf8())
    }

    @Test
    fun applyPatches_replacesText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("abc", 3)

        val patches = listOf(DisplayPatch(
            baseRevision = 0,
            newRevision = 1,
            replaceByteStart = 1,
            replaceByteEndExclusive = 2,
            insertedText = "X",
            resultingSelectionStart = 2,
            resultingSelectionEnd = 2
        ))

        mirror.applyPatches(patches)
        assertEquals("aXc", mirror.getText())
    }

    @Test
    fun applyPatches_skipsStaleRevisions() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        val patches = listOf(
            DisplayPatch(0, 1, 2, 2, "c", 3, 3),
            DisplayPatch(0, 1, 2, 2, "d", 3, 3)
        )

        mirror.applyPatches(patches)
        assertEquals("abc", mirror.getText())
        assertEquals(1, mirror.getRevision())
    }

    @Test
    fun applyPatches_handlesChineseText() {
        val mirror = DisplayTextMirror()
        mirror.loadText("你好", 6)

        val patches = listOf(DisplayPatch(
            baseRevision = 0,
            newRevision = 1,
            replaceByteStart = 6,
            replaceByteEndExclusive = 6,
            insertedText = "世",
            resultingSelectionStart = 9,
            resultingSelectionEnd = 9
        ))

        mirror.applyPatches(patches)
        assertEquals("你好世", mirror.getText())
        assertEquals(9, mirror.getCursorUtf8())
    }

    @Test
    fun updateComposition_setsUnderline() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        mirror.updateComposition(2, 2, "c")
        val range = mirror.getCompositionRangeUtf16()
        assertNotNull(range)
        assertEquals("abc", mirror.getText())
    }

    @Test
    fun clearComposition_removesComposition() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        mirror.updateComposition(2, 2, "c")
        mirror.clearComposition()
        assertNull(mirror.getCompositionRangeUtf16())
    }

    @Test
    fun applyPatches_clearsCompositionBeforeApplying() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)

        mirror.updateComposition(2, 2, "c")

        val patches = listOf(DisplayPatch(
            baseRevision = 0,
            newRevision = 1,
            replaceByteStart = 2,
            replaceByteEndExclusive = 2,
            insertedText = "d",
            resultingSelectionStart = 3,
            resultingSelectionEnd = 3
        ))

        mirror.applyPatches(patches)
        assertNull(mirror.getCompositionRangeUtf16())
    }
}
