package com.xiwei.sujian.editor.v2.projection

data class OffsetMapping(
    val realUtf8ToDisplayUtf16: (Int) -> Int,
    val displayUtf16ToRealUtf8: (Int) -> Int,
    val displayLengthUtf16: Int,
    val realLengthUtf8: Int
)

class DisplayTextProjection(
    val realText: String,
    val displayText: String,
    val isMasked: Boolean,
    private val offsetMapping: OffsetMapping
) {
    val realLengthUtf8: Int get() = offsetMapping.realLengthUtf8
    val displayLengthUtf16: Int get() = offsetMapping.displayLengthUtf16

    fun realUtf8ToDisplayUtf16(utf8: Int): Int =
        offsetMapping.realUtf8ToDisplayUtf16(utf8)

    fun displayUtf16ToRealUtf8(utf16: Int): Int =
        offsetMapping.displayUtf16ToRealUtf8(utf16)

    fun getDisplaySpannable(): android.text.SpannableStringBuilder =
        android.text.SpannableStringBuilder(displayText)

    companion object {
        private fun utf8ByteLength(codePoint: Int): Int = when {
            codePoint <= 0x7F -> 1
            codePoint <= 0x7FF -> 2
            codePoint <= 0xFFFF -> 3
            else -> 4
        }

        private data class BoundaryTable(
            val realUtf8: IntArray,
            val realUtf16: IntArray,
            val displayUtf16: IntArray
        )

        private fun buildBoundaryTable(realText: String, displayText: String, realToDisplayUtf16: IntArray): BoundaryTable {
            val realUtf8List = mutableListOf(0)
            val realUtf16List = mutableListOf(0)
            var bytePos = 0
            var utf16Pos = 0
            var i = 0
            while (i < realText.length) {
                val codePoint = realText.codePointAt(i)
                bytePos += utf8ByteLength(codePoint)
                utf16Pos += Character.charCount(codePoint)
                realUtf8List.add(bytePos)
                realUtf16List.add(utf16Pos)
                i += Character.charCount(codePoint)
            }
            val displayUtf16List = mutableListOf(0)
            var displayUtf16Pos = 0
            var j = 0
            while (j < displayText.length) {
                val codePoint = displayText.codePointAt(j)
                displayUtf16Pos += Character.charCount(codePoint)
                displayUtf16List.add(displayUtf16Pos)
                j += Character.charCount(codePoint)
            }
            return BoundaryTable(
                realUtf8 = realUtf8List.toIntArray(),
                realUtf16 = realUtf16List.toIntArray(),
                displayUtf16 = displayUtf16List.toIntArray()
            )
        }

        private fun lookupByBinarySearch(arr: IntArray, value: Int): Int {
            if (value <= 0) return 0
            val last = arr.lastOrNull() ?: return 0
            val safe = value.coerceAtMost(last)
            val idx = arr.binarySearch(safe)
            return if (idx >= 0) idx else -(idx + 1) - 1
        }

        fun identity(text: String): DisplayTextProjection {
            val bytes = text.toByteArray(Charsets.UTF_8)
            val realUtf8 = mutableListOf(0)
            val realUtf16 = mutableListOf(0)
            var bytePos = 0
            var utf16Pos = 0
            var i = 0
            while (i < text.length) {
                val codePoint = text.codePointAt(i)
                bytePos += utf8ByteLength(codePoint)
                utf16Pos += Character.charCount(codePoint)
                realUtf8.add(bytePos)
                realUtf16.add(utf16Pos)
                i += Character.charCount(codePoint)
            }
            val realUtf8Arr = realUtf8.toIntArray()
            val realUtf16Arr = realUtf16.toIntArray()
            val utf16Len = utf16Pos

            return DisplayTextProjection(
                realText = text,
                displayText = text,
                isMasked = false,
                offsetMapping = OffsetMapping(
                    realUtf8ToDisplayUtf16 = { utf8 ->
                        val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                        realUtf16Arr.getOrElse(idx) { utf16Len }.coerceIn(0, utf16Len)
                    },
                    displayUtf16ToRealUtf8 = { utf16 ->
                        val idx = lookupByBinarySearch(realUtf16Arr, utf16)
                        realUtf8Arr.getOrElse(idx) { bytes.size }.coerceIn(0, bytes.size)
                    },
                    displayLengthUtf16 = utf16Len,
                    realLengthUtf8 = bytes.size
                )
            )
        }

        fun maskedWithComposition(
            text: String,
            compStartUtf16: Int,
            compEndUtf16: Int,
            compText: String,
            maskChar: String = "\u2022"
        ): DisplayTextProjection {
            val bytes = text.toByteArray(Charsets.UTF_8)
            val utf16Len = text.length
            var safeCompStart = compStartUtf16.coerceIn(0, utf16Len)
            var safeCompEnd = compEndUtf16.coerceIn(safeCompStart, utf16Len)
            if (safeCompStart > 0 && safeCompStart < utf16Len && Character.isLowSurrogate(text[safeCompStart])) {
                safeCompStart--
            }
            if (safeCompEnd > safeCompStart && safeCompEnd < utf16Len && Character.isLowSurrogate(text[safeCompEnd])) {
                safeCompEnd--
            }

            val displayText = buildString {
                var i = 0
                while (i < safeCompStart) {
                    val codePoint = text.codePointAt(i)
                    val charCount = Character.charCount(codePoint)
                    if (codePoint == '\n'.code) append('\n')
                    else append(maskChar)
                    i += charCount
                }
                append(compText)
                i = safeCompEnd
                while (i < utf16Len) {
                    val codePoint = text.codePointAt(i)
                    val charCount = Character.charCount(codePoint)
                    if (codePoint == '\n'.code) append('\n')
                    else append(maskChar)
                    i += charCount
                }
            }
            val displayLen = displayText.length

            val realUtf8List = mutableListOf(0)
            val realUtf16List = mutableListOf(0)
            var bytePos = 0
            var utf16Pos = 0
            var i = 0
            while (i < text.length) {
                val codePoint = text.codePointAt(i)
                bytePos += utf8ByteLength(codePoint)
                utf16Pos += Character.charCount(codePoint)
                realUtf8List.add(bytePos)
                realUtf16List.add(utf16Pos)
                i += Character.charCount(codePoint)
            }
            val realUtf8Arr = realUtf8List.toIntArray()
            val realUtf16Arr = realUtf16List.toIntArray()

            val codePointCount = realUtf8Arr.size - 1
            var compStartCp: Int = 0
            var cpIdx = 0
            compStartCp@ while (cpIdx < realUtf16Arr.size) {
                if (realUtf16Arr[cpIdx] >= safeCompStart) {
                    compStartCp = cpIdx
                    break@compStartCp
                }
                cpIdx++
            }
            if (cpIdx >= realUtf16Arr.size) compStartCp = codePointCount

            var compEndCp: Int = 0
            var cpIdx2 = 0
            compEndCp@ while (cpIdx2 < realUtf16Arr.size) {
                if (realUtf16Arr[cpIdx2] >= safeCompEnd) {
                    compEndCp = cpIdx2
                    break@compEndCp
                }
                cpIdx2++
            }
            if (cpIdx2 >= realUtf16Arr.size) compEndCp = codePointCount

            val compTextUtf16Len = compText.length
            val preCompDisplayUtf16 = compStartCp
            val postCompDisplayUtf16Start = preCompDisplayUtf16 + compTextUtf16Len

            val realUtf8ToDisplayUtf16Mapping = { utf8: Int ->
                val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                val cpPos = idx.coerceIn(0, codePointCount)
                when {
                    cpPos < compStartCp -> cpPos
                    cpPos < compEndCp -> {
                        if (compEndCp == compStartCp) compStartCp
                        else compStartCp + ((cpPos - compStartCp) * compTextUtf16Len / (compEndCp - compStartCp))
                    }
                    else -> postCompDisplayUtf16Start + (cpPos - compEndCp)
                }.coerceIn(0, displayLen)
            }

            val displayUtf16ToRealUtf8Mapping = { utf16: Int ->
                val safeUtf16 = utf16.coerceIn(0, displayLen)
                when {
                    safeUtf16 < preCompDisplayUtf16 -> {
                        val cpPos = safeUtf16
                        realUtf8Arr.getOrElse(cpPos) { bytes.size }.coerceIn(0, bytes.size)
                    }
                    safeUtf16 < postCompDisplayUtf16Start -> {
                        if (compTextUtf16Len == 0) {
                            realUtf8Arr.getOrElse(compStartCp) { bytes.size }.coerceIn(0, bytes.size)
                        } else {
                            val cpPos = compStartCp + ((safeUtf16 - preCompDisplayUtf16) * (compEndCp - compStartCp) / compTextUtf16Len)
                            realUtf8Arr.getOrElse(cpPos.coerceIn(compStartCp, compEndCp)) { bytes.size }.coerceIn(0, bytes.size)
                        }
                    }
                    else -> {
                        val cpPos = compEndCp + (safeUtf16 - postCompDisplayUtf16Start)
                        realUtf8Arr.getOrElse(cpPos.coerceIn(0, codePointCount)) { bytes.size }.coerceIn(0, bytes.size)
                    }
                }
            }

            return DisplayTextProjection(
                realText = text,
                displayText = displayText,
                isMasked = true,
                offsetMapping = OffsetMapping(
                    realUtf8ToDisplayUtf16 = realUtf8ToDisplayUtf16Mapping,
                    displayUtf16ToRealUtf8 = displayUtf16ToRealUtf8Mapping,
                    displayLengthUtf16 = displayLen,
                    realLengthUtf8 = bytes.size
                )
            )
        }

        fun masked(text: String, maskChar: String = "\u2022"): DisplayTextProjection {
            val bytes = text.toByteArray(Charsets.UTF_8)
            val displayText = buildString {
                var i = 0
                while (i < text.length) {
                    val codePoint = text.codePointAt(i)
                    if (codePoint == '\n'.code) append('\n')
                    else append(maskChar)
                    i += Character.charCount(codePoint)
                }
            }
            val displayLen = displayText.length

            val realUtf8List = mutableListOf(0)
            var bytePos = 0
            var i = 0
            while (i < text.length) {
                val codePoint = text.codePointAt(i)
                bytePos += utf8ByteLength(codePoint)
                realUtf8List.add(bytePos)
                i += Character.charCount(codePoint)
            }
            val realUtf8Arr = realUtf8List.toIntArray()
            val codePointCount = realUtf8Arr.size - 1

            return DisplayTextProjection(
                realText = text,
                displayText = displayText,
                isMasked = true,
                offsetMapping = OffsetMapping(
                    realUtf8ToDisplayUtf16 = { utf8 ->
                        val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                        idx.coerceIn(0, codePointCount).coerceIn(0, displayLen)
                    },
                    displayUtf16ToRealUtf8 = { utf16 ->
                        val cpPos = utf16.coerceIn(0, codePointCount)
                        realUtf8Arr.getOrElse(cpPos) { bytes.size }.coerceIn(0, bytes.size)
                    },
                    displayLengthUtf16 = displayLen,
                    realLengthUtf8 = bytes.size
                )
            )
        }
    }
}
