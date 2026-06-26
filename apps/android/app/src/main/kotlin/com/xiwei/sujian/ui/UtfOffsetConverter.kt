package com.xiwei.sujian.ui

/**
 * UTF-16 ↔ UTF-8 byte offset 转换工具。
 *
 * Core EditorEngine 使用 UTF-8 byte offset（range_start, old_cursor_index, new_cursor_index），
 * Android Layout/Editable 使用 UTF-16 code unit offset（selectionStart, getLineForOffset 等）。
 * 在调用 Core 前必须把 UTF-16 转成 UTF-8；Core 返回后必须把 UTF-8 转回 UTF-16。
 */
object UtfOffsetConverter {

    /**
     * 将 UTF-16 code unit offset 转换为 UTF-8 byte offset。
     * 不合法时 clamp 到最近合法字符边界。
     */
    fun utf16OffsetToUtf8ByteOffset(text: String, utf16Offset: Int): Int {
        val clampedOffset = utf16Offset.coerceIn(0, text.length)
        // 如果 offset 落在 surrogate pair 中间，clamp 到 pair 起始
        var adjustedOffset = clampedOffset
        if (adjustedOffset > 0 && adjustedOffset < text.length) {
            if (Character.isLowSurrogate(text[adjustedOffset]) && Character.isHighSurrogate(text[adjustedOffset - 1])) {
                adjustedOffset = adjustedOffset - 1
            }
        }
        return text.substring(0, adjustedOffset).toByteArray(Charsets.UTF_8).size
    }

    /**
     * 将 UTF-8 byte offset 转换为 UTF-16 code unit offset。
     * 不合法时 clamp 到最近合法字符边界。
     */
    fun utf8ByteOffsetToUtf16Offset(text: String, byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        val utf8Bytes = text.toByteArray(Charsets.UTF_8)
        if (byteOffset >= utf8Bytes.size) return text.length

        // 从 UTF-8 字节流中找到合法字符边界对应的 UTF-16 offset
        var utf16Index = 0
        var byteIndex = 0
        var i = 0
        while (i < text.length && byteIndex < byteOffset) {
            val codePoint = text.codePointAt(i)
            val charCount = Character.charCount(codePoint)
            val charByteLen = text.substring(i, i + charCount).toByteArray(Charsets.UTF_8).size
            if (byteIndex + charByteLen > byteOffset) {
                // byte offset 落在多字节字符中间，clamp 到字符起始
                break
            }
            byteIndex += charByteLen
            utf16Index += charCount
            i += charCount
        }
        return utf16Index
    }
}
