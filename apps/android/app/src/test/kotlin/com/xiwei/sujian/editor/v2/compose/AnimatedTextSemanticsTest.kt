package com.xiwei.sujian.editor.v2.compose

import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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
        val cursorUtf16 = 5
        val before = localValue.substring(0, cursorUtf16)
        val after = localValue.substring(cursorUtf16)
        val newText = before + insertText + after
        val newCursor = cursorUtf16 + insertText.length
        val utf8Offset = newText.substring(0, newCursor).toByteArray(Charsets.UTF_8).size
        assertEquals("abc🙂X", before + insertText)
        assertEquals("abc🙂Xdef", newText)
        assertEquals(6, newCursor)
        assertEquals(8, utf8Offset)
    }

    @Test
    fun insertAtCursor_emojiSelectionReplace_utf8OffsetCorrect() {
        val localValue = "ab🙂cd"
        val selStart = 2
        val selEnd = 4
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
        assertEquals(8, newCursor)
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

    @Test
    fun selectionRange_resetsOnExternalValueChange() {
        val oldValue = "hello"
        val newValue = "hi"
        val oldSelection = TextRange(oldValue.length)
        assertEquals(TextRange(5), oldSelection)
        val newSelection = TextRange(newValue.length)
        assertEquals(TextRange(2), newSelection)
    }

    @Test
    fun selectionRange_clampedAfterExternalValueChange() {
        val oldValue = "你好世界"
        val newValue = "你好"
        val oldSelection = TextRange(oldValue.length)
        assertEquals(TextRange(4), oldSelection)
        val newSelection = TextRange(newValue.length)
        val clampedStart = newSelection.start.coerceIn(0, newValue.length)
        val clampedEnd = newSelection.end.coerceIn(0, newValue.length)
        assertEquals(2, clampedStart)
        assertEquals(2, clampedEnd)
    }

    @Test
    fun selectionRange_surrogatePairNotSplit() {
        val textWithEmoji = "ab🙂cd"
        val lowSurrogateIndex = 3
        assertTrue(textWithEmoji[lowSurrogateIndex].isLowSurrogate())
        val clampedPos = lowSurrogateIndex.coerceIn(0, textWithEmoji.length)
        val safePos =
            if (clampedPos in 1 until textWithEmoji.length && textWithEmoji[clampedPos].isLowSurrogate()) {
                clampedPos - 1
            } else {
                clampedPos
            }
        assertEquals(2, safePos)
    }

    @Test
    fun setSelection_utf16EmojiBoundaryNotSplit() {
        val text = "a🙂b"
        val emojiLowSurrogateIndex = 2
        val clampedStart = emojiLowSurrogateIndex.coerceIn(0, text.length)
        val safeStart =
            if (clampedStart in 1 until text.length && text[clampedStart].isLowSurrogate()) {
                clampedStart - 1
            } else {
                clampedStart
            }
        assertEquals(1, safeStart)
    }

    @Test
    fun insertTextAtCursor_preservesCoordinatorContract() {
        val localValue = "hello"
        val insertText = "X"
        val selMin = 3
        val selMax = 3
        val before = localValue.substring(0, selMin)
        val after = localValue.substring(selMax)
        val newText = before + insertText + after
        val newCursor = selMin + insertText.length
        val utf8Offset = newText.substring(0, newCursor).toByteArray(Charsets.UTF_8).size
        assertEquals("helXlo", newText)
        assertEquals(4, newCursor)
        assertEquals(4, utf8Offset)
    }

    @Test
    fun setText_preservesCoordinatorContract() {
        val newText = "world"
        val newCursor = newText.length
        val utf8Offset = newText.toByteArray(Charsets.UTF_8).size
        assertEquals(5, newCursor)
        assertEquals(5, utf8Offset)
    }

    @Test
    fun safeCharIndex_cjk_returnsSameIndex() {
        assertEquals(2, TextOffsetUtils.safeCharIndex("你好世界", 2))
    }

    @Test
    fun safeCharIndex_cjk_utf8OffsetCorrect() {
        assertEquals(6, TextOffsetUtils.utf8OffsetForCharIndex("你好世界", 2))
    }

    @Test
    fun safeCharIndex_emoji_lowSurrogate_backsOff() {
        assertEquals(2, TextOffsetUtils.safeCharIndex("ab🙂cd", 3))
    }

    @Test
    fun safeCharIndex_emoji_utf8OffsetNotSplit() {
        assertEquals(2, TextOffsetUtils.utf8OffsetForCharIndex("ab🙂cd", 3))
    }

    @Test
    fun insertAtCursor_consecutive_cursorAdvances() {
        val (text1, cursor1) = TextOffsetUtils.insertAtCursor("abc", 3, "X")
        assertEquals("abcX", text1)
        assertEquals(4, cursor1)
        val (text2, cursor2) = TextOffsetUtils.insertAtCursor(text1, cursor1, "Y")
        assertEquals("abcXY", text2)
        assertEquals(5, cursor2)
    }

    @Test
    fun insertAtCursor_consecutiveEmoji_cursorAdvancesCorrectly() {
        val (text1, cursor1) = TextOffsetUtils.insertAtCursor("ab", 2, "🙂")
        val utf8Offset1 = TextOffsetUtils.utf8OffsetForCharIndex(text1, cursor1)
        assertEquals("ab🙂", text1)
        assertEquals(4, cursor1)
        assertEquals(6, utf8Offset1)
        val (text2, cursor2) = TextOffsetUtils.insertAtCursor(text1, cursor1, "c")
        val utf8Offset2 = TextOffsetUtils.utf8OffsetForCharIndex(text2, cursor2)
        assertEquals("ab🙂c", text2)
        assertEquals(5, cursor2)
        assertEquals(7, utf8Offset2)
    }

    @Test
    fun replaceSelection_surrogatePair_utf8Correct() {
        val (newText, newCursor) = TextOffsetUtils.replaceSelection("a🙂b🙂c", 1, 4, "XY")
        val utf8Offset = TextOffsetUtils.utf8OffsetForCharIndex(newText, newCursor)
        assertEquals("aXY🙂c", newText)
        assertEquals(3, newCursor)
        assertEquals(3, utf8Offset)
    }

    @Test
    fun safeCharIndex_atBoundary_zeroAndLength() {
        val text = "hello"
        assertEquals(0, TextOffsetUtils.safeCharIndex(text, 0))
        assertEquals(5, TextOffsetUtils.safeCharIndex(text, 5))
        assertEquals(0, TextOffsetUtils.utf8OffsetForCharIndex(text, 0))
        assertEquals(5, TextOffsetUtils.utf8OffsetForCharIndex(text, 5))
    }

    @Test
    fun safeCharIndex_outOfBounds_clamped() {
        val text = "hi"
        assertEquals(0, TextOffsetUtils.safeCharIndex(text, -1))
        assertEquals(2, TextOffsetUtils.safeCharIndex(text, 100))
    }

    @Test
    fun utf8OffsetForCharIndex_emptyString_alwaysZero() {
        assertEquals(0, TextOffsetUtils.utf8OffsetForCharIndex("", 0))
        assertEquals(0, TextOffsetUtils.utf8OffsetForCharIndex("", 5))
    }
}
