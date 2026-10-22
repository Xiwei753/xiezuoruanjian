package com.xiwei.sujian.feature.search.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight

/**
 * 把 Core 返回的 UTF-8 byte 范围转成 Compose UTF-16 offset。
 *
 * 搜索 UI 不依赖 editor/input 的 Compose TextRange 工具，只做只读显示换算。
 * 越界或落在多字节字符中间时向前对齐到字符起始位置。
 */
internal fun utf16RangeForUtf8Bytes(
    text: String,
    byteStart: Int,
    byteEnd: Int,
): IntRange? {
    if (byteStart < 0 || byteEnd <= byteStart) return null
    val start = utf16IndexForUtf8Byte(text, byteStart)
    val end = utf16IndexForUtf8Byte(text, byteEnd)
    if (start >= end || end > text.length) return null
    return start until end
}

internal fun highlightedSearchText(
    text: String,
    utf8Ranges: List<List<Int>>,
    highlightColor: Color,
): AnnotatedString =
    buildAnnotatedString {
        append(text)
        utf8Ranges
            .mapNotNull { range ->
                if (range.size < 2) return@mapNotNull null
                utf16RangeForUtf8Bytes(text, range[0], range[1])
            }
            .sortedBy { it.first }
            .forEach { range ->
                addStyle(
                    SpanStyle(
                        background = highlightColor,
                        fontWeight = FontWeight.SemiBold,
                    ),
                    range.first,
                    range.last + 1,
                )
            }
    }

private fun utf16IndexForUtf8Byte(
    text: String,
    utf8ByteOffset: Int,
): Int {
    if (utf8ByteOffset <= 0) return 0
    var byteCount = 0
    var index = 0
    while (index < text.length) {
        if (byteCount >= utf8ByteOffset) return index
        val codePoint = text.codePointAt(index)
        val byteLength =
            when {
                codePoint <= 0x7F -> 1
                codePoint <= 0x7FF -> 2
                codePoint <= 0xFFFF -> 3
                else -> 4
            }
        if (byteCount + byteLength > utf8ByteOffset) return index
        byteCount += byteLength
        index += Character.charCount(codePoint)
    }
    return text.length
}
