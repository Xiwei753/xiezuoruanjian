package com.xiwei.sujian.ui

import org.junit.Assert.*
import org.junit.Test

class UtfOffsetConverterTest {

    @Test
    fun asciiText_utf16ToUtf8_sameOffset() {
        val text = "abc"
        assertEquals(0, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 0))
        assertEquals(1, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 1))
        assertEquals(2, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 2))
        assertEquals(3, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 3))
    }

    @Test
    fun asciiText_utf8ToUtf16_sameOffset() {
        val text = "abc"
        assertEquals(0, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 0))
        assertEquals(1, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 1))
        assertEquals(2, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 2))
        assertEquals(3, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 3))
    }

    @Test
    fun chineseChar_utf16Offset1_isUtf8ByteOffset3() {
        val text = "你"
        // UTF-16: offset 1 = end of "你"
        // UTF-8: "你" = 3 bytes, so byte offset = 3
        assertEquals(3, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 1))
    }

    @Test
    fun chineseChar_utf8ByteOffset3_isUtf16Offset1() {
        val text = "你"
        assertEquals(1, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 3))
    }

    @Test
    fun mixedText_a你b_insertOffsets() {
        val text = "a你b"
        // UTF-16 offsets: a=0, 你=1, b=2, end=3
        // UTF-8 byte offsets: a=0, 你=1, b=4, end=5
        assertEquals(0, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 0)) // 'a'
        assertEquals(1, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 1)) // start of '你'
        assertEquals(4, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 2)) // start of 'b'
        assertEquals(5, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 3)) // end

        // Reverse
        assertEquals(0, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 0))
        assertEquals(1, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 1))
        assertEquals(2, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 4))
        assertEquals(3, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 5))
    }

    @Test
    fun emoji_utf16ToUtf8() {
        val text = "😀"
        // 😀 is U+1F600, represented as surrogate pair in UTF-16 (2 code units)
        // UTF-8: 4 bytes
        assertEquals(4, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 2)) // end of emoji
    }

    @Test
    fun emoji_utf8ToUtf16() {
        val text = "😀"
        assertEquals(2, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 4))
    }

    @Test
    fun surrogatePairClamp_utf16OffsetInMiddleClampsToStart() {
        val text = "😀"
        // UTF-16 offset 1 is in the middle of surrogate pair, should clamp to 0
        // UTF-8 byte offset for clamped position 0 is 0
        val byteOffset = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 1)
        assertEquals(0, byteOffset)
    }

    @Test
    fun utf8ByteOffsetInMiddleOfMultiByteChar_clampsToCharBoundary() {
        val text = "你"
        // UTF-8 byte offset 1 or 2 is in the middle of "你" (3 bytes), should clamp to 0
        assertEquals(0, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 1))
        assertEquals(0, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 2))
    }

    @Test
    fun deleteChineseChar_offsetsCorrect() {
        val oldText = "你好"
        val newText = "你"
        // Deleting "好" at UTF-16 offset 1
        // Core expects old_cursor_index = UTF-8 byte offset of cursor before delete = 6 (end of "你好")
        // Core expects new_cursor_index = UTF-8 byte offset of cursor after delete = 3 (end of "你")
        assertEquals(6, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(oldText, 2))
        assertEquals(3, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(newText, 1))
    }

    @Test
    fun emptyText_offsetsAreZero() {
        assertEquals(0, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset("", 0))
        assertEquals(0, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset("", 0))
    }

    @Test
    fun outOfRangeClamps() {
        val text = "ab"
        assertEquals(2, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 100))
        assertEquals(2, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 100))
        assertEquals(0, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, -1))
        assertEquals(0, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, -1))
    }

    @Test
    fun mixedChineseEnglishDeleteRange() {
        val text = "a你b好c"
        // UTF-16: a=0, 你=1, b=2, 好=3, c=4, end=5
        // UTF-8: a=0, 你=1, b=4, 好=5, c=8, end=9
        assertEquals(5, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 3)) // start of '好'
        assertEquals(3, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 5)) // start of '好'
    }

    @Test
    fun zwjEmoji_doesNotCrash() {
        // 👨‍👩‍👧 = U+1F468 U+200D U+1F469 U+200D U+1F467
        // This is a ZWJ sequence with multiple code points
        val text = "a👨‍👩‍👧b"
        // Just verify it doesn't crash and roundtrips
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        val endByteOffset = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, text.length)
        assertEquals(utf8Len, endByteOffset)
        val roundtripUtf16 = UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, endByteOffset)
        assertEquals(text.length, roundtripUtf16)
    }

    @Test
    fun combiningMark_doesNotCrash() {
        // e + combining acute accent
        val text = "e\u0301x"
        // e=1 code unit, combining accent=1 code unit, x=1 code unit, total=3
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        val endByteOffset = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, text.length)
        assertEquals(utf8Len, endByteOffset)
    }

    @Test
    fun emojiSurrogatePair_utf8ByteOffsetRoundtrip() {
        // 😀 = U+1F600, surrogate pair in UTF-16, 4 bytes in UTF-8
        val text = "a😀b"
        // UTF-16: a=0, 😀=1..2, b=3, end=4
        // UTF-8: a=0, 😀=1..4, b=5, end=6
        val byteOffsetAtEmojiEnd = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 2) // after emoji
        assertEquals(5, byteOffsetAtEmojiEnd)
        val roundtrip = UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, byteOffsetAtEmojiEnd)
        assertEquals(2, roundtrip)
    }

    @Test
    fun multipleEmoji_offsetsCorrect() {
        val text = "😀😀"
        // Each emoji: 2 UTF-16 code units, 4 UTF-8 bytes
        assertEquals(4, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 2)) // after first emoji
        assertEquals(8, UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(text, 4)) // after second emoji
        assertEquals(2, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 4))
        assertEquals(4, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, 8))
    }

    @Test
    fun chineseInsertDelete_coreByteOffsetRoundtrip() {
        // Simulating Core byte offset ↔ Android UTF-16 offset for Chinese text editing
        val oldText = "你好世界"
        val newText = "你好"  // deleted "世界"
        // UTF-16: 你=0, 好=1, 世=2, 界=3, end=4
        // UTF-8: 你=0..2, 好=3..5, 世=6..8, 界=9..11, end=12
        val oldCursorUtf16 = 4 // end of text
        val newCursorUtf16 = 2 // after "好"
        val oldCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(oldText, oldCursorUtf16)
        val newCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(newText, newCursorUtf16)
        assertEquals(12, oldCursorByte)
        assertEquals(6, newCursorByte)
        // Roundtrip back
        assertEquals(4, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(oldText, oldCursorByte))
        assertEquals(2, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(newText, newCursorByte))
    }

    @Test
    fun illegalByteOffset_negativeClampsToZero() {
        val text = "abc"
        assertEquals(0, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, -5))
    }

    @Test
    fun illegalByteOffset_veryLargeClampsToEnd() {
        val text = "你好"
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        assertEquals(text.length, UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(text, utf8Len + 100))
    }
}
