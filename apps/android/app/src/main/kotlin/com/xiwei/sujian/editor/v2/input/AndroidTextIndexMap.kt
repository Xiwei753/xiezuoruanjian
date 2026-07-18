package com.xiwei.sujian.editor.v2.input

import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidTextIndexMap(
    private val mirror: DisplayTextMirror
) {
    private val text: String = mirror.getText()
    private val byteBoundaries: IntArray by lazy { buildByteBoundaries() }
    private val utf16Positions: IntArray by lazy { buildUtf16Positions() }
    private val _utf16Length: Int by lazy { countUtf16CodeUnits() }
    private val _utf8Length: Int by lazy { text.toByteArray(Charsets.UTF_8).size }

    fun utf8ToUtf16(byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        val maxByte = byteBoundaries.lastOrNull() ?: 0
        val safeOffset = byteOffset.coerceAtMost(maxByte)
        val idx = byteBoundaries.binarySearch(safeOffset)
        val pos = if (idx >= 0) idx else -(idx + 1) - 1
        return if (pos >= 0 && pos < utf16Positions.size) utf16Positions[pos] else _utf16Length
    }

    fun utf16ToUtf8(utf16Offset: Int): Int {
        if (utf16Offset <= 0) return 0
        val maxUtf16 = utf16Positions.lastOrNull() ?: 0
        val safeOffset = utf16Offset.coerceAtMost(maxUtf16)
        val idx = utf16Positions.binarySearch(safeOffset)
        val pos = if (idx >= 0) idx else -(idx + 1) - 1
        return if (pos >= 0 && pos < byteBoundaries.size) byteBoundaries[pos] else _utf8Length
    }

    fun utf8RangeToUtf16(startByte: Int, endByte: Int): IntRange {
        return utf8ToUtf16(startByte)..utf8ToUtf16(endByte)
    }

    fun utf16RangeToUtf8(startUtf16: Int, endUtf16: Int): Pair<Int, Int> {
        return Pair(utf16ToUtf8(startUtf16), utf16ToUtf8(endUtf16))
    }

    fun getUtf16Length(): Int = _utf16Length

    fun getUtf8Length(): Int = _utf8Length

    private fun countUtf16CodeUnits(): Int {
        var count = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            count += Character.charCount(codePoint)
            i += Character.charCount(codePoint)
        }
        return count
    }

    private fun utf8ByteLength(codePoint: Int): Int {
        return when {
            codePoint <= 0x7F -> 1
            codePoint <= 0x7FF -> 2
            codePoint <= 0xFFFF -> 3
            else -> 4
        }
    }

    private fun buildByteBoundaries(): IntArray {
        val boundaries = mutableListOf(0)
        var byteCount = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val utf8Len = utf8ByteLength(codePoint)
            val utf16Len = Character.charCount(codePoint)
            byteCount += utf8Len
            boundaries.add(byteCount)
            i += utf16Len
        }
        return boundaries.toIntArray()
    }

    private fun buildUtf16Positions(): IntArray {
        val positions = mutableListOf(0)
        var utf16Count = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            val utf16Len = Character.charCount(codePoint)
            utf16Count += utf16Len
            positions.add(utf16Count)
            i += utf16Len
        }
        return positions.toIntArray()
    }
}
