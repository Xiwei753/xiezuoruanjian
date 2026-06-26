package com.xiwei.sujian.ui

import org.junit.Assert.*
import org.junit.Test

/**
 * TypingAnimationController 中文插入/删除时 Core byte offset ↔ Android UTF-16 offset 正确性测试。
 *
 * TypingAnimationController 在 beforeTextChanged 中将 UTF-16 offset 转为 UTF-8 byte offset
 * 传给 Core，在 afterTextChanged 中将 Core 返回的 UTF-8 byte offset 转回 UTF-16 offset。
 * 这些测试验证该 roundtrip 在中文编辑场景下的正确性。
 */
class TypingAnimationControllerOffsetTest {

    @Test
    fun insertChineseChar_byteOffsetRoundtrip() {
        // 用户在 "你" 后插入 "好"
        val oldText = "你"
        val newText = "你好"
        // UTF-16: 你=0, end=1 (old); 你=0, 好=1, end=2 (new)
        // UTF-8: 你=0..2, end=3 (old); 你=0..2, 好=3..5, end=6 (new)
        
        // beforeTextChanged: old cursor at end of "你" = UTF-16 offset 1
        val oldCursorUtf16 = 1
        val oldCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(oldText, oldCursorUtf16)
        assertEquals(3, oldCursorByte)
        
        // afterTextChanged: new cursor at end of "你好" = UTF-16 offset 2
        val newCursorUtf16 = 2
        val newCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(newText, newCursorUtf16)
        assertEquals(6, newCursorByte)
        
        // Core returns rangeStart as UTF-8 byte offset for the inserted text
        // "好" starts at UTF-8 byte offset 3
        // Convert back to UTF-16 for Android Layout
        val rangeStartUtf16 = UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(newText, 3)
        assertEquals(1, rangeStartUtf16) // "好" starts at UTF-16 offset 1
    }

    @Test
    fun deleteChineseChar_byteOffsetRoundtrip() {
        // 用户删除 "好"，从 "你好" 变为 "你"
        val oldText = "你好"
        val newText = "你"
        
        // beforeTextChanged: old cursor at end of "你好" = UTF-16 offset 2
        val oldCursorUtf16 = 2
        val oldCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(oldText, oldCursorUtf16)
        assertEquals(6, oldCursorByte)
        
        // afterTextChanged: new cursor at end of "你" = UTF-16 offset 1
        val newCursorUtf16 = 1
        val newCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(newText, newCursorUtf16)
        assertEquals(3, newCursorByte)
        
        // Core returns rangeStart for deleted "好" = UTF-8 byte offset 3
        // But in afterTextChanged, we use newText for utf8→utf16 conversion
        // "好" was at UTF-8 byte offset 3 in oldText, but newText doesn't have it
        // TypingAnimationController uses pendingDeleteStart (UTF-16) from beforeTextChanged
    }

    @Test
    fun mixedChineseEnglishInsert_byteOffsetRoundtrip() {
        // 用户在 "a你b" 后插入 "好"
        val oldText = "a你b"
        val newText = "a你b好"
        // UTF-16: a=0, 你=1, b=2, end=3 (old); a=0, 你=1, b=2, 好=3, end=4 (new)
        // UTF-8: a=0, 你=1..3, b=4, end=5 (old); a=0, 你=1..3, b=4, 好=5..7, end=8 (new)
        
        val oldCursorUtf16 = 3
        val oldCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(oldText, oldCursorUtf16)
        assertEquals(5, oldCursorByte)
        
        val newCursorUtf16 = 4
        val newCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(newText, newCursorUtf16)
        assertEquals(8, newCursorByte)
        
        // Core returns rangeStart = 5 (UTF-8 byte offset of "好")
        val rangeStartUtf16 = UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(newText, 5)
        assertEquals(3, rangeStartUtf16) // "好" starts at UTF-16 offset 3
    }

    @Test
    fun insertEmoji_byteOffsetRoundtrip() {
        // 用户插入 emoji
        val oldText = "ab"
        val newText = "a😀b"
        // UTF-16: a=0, b=1, end=2 (old); a=0, 😀=1..2, b=3, end=4 (new)
        // UTF-8: a=0, b=1, end=2 (old); a=0, 😀=1..4, b=5, end=6 (new)
        
        val oldCursorUtf16 = 1 // cursor after "a"
        val oldCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(oldText, oldCursorUtf16)
        assertEquals(1, oldCursorByte)
        
        val newCursorUtf16 = 3 // cursor after emoji (2 code units)
        val newCursorByte = UtfOffsetConverter.utf16OffsetToUtf8ByteOffset(newText, newCursorUtf16)
        assertEquals(5, newCursorByte)
        
        // Core returns rangeStart = 1 (UTF-8 byte offset of emoji)
        val rangeStartUtf16 = UtfOffsetConverter.utf8ByteOffsetToUtf16Offset(newText, 1)
        assertEquals(1, rangeStartUtf16) // emoji starts at UTF-16 offset 1
    }
}
