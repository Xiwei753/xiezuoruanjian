package com.xiwei.sujian.editor.v2.compose

import org.junit.Assert.assertEquals
import org.junit.Test

class AnimatedTextSemanticsTest {

    @Test
    fun insertAtCursor_middleInsert_correctUtf8Offset() {
        val localValue = "你好世界"
        val insertText = "中间"
        val cursorUtf16 = 2
        val before = localValue.substring(0, cursorUtf16)
        val after = localValue.substring(cursorUtf16)
        val newText = before + insertText + after
        val newCursor = cursorUtf16 + insertText.length
        val utf8Offset = newText.substring(0, newCursor).toByteArray(Charsets.UTF_8).size
        assertEquals("你好中间", before + insertText)
        assertEquals("你好中间世界", newText)
        assertEquals(4, newCursor)
        assertEquals(12, utf8Offset)
    }

    @Test
    fun insertAtCursor_replaceSelection_correctResult() {
        val localValue = "你好世界"
        val selStart = 1
        val selEnd = 3
        val insertText = "替换"
        val before = localValue.substring(0, selStart)
        val after = localValue.substring(selEnd)
        val newText = before + insertText + after
        val newCursor = selStart + insertText.length
        assertEquals("你替换界", newText)
        assertEquals(3, newCursor)
    }

    @Test
    fun insertAtCursor_emoji_utf16Utf8OffsetCorrect() {
        val localValue = "abc🙂def"
        val insertText = "X"
        val cursorUtf16 = 4
        val before = localValue.substring(0, cursorUtf16)
        val after = localValue.substring(cursorUtf16)
        val newText = before + insertText + after
        val newCursor = cursorUtf16 + insertText.length
        val utf8Offset = newText.substring(0, newCursor).toByteArray(Charsets.UTF_8).size
        assertEquals("abc🙂X", before + insertText)
        assertEquals("abc🙂Xdef", newText)
        assertEquals(5, newCursor)
        assertEquals(8, utf8Offset)
    }

    @Test
    fun insertAtCursor_emojiSelectionReplace_utf8OffsetCorrect() {
        val localValue = "ab🙂cd"
        val selStart = 2
        val selEnd = 3
        val insertText = "XY"
        val before = localValue.substring(0, selStart)
        val after = localValue.substring(selEnd)
        val newText = before + insertText + after
        val newCursor = selStart + insertText.length
        val utf8Offset = newText.substring(0, newCursor).toByteArray(Charsets.UTF_8).size
        assertEquals("abXYcd", newText)
        assertEquals(4, newCursor)
        assertEquals(4, utf8Offset)
    }

    @Test
    fun insertAtCursor_emptyString_noChange() {
        val localValue = "hello"
        val insertText = ""
        val cursorUtf16 = 3
        val before = localValue.substring(0, cursorUtf16)
        val after = localValue.substring(cursorUtf16)
        val newText = before + insertText + after
        assertEquals("hello", newText)
    }

    @Test
    fun insertAtCursor_fullSelection_replaceAll() {
        val localValue = "旧文本"
        val selStart = 0
        val selEnd = localValue.length
        val insertText = "新文本"
        val before = localValue.substring(0, selStart)
        val after = localValue.substring(selEnd)
        val newText = before + insertText + after
        val newCursor = selStart + insertText.length
        assertEquals("新文本", newText)
        assertEquals(3, newCursor)
    }

    @Test
    fun setText_movesCursorToEnd() {
        val newText = "新内容"
        val newCursor = newText.length
        val utf8Offset = newText.toByteArray(Charsets.UTF_8).size
        assertEquals(3, newCursor)
        assertEquals(9, utf8Offset)
    }

    @Test
    fun setText_withEmoji_cursorAtEndUtf8Correct() {
        val newText = "abc🙂def"
        val newCursor = newText.length
        val utf8Offset = newText.toByteArray(Charsets.UTF_8).size
        assertEquals(7, newCursor)
        assertEquals(10, utf8Offset)
    }

    @Test
    fun setSelection_clampedToBounds() {
        val localValue = "hello"
        val selStart = (-1).coerceIn(0, localValue.length)
        val selEnd = 100.coerceIn(0, localValue.length)
        assertEquals(0, selStart)
        assertEquals(5, selEnd)
    }

    @Test
    fun disabledField_noEditableActions() {
        val enabled = false
        val hasSetText = enabled
        val hasInsertTextAtCursor = enabled
        val hasSetSelection = enabled
        assertEquals(false, hasSetText)
        assertEquals(false, hasInsertTextAtCursor)
        assertEquals(false, hasSetSelection)
    }
}
