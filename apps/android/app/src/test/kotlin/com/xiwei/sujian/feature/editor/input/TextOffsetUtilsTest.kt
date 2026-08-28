package com.xiwei.sujian.feature.editor.input

import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #641 评论1 第2节：UTF-8 byte ↔ UTF-16 code-unit 偏移转换边界测试。
 *
 * 覆盖：ASCII / CJK / emoji(surrogate pair) / 混合文本、前缀/后缀、
 * selection/rejection 路径、非法边界（多字节字符中间）不抛异常。
 */
@Suppress("TooManyFunctions")
class TextOffsetUtilsTest {
    companion object {
        private const val ASCII_TEXT = "Hello"
        private const val MIXED_TEXT = "a你😀b"
    }

    @Test
    fun utf16OffsetForUtf8Byte_ascii_identity() {
        val text = ASCII_TEXT
        for (i in 0..5) {
            assertEquals("ASCII byte $i == char $i", i, TextOffsetUtils.utf16OffsetForUtf8Byte(text, i))
        }
    }

    @Test
    fun utf16OffsetForUtf8Byte_cjk_threeBytesToOneUtf16() {
        val text = "你好"
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 0))
        assertEquals(1, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 3))
        assertEquals(2, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 6))
    }

    @Test
    fun utf16OffsetForUtf8Byte_cjk_midByteSnapsToCharStart() {
        val text = "你好"
        assertEquals("byte 1 在你中间 → 0", 0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 1))
        assertEquals("byte 2 在你中间 → 0", 0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 2))
        assertEquals("byte 4 在好中间 → 1", 1, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 4))
        assertEquals("byte 5 在好中间 → 1", 1, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 5))
    }

    @Test
    fun utf16OffsetForUtf8Byte_emoji_fourBytesToTwoUtf16() {
        val text = "😀"
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 0))
        assertEquals("emoji 4 bytes → 2 UTF-16 units", 2, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 4))
    }

    @Test
    fun utf16OffsetForUtf8Byte_emoji_midByteSnapsToStart() {
        val text = "😀"
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 1))
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 2))
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 3))
    }

    @Test
    fun utf16OffsetForUtf8Byte_mixedAsciiCjkEmoji() {
        val text = MIXED_TEXT
        // a=1byte/1utf16, 你=3bytes/1utf16, 😀=4bytes/2utf16, b=1byte/1utf16
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 0))
        assertEquals(1, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 1))
        assertEquals(2, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 4))
        assertEquals(4, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 8))
        assertEquals(5, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 9))
    }

    @Test
    fun utf16OffsetForUtf8Byte_overflowClampsToEnd() {
        val text = "abc"
        assertEquals(3, TextOffsetUtils.utf16OffsetForUtf8Byte(text, 100))
    }

    @Test
    fun utf16OffsetForUtf8Byte_emptyString_returnsZero() {
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte("", 0))
        assertEquals(0, TextOffsetUtils.utf16OffsetForUtf8Byte("", 10))
    }

    @Test
    fun utf8OffsetForCharIndex_ascii_identity() {
        val text = ASCII_TEXT
        for (i in 0..5) {
            assertEquals(i, TextOffsetUtils.utf8OffsetForCharIndex(text, i))
        }
    }

    @Test
    fun utf8OffsetForCharIndex_cjk() {
        val text = "你好"
        assertEquals(0, TextOffsetUtils.utf8OffsetForCharIndex(text, 0))
        assertEquals(3, TextOffsetUtils.utf8OffsetForCharIndex(text, 1))
        assertEquals(6, TextOffsetUtils.utf8OffsetForCharIndex(text, 2))
    }

    @Test
    fun utf8OffsetForCharIndex_emoji_surrogatePair() {
        val text = "😀"
        assertEquals(0, TextOffsetUtils.utf8OffsetForCharIndex(text, 0))
        assertEquals(4, TextOffsetUtils.utf8OffsetForCharIndex(text, 2))
    }

    @Test
    fun utf8OffsetForCharIndex_mixed() {
        val text = MIXED_TEXT
        assertEquals(0, TextOffsetUtils.utf8OffsetForCharIndex(text, 0))
        assertEquals(1, TextOffsetUtils.utf8OffsetForCharIndex(text, 1))
        assertEquals(4, TextOffsetUtils.utf8OffsetForCharIndex(text, 2))
        assertEquals(8, TextOffsetUtils.utf8OffsetForCharIndex(text, 4))
        assertEquals(9, TextOffsetUtils.utf8OffsetForCharIndex(text, 5))
    }

    @Test
    fun utf8OffsetForCharIndex_lowSurrogateSnapsBack() {
        val text = "😀"
        assertEquals("low surrogate index 1 snaps to 0", 0, TextOffsetUtils.utf8OffsetForCharIndex(text, 1))
    }

    @Test
    fun utf16TextRangeForUtf8_ascii() {
        val text = ASCII_TEXT
        assertEquals(TextRange(1, 3), TextOffsetUtils.utf16TextRangeForUtf8(text, 1, 3))
    }

    @Test
    fun utf16TextRangeForUtf8_cjk() {
        val text = "你好世界"
        assertEquals(TextRange(1, 3), TextOffsetUtils.utf16TextRangeForUtf8(text, 3, 9))
    }

    @Test
    fun utf16TextRangeForUtf8_emoji() {
        val text = "😀😀"
        assertEquals(TextRange(0, 2), TextOffsetUtils.utf16TextRangeForUtf8(text, 0, 4))
        assertEquals(TextRange(2, 4), TextOffsetUtils.utf16TextRangeForUtf8(text, 4, 8))
    }

    @Test
    fun utf16TextRangeForUtf8_mixed_rejectionScenario() {
        val text = MIXED_TEXT
        assertEquals(TextRange(1, 4), TextOffsetUtils.utf16TextRangeForUtf8(text, 1, 8))
    }

    @Test
    fun utf16TextRangeForUtf8_negativeClampsToZero() {
        val text = "abc"
        assertEquals(TextRange(0, 2), TextOffsetUtils.utf16TextRangeForUtf8(text, -1, 2))
    }

    @Test
    fun utf16TextRangeForUtf8_startAfterEnd_normalizes() {
        val text = "abc"
        assertEquals(TextRange(1, 2), TextOffsetUtils.utf16TextRangeForUtf8(text, 2, 1))
    }

    @Test
    fun roundTrip_utf16ToUtf8ToUtf16_cjk() {
        val text = "你好世界"
        for (utf16 in 0..4) {
            val utf8 = TextOffsetUtils.utf8OffsetForCharIndex(text, utf16)
            val back = TextOffsetUtils.utf16OffsetForUtf8Byte(text, utf8)
            assertEquals("round-trip utf16=$utf16", utf16, back)
        }
    }

    @Test
    fun roundTrip_utf16ToUtf8ToUtf16_emoji() {
        val text = "a😀b"
        for (utf16 in listOf(0, 1, 3, 4)) {
            val utf8 = TextOffsetUtils.utf8OffsetForCharIndex(text, utf16)
            val back = TextOffsetUtils.utf16OffsetForUtf8Byte(text, utf8)
            assertEquals("round-trip utf16=$utf16", utf16, back)
        }
    }
}
