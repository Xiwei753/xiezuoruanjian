package com.xiwei.sujian.editor.v2.projection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DisplayTextProjectionTest {

    @Test
    fun identityProjectionRealEqualsDisplay() {
        val text = "Hello 世界"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(text, proj.realText)
        assertEquals(text, proj.displayText)
        assertFalse(proj.isMasked)
    }

    @Test
    fun identityProjectionLengths() {
        val text = "Hello 世界"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(text.toByteArray(Charsets.UTF_8).size, proj.realLengthUtf8)
        assertEquals(text.length, proj.displayLengthUtf16)
    }

    @Test
    fun maskedProjectionDisplayIsMasked() {
        val text = "abc"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(text, proj.realText)
        assertEquals("\u2022\u2022\u2022", proj.displayText)
        assertTrue(proj.isMasked)
    }

    @Test
    fun maskedProjectionWithMultibyteChars() {
        val text = "你好"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(2, proj.displayLengthUtf16)
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        assertEquals(utf8Len, proj.realLengthUtf8)
    }

    @Test
    fun maskedProjectionWithCustomMaskChar() {
        val text = "ab"
        val proj = DisplayTextProjection.masked(text, "*")
        assertEquals("**", proj.displayText)
    }

    @Test
    fun emptyTextIdentityProjection() {
        val proj = DisplayTextProjection.identity("")
        assertEquals("", proj.realText)
        assertEquals("", proj.displayText)
        assertEquals(0, proj.realLengthUtf8)
        assertEquals(0, proj.displayLengthUtf16)
    }

    @Test
    fun emptyTextMaskedProjection() {
        val proj = DisplayTextProjection.masked("")
        assertEquals("", proj.realText)
        assertEquals("", proj.displayText)
        assertEquals(0, proj.realLengthUtf8)
        assertEquals(0, proj.displayLengthUtf16)
    }

    @Test
    fun maskedProjectionUtf8ToUtf16AtBoundary() {
        val text = "Hello"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(5, proj.realUtf8ToDisplayUtf16(5))
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
    }

    @Test
    fun maskedProjectionDisplayUtf16ToRealUtf8() {
        val text = "Hello"
        val proj = DisplayTextProjection.masked(text)
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        assertEquals(utf8Len, proj.displayUtf16ToRealUtf8(5))
        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
    }

    @Test
    fun maskedWithCompositionShowsCompTextUnmasked() {
        val text = "password"
        val proj = DisplayTextProjection.maskedWithComposition(text, 4, 8, "word")
        assertEquals("\u2022\u2022\u2022\u2022word", proj.displayText)
        assertTrue(proj.isMasked)
    }

    @Test
    fun maskedWithCompositionAtStart() {
        val text = "hello"
        val proj = DisplayTextProjection.maskedWithComposition(text, 0, 3, "hel")
        assertEquals("hel\u2022\u2022", proj.displayText)
    }

    @Test
    fun maskedWithCompositionFullCoverage() {
        val text = "abc"
        val proj = DisplayTextProjection.maskedWithComposition(text, 0, 3, "abc")
        assertEquals("abc", proj.displayText)
    }

    @Test
    fun maskedWithCompositionEmptyCompRangeFallsBackToMasked() {
        val text = "abc"
        val proj = DisplayTextProjection.maskedWithComposition(text, -1, -1, "")
        assertEquals("\u2022\u2022\u2022", proj.displayText)
    }

    @Test
    fun maskedWithCompositionDisplayLength() {
        val text = "abcdef"
        val proj = DisplayTextProjection.maskedWithComposition(text, 3, 6, "def")
        assertEquals(6, proj.displayLengthUtf16)
        assertEquals(text.toByteArray(Charsets.UTF_8).size, proj.realLengthUtf8)
    }

    @Test
    fun maskedWithCompositionCustomMaskChar() {
        val text = "ab"
        val proj = DisplayTextProjection.maskedWithComposition(text, 1, 2, "b", "*")
        assertEquals("*b", proj.displayText)
    }
}
