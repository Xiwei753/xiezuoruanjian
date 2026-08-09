package com.xiwei.sujian.feature.editor.input

import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror

/**
 * Bidirectional UTF-8 ↔ UTF-16 offset mapping for a fixed text snapshot.
 *
 * Alignment guarantee: all conversions operate at code-point boundaries, not arbitrary
 * byte offsets. UTF-8 byte offsets that fall inside a multi-byte sequence are snapped
 * to the nearest code-point boundary via binary search. This is essential because the
 * Rust EditorKernel uses UTF-8 byte offsets exclusively, while Android's Layout and
 * InputConnection APIs use UTF-16 offsets — every cross-boundary call must go through
 * this mapping to avoid misaligned offsets that would produce invalid edits.
 *
 * Thread constraint: not thread-safe; each instance is bound to a single text snapshot
 * and must not be shared across threads. Rebuild after any text mutation.
 */
class AndroidTextIndexMap private constructor(
    private val text: String,
) {
    constructor(mirror: DisplayTextMirror) : this(mirror.getText())

    companion object {
        fun fromText(text: String): AndroidTextIndexMap = AndroidTextIndexMap(text)

        /**
         * Compute the resulting selection (UTF-8 byte offsets) after an IME commit operation.
         *
         * [newCursorPosition] follows Android's InputConnection convention:
         * - `> 0`: cursor is placed after the replacement, 1-indexed from replacement end.
         * - `<= 0`: cursor is placed before the replacement, 0-indexed from replacement start.
         *
         * Returns a collapsed selection (anchor == head) in UTF-8 byte offsets,
         * clamped to valid char boundaries within the virtual text.
         */
        fun computeResultingSelectionUtf8(
            committedText: String,
            newCursorPosition: Int,
            replaceStartUtf8: Int,
            replaceEndUtf8: Int,
            replacementText: String,
        ): Pair<Int, Int> {
            // newCursorPosition follows Android's InputConnection convention:
            // > 0: cursor is after the replacement, 1-indexed from replacement end.
            // <= 0: cursor is before the replacement, 0-indexed from replacement start.
            val committedBytes = committedText.toByteArray(Charsets.UTF_8)
            val safeStart = replaceStartUtf8.coerceIn(0, committedBytes.size)
            val safeEnd = replaceEndUtf8.coerceIn(safeStart, committedBytes.size)
            val virtualText =
                String(committedBytes, 0, safeStart, Charsets.UTF_8) +
                    replacementText +
                    String(committedBytes, safeEnd, committedBytes.size - safeEnd, Charsets.UTF_8)
            val virtualIndexMap = fromText(virtualText)
            val replaceStartUtf16 = virtualIndexMap.utf8ToUtf16(safeStart)
            val replacementUtf16Len = countUtf16CodeUnits(replacementText)
            val replaceEndUtf16 = replaceStartUtf16 + replacementUtf16Len
            val totalUtf16 = virtualIndexMap.getUtf16Length()

            val targetUtf16: Int
            if (newCursorPosition > 0) {
                targetUtf16 = (replaceEndUtf16 + newCursorPosition - 1).coerceIn(0, totalUtf16)
            } else {
                targetUtf16 = (replaceStartUtf16 + newCursorPosition).coerceIn(0, totalUtf16)
            }
            val targetUtf8 = virtualIndexMap.utf16ToUtf8(targetUtf16)
            return Pair(targetUtf8, targetUtf8)
        }

        fun countUtf16CodeUnits(text: String): Int {
            var count = 0
            var i = 0
            while (i < text.length) {
                val codePoint = text.codePointAt(i)
                count += Character.charCount(codePoint)
                i += Character.charCount(codePoint)
            }
            return count
        }
    }

    private val byteBoundaries: IntArray by lazy { buildByteBoundaries() }
    private val utf16Positions: IntArray by lazy { buildUtf16Positions() }
    private val _utf16Length: Int by lazy { countUtf16CodeUnits() }
    private val _utf8Length: Int by lazy { text.toByteArray(Charsets.UTF_8).size }

    fun utf8ToUtf16(byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        val maxByte = byteBoundaries.lastOrNull() ?: 0
        val safeOffset = byteOffset.coerceAtMost(maxByte)
        val idx = byteBoundaries.binarySearch(safeOffset)
        // If byteOffset falls inside a multi-byte UTF-8 sequence, binarySearch returns the
        // insertion point; -(idx+1)-1 snaps to the nearest preceding code-point boundary,
        // ensuring the result is always a valid UTF-16 offset.
        val pos = if (idx >= 0) idx else -(idx + 1) - 1
        return if (pos >= 0 && pos < utf16Positions.size) utf16Positions[pos] else _utf16Length
    }

    fun utf16ToUtf8(utf16Offset: Int): Int {
        if (utf16Offset <= 0) return 0
        val maxUtf16 = utf16Positions.lastOrNull() ?: 0
        val safeOffset = utf16Offset.coerceAtMost(maxUtf16)
        val idx = utf16Positions.binarySearch(safeOffset)
        // Same snap-to-code-point logic as utf8ToUtf16: falls back to the nearest
        // preceding code-point boundary when the offset is inside a surrogate pair.
        val pos = if (idx >= 0) idx else -(idx + 1) - 1
        return if (pos >= 0 && pos < byteBoundaries.size) byteBoundaries[pos] else _utf8Length
    }

    fun utf8RangeToUtf16(
        startByte: Int,
        endByte: Int,
    ): IntRange {
        return utf8ToUtf16(startByte)..utf8ToUtf16(endByte)
    }

    fun utf16RangeToUtf8(
        startUtf16: Int,
        endUtf16: Int,
    ): Pair<Int, Int> {
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
