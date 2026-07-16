package com.xiwei.sujian.editor.v2.input

import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidTextIndexMap(
    private val mirror: DisplayTextMirror
) {
    private val text: String = mirror.getText()
    private val utf8ToUtf16Cache: MutableList<Int> by lazy { buildUtf8ToUtf16Map() }
    private val utf16ToUtf8Cache: IntArray by lazy { buildUtf16ToUtf8Map() }

    fun utf8ToUtf16(byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        val safeOffset = byteOffset.coerceAtMost(utf8ToUtf16Cache.lastOrNull() ?: 0)
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

    private fun buildUtf8ToUtf16Map(): MutableList<Int> {
        val map = mutableListOf(0)
        var byteCount = 0
        for (char in text) {
            byteCount += char.toString().toByteArray(Charsets.UTF_8).size
            map.add(byteCount)
        }
        return map
    }

    private fun buildUtf16ToUtf8Map(): IntArray {
        val map = IntArray(text.length + 1)
        var byteCount = 0
        map[0] = 0
        for (i in text.indices) {
            byteCount += text[i].toString().toByteArray(Charsets.UTF_8).size
            map[i + 1] = byteCount
        }
        return map
    }
}
