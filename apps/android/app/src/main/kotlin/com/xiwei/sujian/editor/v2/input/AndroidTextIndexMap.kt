package com.xiwei.sujian.editor.v2.input

import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidTextIndexMap(
    private val mirror: DisplayTextMirror
) {
    fun utf8ToUtf16(byteOffset: Int): Int {
        val text = mirror.getText()
        var utf16Index = 0
        var byteCount = 0
        for (char in text) {
            if (byteCount >= byteOffset) break
            byteCount += char.toString().toByteArray(Charsets.UTF_8).size
            utf16Index++
        }
        return utf16Index
    }

    fun utf16ToUtf8(utf16Offset: Int): Int {
        val text = mirror.getText()
        if (utf16Offset <= 0) return 0
        val safeOffset = utf16Offset.coerceAtMost(text.length)
        return text.substring(0, safeOffset).toByteArray(Charsets.UTF_8).size
    }

    fun utf8RangeToUtf16(startByte: Int, endByte: Int): IntRange {
        return utf8ToUtf16(startByte)..utf8ToUtf16(endByte)
    }

    fun utf16RangeToUtf8(startUtf16: Int, endUtf16: Int): Pair<Int, Int> {
        return Pair(utf16ToUtf8(startUtf16), utf16ToUtf8(endUtf16))
    }
}
