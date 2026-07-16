package com.xiwei.sujian.editor.v2.input

import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidTextIndexMap(
    private val mirror: DisplayTextMirror
) {
    private val text: String = mirror.getText()
    private val utf8ToUtf16Cache: IntArray by lazy { buildUtf8ToUtf16Map() }
    private val utf16ToUtf8Cache: IntArray by lazy { buildUtf16ToUtf8Map() }
    private val _utf16Length: Int by lazy { countUtf16CodeUnits() }

    fun utf8ToUtf16(byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        val maxByte = utf8ToUtf16Cache.lastOrNull() ?: 0
        val safeOffset = byteOffset.coerceAtMost(maxByte)
        val idx = utf8ToUtf16Cache.binarySearch(safeOffset)
        return if (idx >= 0) idx else -(idx + 1) - 1
    }

    fun utf16ToUtf8(utf16Offset: Int): Int {
        if (utf16Offset <= 0) return 0
        val safeOffset = utf16Offset.coerceAtMost(utf16ToUtf8Cache.lastIndex)
        return utf16ToUtf8Cache[safeOffset]
    }

    fun utf8RangeToUtf16(startByte: Int, endByte: Int): IntRange {
        return utf8ToUtf16(startByte)..utf8ToUtf16(endByte)
    }

    fun utf16RangeToUtf8(startUtf16: Int, endUtf16: Int): Pair<Int, Int> {
        return Pair(utf16ToUtf8(startUtf16), utf16ToUtf8(endUtf16))
    }

    fun getUtf16Length(): Int = _utf16Length

    private fun countUtf16CodeUnits(): Int {
        var count = 0
        for (char in text) {
            count += if (char.isSurrogate()) {
                if (char.isHighSurrogate()) 2 else 0
            } else {
                1
            }
        }
        return count
    }

    private fun buildUtf8ToUtf16Map(): IntArray {
        val byteBoundaryToUtf16 = mutableListOf(0)
        var byteCount = 0
        var utf16Count = 0
        var i = 0
        while (i < text.length) {
            val char = text[i]
            val utf8Len = char.toString().toByteArray(Charsets.UTF_8).size
            val utf16Len = if (char.isHighSurrogate() && i + 1 < text.length && text[i + 1].isLowSurrogate()) {
                2
            } else if (char.isLowSurrogate()) {
                0
            } else {
                1
            }
            byteCount += utf8Len
            utf16Count += utf16Len
            byteBoundaryToUtf16.add(byteCount)
            i++
        }
        return byteBoundaryToUtf16.toIntArray()
    }

    private fun buildUtf16ToUtf8Map(): IntArray {
        val utf16Boundaries = mutableListOf(0)
        var byteCount = 0
        var utf16Count = 0
        var i = 0
        while (i < text.length) {
            val char = text[i]
            val utf8Len = char.toString().toByteArray(Charsets.UTF_8).size
            val utf16Len = if (char.isHighSurrogate() && i + 1 < text.length && text[i + 1].isLowSurrogate()) {
                2
            } else if (char.isLowSurrogate()) {
                0
            } else {
                1
            }
            byteCount += utf8Len
            utf16Count += utf16Len
            if (utf16Len == 2) {
                utf16Boundaries.add(byteCount)
                utf16Boundaries.add(byteCount)
            } else if (utf16Len == 1) {
                utf16Boundaries.add(byteCount)
            }
            i++
        }
        return utf16Boundaries.toIntArray()
    }
}
