package com.xiwei.sujian.feature.editor.input

import androidx.compose.ui.text.TextRange

/**
 * #641 评论1 第2节：UTF-8 byte offset ↔ UTF-16 code-unit offset 健壮转换。
 *
 * Core/Rust 正文偏移是 UTF-8 byte offset；Compose [TextFieldState] / [TextRange]
 * 偏移是 UTF-16 code-unit offset。二者不能混用，否则在 CJK/emoji/surrogate 上错位。
 *
 * 所有转换都不会在非法 UTF-8 边界产生异常：落在多字节字符中间的 byte offset
 * 向前对齐到下一个字符边界；落在 surrogate 中间的 char index 向前对齐。
 */
object TextOffsetUtils {
    fun safeCharIndex(
        text: String,
        rawIndex: Int,
    ): Int {
        val clamped = rawIndex.coerceIn(0, text.length)
        return if (clamped in 1 until text.length && text[clamped].isLowSurrogate()) clamped - 1 else clamped
    }

    /**
     * UTF-16 char/code-unit index → UTF-8 byte offset。
     * [charIndex] 落在 surrogate 中间时向前对齐到 high surrogate。
     */
    fun utf8OffsetForCharIndex(
        text: String,
        charIndex: Int,
    ): Int {
        val safe = safeCharIndex(text, charIndex)
        if (safe == 0) return 0
        if (safe >= text.length) return text.toByteArray(Charsets.UTF_8).size
        var byteCount = 0
        val chars = text.toCharArray()
        var i = 0
        while (i < safe) {
            val ch = chars[i]
            if (ch.isHighSurrogate() && i + 1 < chars.size && chars[i + 1].isLowSurrogate()) {
                byteCount += 4
                i += 2
            } else if (ch.isLowSurrogate()) {
                i += 1
            } else {
                byteCount += utf8ByteLength(ch.code)
                i += 1
            }
        }
        return byteCount
    }

    /**
     * #641：UTF-8 byte offset → UTF-16 code-unit index。
     * [utf8ByteOffset] 落在多字节字符中间时向前对齐到该字符的起始 code-unit。
     * 不会在非法边界抛异常。
     */
    fun utf16OffsetForUtf8Byte(
        text: String,
        utf8ByteOffset: Int,
    ): Int {
        if (utf8ByteOffset <= 0) return 0
        val chars = text.toCharArray()
        if (chars.isEmpty()) return 0
        var byteCount = 0
        var i = 0
        while (i < chars.size) {
            if (byteCount >= utf8ByteOffset) return i
            val len = utf8SpanByteLength(chars, i)
            if (len > 0 && byteCount + len > utf8ByteOffset) return i
            byteCount += len
            i += utf8SpanCharAdvance(chars, i)
        }
        return i
    }

    /**
     * #641 评论 问题1a：UTF-8 byte offset → UTF-16 code-unit index 的统一入口。
     *
     * 先按 UTF-8 byte 长度做范围校验（不是 String.length 的 UTF-16 code-unit 数），
     * 越界返回 null；合法时委托给 [utf16OffsetForUtf8Byte]。
     *
     * 调用方不再写 `if (utf8Offset in 0..text.length)` 这种把 byte offset 和
     * UTF-16 length 混比的错误守卫。
     */
    fun utf16OffsetForUtf8ByteOrNull(
        text: String,
        utf8ByteOffset: Int,
    ): Int? {
        val byteLength = text.encodeToByteArray().size
        if (utf8ByteOffset !in 0..byteLength) return null
        return utf16OffsetForUtf8Byte(text, utf8ByteOffset)
    }

    private fun utf8SpanByteLength(
        chars: CharArray,
        index: Int,
    ): Int {
        val ch = chars[index]
        if (ch.isHighSurrogate() && index + 1 < chars.size && chars[index + 1].isLowSurrogate()) return 4
        if (ch.isLowSurrogate()) return 0
        return utf8ByteLength(ch.code)
    }

    private fun utf8SpanCharAdvance(
        chars: CharArray,
        index: Int,
    ): Int = if (chars[index].isHighSurrogate() && index + 1 < chars.size && chars[index + 1].isLowSurrogate()) 2 else 1

    /**
     * #641：把 Core 返回的 UTF-8 byte selection 转成 Compose UTF-16 [TextRange]。
     * 供 bridge rejection / authoritative text / undo-restored 路径统一调用。
     */
    fun utf16TextRangeForUtf8(
        text: String,
        utf8Start: Int,
        utf8End: Int,
    ): TextRange {
        val start = utf16OffsetForUtf8Byte(text, utf8Start.coerceAtLeast(0))
        val end = utf16OffsetForUtf8Byte(text, utf8End.coerceAtLeast(0))
        return TextRange(minOf(start, end), maxOf(start, end))
    }

    private fun utf8ByteLength(codePoint: Int): Int =
        when {
            codePoint <= 0x7F -> 1
            codePoint <= 0x7FF -> 2
            codePoint <= 0xFFFF -> 3
            else -> 4
        }

    fun insertAtCursor(
        text: String,
        cursorUtf16: Int,
        insertText: String,
    ): Pair<String, Int> {
        val before = text.substring(0, cursorUtf16)
        val after = text.substring(cursorUtf16)
        val newText = before + insertText + after
        val newCursor = cursorUtf16 + insertText.length
        return newText to newCursor
    }

    fun replaceSelection(
        text: String,
        selStart: Int,
        selEnd: Int,
        insertText: String,
    ): Pair<String, Int> {
        val before = text.substring(0, selStart)
        val after = text.substring(selEnd)
        val newText = before + insertText + after
        val newCursor = selStart + insertText.length
        return newText to newCursor
    }
}
