package com.xiwei.sujian.editor.v2.projection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SecretProjectionAtomicityTest {

    @Test
    fun maskedProjectionDisplayMatchesRealLength() {
        val text = "password123"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(text.length, proj.displayText.length)
        assertEquals(text.length, proj.displayLengthUtf16)
    }

    @Test
    fun maskedProjectionUtf8ToDisplayUtf16AtBoundaries() {
        val text = "abc"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(3))
    }

    @Test
    fun maskedProjectionDisplayUtf16ToRealUtf8AtBoundaries() {
        val text = "abc"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(3, proj.displayUtf16ToRealUtf8(3))
    }

    @Test
    fun identityProjectionPreservesText() {
        val text = "Hello 世界"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(text, proj.realText)
        assertEquals(text, proj.displayText)
        assertFalse(proj.isMasked)
    }

    @Test
    fun maskedProjectionWithMultibyteUtf8() {
        val text = "你好世界"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(text.length, proj.displayLengthUtf16)
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        assertEquals(utf8Len, proj.realLengthUtf8)
        for (i in 0..text.length) {
            val realUtf8 = text.substring(0, i).toByteArray(Charsets.UTF_8).size
            assertEquals(i, proj.realUtf8ToDisplayUtf16(realUtf8))
        }
    }

    @Test
    fun emptyTextMaskedProjectionIsIdentity() {
        val proj = DisplayTextProjection.masked("")
        assertEquals("", proj.realText)
        assertEquals("", proj.displayText)
        assertEquals(0, proj.realLengthUtf8)
        assertEquals(0, proj.displayLengthUtf16)
    }

    @Test
    fun projectionOffsetMappingConsistentForAscii() {
        val text = "abcdef"
        val proj = DisplayTextProjection.masked(text)
        for (utf8 in 0..6) {
            val displayUtf16 = proj.realUtf8ToDisplayUtf16(utf8)
            val roundTrip = proj.displayUtf16ToRealUtf8(displayUtf16)
            assertEquals(utf8, roundTrip)
        }
    }

    @Test
    fun customMaskCharUsed() {
        val proj = DisplayTextProjection.masked("ab", "*")
        assertEquals("**", proj.displayText)
    }
}
