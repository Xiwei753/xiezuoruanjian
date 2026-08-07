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
    fun identityProjectionChineseUtf8ToUtf16() {
        val text = "中a"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(3))
        assertEquals(2, proj.realUtf8ToDisplayUtf16(4))
    }

    @Test
    fun identityProjectionChineseUtf16ToUtf8() {
        val text = "中a"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(3, proj.displayUtf16ToRealUtf8(1))
        assertEquals(4, proj.displayUtf16ToRealUtf8(2))
    }

    @Test
    fun identityProjectionEmojiUtf8ToUtf16() {
        val text = "a😀b"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(1))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(5))
        assertEquals(4, proj.realUtf8ToDisplayUtf16(6))
    }

    @Test
    fun identityProjectionEmojiUtf16ToUtf8() {
        val text = "a😀b"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(1, proj.displayUtf16ToRealUtf8(1))
        assertEquals(5, proj.displayUtf16ToRealUtf8(3))
        assertEquals(6, proj.displayUtf16ToRealUtf8(4))
    }

    @Test
    fun identityProjectionMidByteSnapsToCodePoint() {
        val text = "中a"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(1))
        assertEquals(0, proj.realUtf8ToDisplayUtf16(2))
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
    fun maskedProjectionChineseUtf8ToDisplayUtf16() {
        val text = "中a"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(3))
        assertEquals(2, proj.realUtf8ToDisplayUtf16(4))
    }

    @Test
    fun maskedProjectionChineseDisplayUtf16ToRealUtf8() {
        val text = "中a"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(3, proj.displayUtf16ToRealUtf8(1))
        assertEquals(4, proj.displayUtf16ToRealUtf8(2))
    }

    @Test
    fun maskedProjectionEmojiUtf8ToDisplayUtf16() {
        val text = "a😀b"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(1))
        assertEquals(2, proj.realUtf8ToDisplayUtf16(5))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(6))
    }

    @Test
    fun maskedProjectionEmojiDisplayUtf16ToRealUtf8() {
        val text = "a😀b"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(1, proj.displayUtf16ToRealUtf8(1))
        assertEquals(5, proj.displayUtf16ToRealUtf8(2))
        assertEquals(6, proj.displayUtf16ToRealUtf8(3))
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

    @Test
    fun maskedProjectionPreservesNewlines() {
        val text = "ab\ncd"
        val proj = DisplayTextProjection.masked(text)
        assertEquals("\u2022\u2022\n\u2022\u2022", proj.displayText)
    }

    @Test
    fun maskedWithCompositionPreservesNewlines() {
        val text = "ab\ncd"
        val proj = DisplayTextProjection.maskedWithComposition(text, 0, 2, "ab")
        assertEquals("ab\n\u2022\u2022", proj.displayText)
    }

    @Test
    fun maskedWithCompositionChineseOffsetMapping() {
        val text = "你好世界"
        val proj = DisplayTextProjection.maskedWithComposition(text, 2, 4, "好世")
        assertEquals("\u2022\u2022好世", proj.displayText)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(3))
        assertEquals(2, proj.realUtf8ToDisplayUtf16(6))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(9))
        assertEquals(4, proj.realUtf8ToDisplayUtf16(12))
    }

    @Test
    fun maskedEmojiOffsetMapping() {
        val text = "a😀b"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(1))
        assertEquals(2, proj.realUtf8ToDisplayUtf16(5))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(6))

        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(1, proj.displayUtf16ToRealUtf8(1))
        assertEquals(5, proj.displayUtf16ToRealUtf8(2))
        assertEquals(6, proj.displayUtf16ToRealUtf8(3))
    }

    @Test
    fun identityRoundTripChineseAtCodePointBoundaries() {
        val text = "中文字"
        val proj = DisplayTextProjection.identity(text)
        val codePointUtf8Boundaries = mutableListOf(0)
        var bytePos = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val utf8Len =
                when {
                    codePoint <= 0x7F -> 1
                    codePoint <= 0x7FF -> 2
                    codePoint <= 0xFFFF -> 3
                    else -> 4
                }
            bytePos += utf8Len
            codePointUtf8Boundaries.add(bytePos)
            i += Character.charCount(codePoint)
        }
        for (utf8 in codePointUtf8Boundaries) {
            val utf16 = proj.realUtf8ToDisplayUtf16(utf8)
            val back = proj.displayUtf16ToRealUtf8(utf16)
            assertEquals("Round-trip for utf8=$utf8", utf8, back)
        }
    }

    @Test
    fun maskedRoundTripChineseAtCodePointBoundaries() {
        val text = "中文字"
        val proj = DisplayTextProjection.masked(text)
        val codePointUtf8Boundaries = mutableListOf(0)
        var bytePos = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val utf8Len =
                when {
                    codePoint <= 0x7F -> 1
                    codePoint <= 0x7FF -> 2
                    codePoint <= 0xFFFF -> 3
                    else -> 4
                }
            bytePos += utf8Len
            codePointUtf8Boundaries.add(bytePos)
            i += Character.charCount(codePoint)
        }
        for (utf8 in codePointUtf8Boundaries) {
            val utf16 = proj.realUtf8ToDisplayUtf16(utf8)
            val back = proj.displayUtf16ToRealUtf8(utf16)
            assertEquals("Round-trip for utf8=$utf8", utf8, back)
        }
    }

    @Test
    fun identityRoundTripMixedAtCodePointBoundaries() {
        val text = "a中b😀c"
        val proj = DisplayTextProjection.identity(text)
        val codePointUtf8Boundaries = mutableListOf(0)
        var bytePos = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val utf8Len =
                when {
                    codePoint <= 0x7F -> 1
                    codePoint <= 0x7FF -> 2
                    codePoint <= 0xFFFF -> 3
                    else -> 4
                }
            bytePos += utf8Len
            codePointUtf8Boundaries.add(bytePos)
            i += Character.charCount(codePoint)
        }
        for (utf8 in codePointUtf8Boundaries) {
            val utf16 = proj.realUtf8ToDisplayUtf16(utf8)
            val back = proj.displayUtf16ToRealUtf8(utf16)
            assertEquals("Round-trip for utf8=$utf8", utf8, back)
        }
    }

    @Test
    fun maskedWithCompositionEmojiNoSurrogateSplit() {
        val text = "a😀b"
        val proj = DisplayTextProjection.maskedWithComposition(text, 1, 3, "😀")
        assertEquals("\u2022😀\u2022", proj.displayText)
    }

    @Test
    fun maskedWithCompositionEmojiOffsetMapping() {
        val text = "a😀b"
        val proj = DisplayTextProjection.maskedWithComposition(text, 1, 3, "😀")
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(1))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(5))
        assertEquals(4, proj.realUtf8ToDisplayUtf16(6))

        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(1, proj.displayUtf16ToRealUtf8(1))
        assertEquals(5, proj.displayUtf16ToRealUtf8(3))
        assertEquals(6, proj.displayUtf16ToRealUtf8(4))
    }

    @Test
    fun maskedWithCompositionMixedChineseEmoji() {
        val text = "中😀a"
        val proj = DisplayTextProjection.maskedWithComposition(text, 0, 3, "中😀")
        assertEquals("中😀\u2022", proj.displayText)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf8ToDisplayUtf16(3))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(7))
        assertEquals(4, proj.realUtf8ToDisplayUtf16(8))
    }

    @Test
    fun maskedWithCompositionEmojiRoundTrip() {
        val text = "a😀b😀c"
        val proj = DisplayTextProjection.maskedWithComposition(text, 1, 3, "😀")
        val codePointUtf8Boundaries = mutableListOf(0)
        var bytePos = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val utf8Len =
                when {
                    codePoint <= 0x7F -> 1
                    codePoint <= 0x7FF -> 2
                    codePoint <= 0xFFFF -> 3
                    else -> 4
                }
            bytePos += utf8Len
            codePointUtf8Boundaries.add(bytePos)
            i += Character.charCount(codePoint)
        }
        for (utf8 in codePointUtf8Boundaries) {
            val utf16 = proj.realUtf8ToDisplayUtf16(utf8)
            val back = proj.displayUtf16ToRealUtf8(utf16)
            assertEquals("Round-trip for utf8=$utf8", utf8, back)
        }
    }

    @Test
    fun identityRealUtf16ToDisplayUtf16Chinese() {
        val text = "中a"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.realUtf16ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf16ToDisplayUtf16(1))
        assertEquals(2, proj.realUtf16ToDisplayUtf16(2))
    }

    @Test
    fun identityDisplayUtf16ToRealUtf16Chinese() {
        val text = "中a"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.displayUtf16ToRealUtf16(0))
        assertEquals(1, proj.displayUtf16ToRealUtf16(1))
        assertEquals(2, proj.displayUtf16ToRealUtf16(2))
    }

    @Test
    fun identityRealUtf16ToDisplayUtf16Emoji() {
        val text = "a😀b"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(0, proj.realUtf16ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf16ToDisplayUtf16(1))
        assertEquals(3, proj.realUtf16ToDisplayUtf16(3))
        assertEquals(4, proj.realUtf16ToDisplayUtf16(4))
    }

    @Test
    fun maskedRealUtf16ToDisplayUtf16Chinese() {
        val text = "中a"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.realUtf16ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf16ToDisplayUtf16(1))
        assertEquals(2, proj.realUtf16ToDisplayUtf16(2))
    }

    @Test
    fun maskedDisplayUtf16ToRealUtf16Chinese() {
        val text = "中a"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.displayUtf16ToRealUtf16(0))
        assertEquals(1, proj.displayUtf16ToRealUtf16(1))
        assertEquals(2, proj.displayUtf16ToRealUtf16(2))
    }

    @Test
    fun maskedRealUtf16ToDisplayUtf16Emoji() {
        val text = "a😀b"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.realUtf16ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf16ToDisplayUtf16(1))
        assertEquals(1, proj.realUtf16ToDisplayUtf16(2))
        assertEquals(2, proj.realUtf16ToDisplayUtf16(3))
        assertEquals(3, proj.realUtf16ToDisplayUtf16(4))
    }

    @Test
    fun maskedWithCompositionRealUtf16ToDisplayUtf16() {
        val text = "你好世界"
        val proj = DisplayTextProjection.maskedWithComposition(text, 2, 4, "好世")
        assertEquals(0, proj.realUtf16ToDisplayUtf16(0))
        assertEquals(1, proj.realUtf16ToDisplayUtf16(1))
        assertEquals(2, proj.realUtf16ToDisplayUtf16(2))
        assertEquals(3, proj.realUtf16ToDisplayUtf16(3))
        assertEquals(4, proj.realUtf16ToDisplayUtf16(4))
    }

    @Test
    fun maskedWithCompositionDisplayUtf16ToRealUtf16() {
        val text = "你好世界"
        val proj = DisplayTextProjection.maskedWithComposition(text, 2, 4, "好世")
        assertEquals(0, proj.displayUtf16ToRealUtf16(0))
        assertEquals(1, proj.displayUtf16ToRealUtf16(1))
        assertEquals(2, proj.displayUtf16ToRealUtf16(2))
        assertEquals(3, proj.displayUtf16ToRealUtf16(3))
        assertEquals(4, proj.displayUtf16ToRealUtf16(4))
        assertEquals(4, proj.displayUtf16ToRealUtf16(5))
    }

    @Test
    fun realUtf16RoundTripIdentity() {
        val text = "a中b😀c"
        val proj = DisplayTextProjection.identity(text)
        var utf16Pos = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val charCount = Character.charCount(codePoint)
            val displayUtf16 = proj.realUtf16ToDisplayUtf16(utf16Pos)
            val back = proj.displayUtf16ToRealUtf16(displayUtf16)
            assertEquals("Round-trip for realUtf16=$utf16Pos", utf16Pos, back)
            utf16Pos += charCount
            i += charCount
        }
    }

    @Test
    fun realUtf16RoundTripMasked() {
        val text = "a中b😀c"
        val proj = DisplayTextProjection.masked(text)
        var utf16Pos = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val charCount = Character.charCount(codePoint)
            val displayUtf16 = proj.realUtf16ToDisplayUtf16(utf16Pos)
            val back = proj.displayUtf16ToRealUtf16(displayUtf16)
            assertEquals("Round-trip for realUtf16=$utf16Pos", utf16Pos, back)
            utf16Pos += charCount
            i += charCount
        }
    }

    @Test
    fun realUtf16RoundTripMaskedWithComposition() {
        val text = "a😀b😀c"
        val proj = DisplayTextProjection.maskedWithComposition(text, 1, 3, "😀")
        var utf16Pos = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val charCount = Character.charCount(codePoint)
            val displayUtf16 = proj.realUtf16ToDisplayUtf16(utf16Pos)
            val back = proj.displayUtf16ToRealUtf16(displayUtf16)
            assertEquals("Round-trip for realUtf16=$utf16Pos", utf16Pos, back)
            utf16Pos += charCount
            i += charCount
        }
    }
}
