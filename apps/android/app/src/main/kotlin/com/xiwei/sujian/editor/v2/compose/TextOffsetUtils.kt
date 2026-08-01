package com.xiwei.sujian.editor.v2.compose

object TextOffsetUtils {
    fun safeCharIndex(text: String, rawIndex: Int): Int {
        val clamped = rawIndex.coerceIn(0, text.length)
        return if (clamped in 1 until text.length && text[clamped].isLowSurrogate()) clamped - 1 else clamped
    }

    fun utf8OffsetForCharIndex(text: String, charIndex: Int): Int {
        val safe = safeCharIndex(text, charIndex)
        return text.substring(0, safe).toByteArray(Charsets.UTF_8).size
    }

    fun insertAtCursor(text: String, cursorUtf16: Int, insertText: String): Pair<String, Int> {
        val before = text.substring(0, cursorUtf16)
        val after = text.substring(cursorUtf16)
        val newText = before + insertText + after
        val newCursor = cursorUtf16 + insertText.length
        return newText to newCursor
    }

    fun replaceSelection(text: String, selStart: Int, selEnd: Int, insertText: String): Pair<String, Int> {
        val before = text.substring(0, selStart)
        val after = text.substring(selEnd)
        val newText = before + insertText + after
        val newCursor = selStart + insertText.length
        return newText to newCursor
    }
}
