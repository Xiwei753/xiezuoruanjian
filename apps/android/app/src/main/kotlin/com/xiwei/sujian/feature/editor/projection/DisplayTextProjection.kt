package com.xiwei.sujian.feature.editor.projection

data class OffsetMapping(
    val realUtf8ToDisplayUtf16: (Int) -> Int,
    val displayUtf16ToRealUtf8: (Int) -> Int,
    val realUtf16ToDisplayUtf16: (Int) -> Int,
    val displayUtf16ToRealUtf16: (Int) -> Int,
    val realUtf8ToRealUtf16: (Int) -> Int,
    val realUtf16ToRealUtf8: (Int) -> Int,
    val displayLengthUtf16: Int,
    val realLengthUtf8: Int,
    val realLengthUtf16: Int,
)

/**
 * #624 评论4: [realText]/[displayText] 改为 [CharSequence] 以长期引用 mirror 的
 * SpannableStringBuilder，避免每键 mirror.getText() 整章 String 拷贝。
 * [indexRef] 非空时长度 getter 优先从增量索引读取（index 随 mirror 增量更新，
 * 长度自动反映最新内容）；为空时回退到 [offsetMapping] 的快照长度。
 */
class DisplayTextProjection(
    val realText: CharSequence,
    val displayText: CharSequence,
    val isMasked: Boolean,
    private val offsetMapping: OffsetMapping,
    private val indexRef: TextOffsetIndex? = null,
) {
    val realLengthUtf8: Int get() = indexRef?.utf8Length() ?: offsetMapping.realLengthUtf8
    val displayLengthUtf16: Int get() = indexRef?.utf16Length() ?: offsetMapping.displayLengthUtf16
    val realLengthUtf16: Int get() = indexRef?.utf16Length() ?: offsetMapping.realLengthUtf16

    fun realUtf8ToDisplayUtf16(utf8: Int): Int = offsetMapping.realUtf8ToDisplayUtf16(utf8)

    fun displayUtf16ToRealUtf8(utf16: Int): Int = offsetMapping.displayUtf16ToRealUtf8(utf16)

    fun realUtf16ToDisplayUtf16(utf16: Int): Int = offsetMapping.realUtf16ToDisplayUtf16(utf16)

    fun displayUtf16ToRealUtf16(utf16: Int): Int = offsetMapping.displayUtf16ToRealUtf16(utf16)

    fun realUtf8ToRealUtf16(utf8: Int): Int = offsetMapping.realUtf8ToRealUtf16(utf8)

    fun realUtf16ToRealUtf8(utf16: Int): Int = offsetMapping.realUtf16ToRealUtf8(utf16)

    /**
     * #624 评论4: 若 [displayText] 已是 [android.text.SpannableStringBuilder] 直接返回
     * （避免拷贝），否则用其构造（适配 String/其他 CharSequence）。
     */
    fun getDisplaySpannable(): android.text.SpannableStringBuilder =
        if (displayText is android.text.SpannableStringBuilder) {
            displayText
        } else {
            android.text.SpannableStringBuilder(displayText)
        }

    companion object {
        private fun utf8ByteLength(codePoint: Int): Int =
            when {
                codePoint <= 0x7F -> 1
                codePoint <= 0x7FF -> 2
                codePoint <= 0xFFFF -> 3
                else -> 4
            }

        private data class BoundaryTable(
            val realUtf8: IntArray,
            val realUtf16: IntArray,
            val displayUtf16: IntArray,
        )

        private fun buildRealBoundaries(text: String): Pair<IntArray, IntArray> {
            // 单遍无装箱构建（#624 评论3：identity 投影每键重建，不能逐字符装箱）。
            val capacity = text.length + 1
            val realUtf8Arr = IntArray(capacity)
            val realUtf16Arr = IntArray(capacity)
            var count = 0
            var bytePos = 0
            var utf16Pos = 0
            var i = 0
            while (i < text.length) {
                val codePoint = text.codePointAt(i)
                bytePos += utf8ByteLength(codePoint)
                utf16Pos += Character.charCount(codePoint)
                count++
                realUtf8Arr[count] = bytePos
                realUtf16Arr[count] = utf16Pos
                i += Character.charCount(codePoint)
            }
            return Pair(realUtf8Arr.copyOf(count + 1), realUtf16Arr.copyOf(count + 1))
        }

        private fun lookupByBinarySearch(
            arr: IntArray,
            value: Int,
        ): Int {
            if (value <= 0) return 0
            val last = arr.lastOrNull() ?: return 0
            val safe = value.coerceAtMost(last)
            val idx = arr.binarySearch(safe)
            return if (idx >= 0) idx else -(idx + 1) - 1
        }

        /**
         * #624 评论4: 从增量 [TextOffsetIndex] 构建 identity projection — 长期引用
         * mirror 的 index 和 spannable，不拷贝整章。index 随 mirror 增量更新，
         * offset 映射 lambda 始终读最新 index；长度 getter 优先从 [index] 读取。
         * [OffsetMapping] 的长度字段是创建时快照，仅作 [indexRef] 为 null 时的 fallback。
         */
        fun identityFromIndex(
            index: TextOffsetIndex,
            text: CharSequence,
        ): DisplayTextProjection {
            return DisplayTextProjection(
                realText = text,
                displayText = text,
                isMasked = false,
                offsetMapping =
                    OffsetMapping(
                        realUtf8ToDisplayUtf16 = { index.utf8ToUtf16(it) },
                        displayUtf16ToRealUtf8 = { index.utf16ToUtf8(it) },
                        realUtf16ToDisplayUtf16 = { it.coerceIn(0, index.utf16Length()) },
                        displayUtf16ToRealUtf16 = { it.coerceIn(0, index.utf16Length()) },
                        realUtf8ToRealUtf16 = { index.utf8ToUtf16(it) },
                        realUtf16ToRealUtf8 = { index.utf16ToUtf8(it) },
                        displayLengthUtf16 = index.utf16Length(),
                        realLengthUtf8 = index.utf8Length(),
                        realLengthUtf16 = index.utf16Length(),
                    ),
                indexRef = index,
            )
        }

        fun identity(text: String): DisplayTextProjection {
            val (realUtf8Arr, realUtf16Arr) = buildRealBoundaries(text)
            val utf16Len = realUtf16Arr.lastOrNull() ?: 0
            // 总 UTF-8 字节数来自边界构建的末位 — 不再 toByteArray 整章拷贝。
            val utf8Len = realUtf8Arr.lastOrNull() ?: 0

            return DisplayTextProjection(
                realText = text,
                displayText = text,
                isMasked = false,
                offsetMapping =
                    OffsetMapping(
                        realUtf8ToDisplayUtf16 = { utf8 ->
                            val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                            realUtf16Arr.getOrElse(idx) { utf16Len }.coerceIn(0, utf16Len)
                        },
                        displayUtf16ToRealUtf8 = { utf16 ->
                            val idx = lookupByBinarySearch(realUtf16Arr, utf16)
                            realUtf8Arr.getOrElse(idx) { utf8Len }.coerceIn(0, utf8Len)
                        },
                        realUtf16ToDisplayUtf16 = { realUtf16 ->
                            val idx = lookupByBinarySearch(realUtf16Arr, realUtf16)
                            realUtf16Arr.getOrElse(idx) { utf16Len }.coerceIn(0, utf16Len)
                        },
                        displayUtf16ToRealUtf16 = { displayUtf16 ->
                            val idx = lookupByBinarySearch(realUtf16Arr, displayUtf16)
                            realUtf16Arr.getOrElse(idx) { utf16Len }.coerceIn(0, utf16Len)
                        },
                        realUtf8ToRealUtf16 = { utf8 ->
                            val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                            realUtf16Arr.getOrElse(idx) { utf16Len }.coerceIn(0, utf16Len)
                        },
                        realUtf16ToRealUtf8 = { utf16 ->
                            val idx = lookupByBinarySearch(realUtf16Arr, utf16)
                            realUtf8Arr.getOrElse(idx) { utf8Len }.coerceIn(0, utf8Len)
                        },
                        displayLengthUtf16 = utf16Len,
                        realLengthUtf8 = utf8Len,
                        realLengthUtf16 = utf16Len,
                    ),
            )
        }

        fun maskedWithComposition(
            text: String,
            compStartUtf16: Int,
            compEndUtf16: Int,
            compText: String,
            maskChar: String = "\u2022",
        ): DisplayTextProjection {
            val utf16Len = text.length
            var safeCompStart = compStartUtf16.coerceIn(0, utf16Len)
            var safeCompEnd = compEndUtf16.coerceIn(safeCompStart, utf16Len)
            if (safeCompStart > 0 && safeCompStart < utf16Len && Character.isLowSurrogate(text[safeCompStart])) {
                safeCompStart--
            }
            if (safeCompEnd > safeCompStart && safeCompEnd < utf16Len && Character.isLowSurrogate(text[safeCompEnd])) {
                safeCompEnd--
            }

            val displayText =
                buildString {
                    var i = 0
                    while (i < safeCompStart) {
                        val codePoint = text.codePointAt(i)
                        val charCount = Character.charCount(codePoint)
                        if (codePoint == '\n'.code) {
                            append('\n')
                        } else {
                            append(maskChar)
                        }
                        i += charCount
                    }
                    append(compText)
                    i = safeCompEnd
                    while (i < utf16Len) {
                        val codePoint = text.codePointAt(i)
                        val charCount = Character.charCount(codePoint)
                        if (codePoint == '\n'.code) {
                            append('\n')
                        } else {
                            append(maskChar)
                        }
                        i += charCount
                    }
                }
            val displayLen = displayText.length

            val (realUtf8Arr, realUtf16Arr) = buildRealBoundaries(text)
            val utf8Len = realUtf8Arr.lastOrNull() ?: 0

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
                        if (compEndCp == compStartCp) {
                            compStartCp
                        } else {
                            compStartCp + ((cpPos - compStartCp) * compTextUtf16Len / (compEndCp - compStartCp))
                        }
                    }
                    else -> postCompDisplayUtf16Start + (cpPos - compEndCp)
                }.coerceIn(0, displayLen)
            }

            val displayUtf16ToRealUtf8Mapping = { utf16: Int ->
                val safeUtf16 = utf16.coerceIn(0, displayLen)
                when {
                    safeUtf16 < preCompDisplayUtf16 -> {
                        val cpPos = safeUtf16
                        realUtf8Arr.getOrElse(cpPos) { utf8Len }.coerceIn(0, utf8Len)
                    }
                    safeUtf16 < postCompDisplayUtf16Start -> {
                        if (compTextUtf16Len == 0) {
                            realUtf8Arr.getOrElse(compStartCp) { utf8Len }.coerceIn(0, utf8Len)
                        } else {
                            val cpPos =
                                compStartCp +
                                    ((safeUtf16 - preCompDisplayUtf16) * (compEndCp - compStartCp) / compTextUtf16Len)
                            realUtf8Arr.getOrElse(
                                cpPos.coerceIn(compStartCp, compEndCp),
                            ) { utf8Len }.coerceIn(0, utf8Len)
                        }
                    }
                    else -> {
                        val cpPos = compEndCp + (safeUtf16 - postCompDisplayUtf16Start)
                        realUtf8Arr.getOrElse(cpPos.coerceIn(0, codePointCount)) { utf8Len }.coerceIn(0, utf8Len)
                    }
                }
            }

            val realUtf16ToDisplayUtf16Mapping = { realUtf16: Int ->
                val idx = lookupByBinarySearch(realUtf16Arr, realUtf16)
                val cpPos = idx.coerceIn(0, codePointCount)
                when {
                    cpPos < compStartCp -> cpPos
                    cpPos < compEndCp -> {
                        if (compEndCp == compStartCp) {
                            compStartCp
                        } else {
                            compStartCp + ((cpPos - compStartCp) * compTextUtf16Len / (compEndCp - compStartCp))
                        }
                    }
                    else -> postCompDisplayUtf16Start + (cpPos - compEndCp)
                }.coerceIn(0, displayLen)
            }

            val displayUtf16ToRealUtf16Mapping = { displayUtf16: Int ->
                val safeUtf16 = displayUtf16.coerceIn(0, displayLen)
                when {
                    safeUtf16 < preCompDisplayUtf16 -> {
                        val cpPos = safeUtf16
                        realUtf16Arr.getOrElse(cpPos) { utf16Len }.coerceIn(0, utf16Len)
                    }
                    safeUtf16 < postCompDisplayUtf16Start -> {
                        if (compTextUtf16Len == 0) {
                            realUtf16Arr.getOrElse(compStartCp) { utf16Len }.coerceIn(0, utf16Len)
                        } else {
                            val cpPos =
                                compStartCp +
                                    ((safeUtf16 - preCompDisplayUtf16) * (compEndCp - compStartCp) / compTextUtf16Len)
                            realUtf16Arr.getOrElse(
                                cpPos.coerceIn(compStartCp, compEndCp),
                            ) { utf16Len }.coerceIn(0, utf16Len)
                        }
                    }
                    else -> {
                        val cpPos = compEndCp + (safeUtf16 - postCompDisplayUtf16Start)
                        realUtf16Arr.getOrElse(cpPos.coerceIn(0, codePointCount)) { utf16Len }.coerceIn(0, utf16Len)
                    }
                }
            }

            val realUtf8ToRealUtf16Mapping = { utf8: Int ->
                val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                realUtf16Arr.getOrElse(idx) { utf16Len }.coerceIn(0, utf16Len)
            }

            val realUtf16ToRealUtf8Mapping = { utf16: Int ->
                val idx = lookupByBinarySearch(realUtf16Arr, utf16)
                realUtf8Arr.getOrElse(idx) { utf8Len }.coerceIn(0, utf8Len)
            }

            return DisplayTextProjection(
                realText = text,
                displayText = displayText,
                isMasked = true,
                offsetMapping =
                    OffsetMapping(
                        realUtf8ToDisplayUtf16 = realUtf8ToDisplayUtf16Mapping,
                        displayUtf16ToRealUtf8 = displayUtf16ToRealUtf8Mapping,
                        realUtf16ToDisplayUtf16 = realUtf16ToDisplayUtf16Mapping,
                        displayUtf16ToRealUtf16 = displayUtf16ToRealUtf16Mapping,
                        realUtf8ToRealUtf16 = realUtf8ToRealUtf16Mapping,
                        realUtf16ToRealUtf8 = realUtf16ToRealUtf8Mapping,
                        displayLengthUtf16 = displayLen,
                        realLengthUtf8 = utf8Len,
                        realLengthUtf16 = utf16Len,
                    ),
            )
        }

        fun masked(
            text: String,
            maskChar: String = "\u2022",
        ): DisplayTextProjection {
            val displayText =
                buildString {
                    var i = 0
                    while (i < text.length) {
                        val codePoint = text.codePointAt(i)
                        if (codePoint == '\n'.code) {
                            append('\n')
                        } else {
                            append(maskChar)
                        }
                        i += Character.charCount(codePoint)
                    }
                }
            val displayLen = displayText.length

            val (realUtf8Arr, realUtf16Arr) = buildRealBoundaries(text)
            val codePointCount = realUtf8Arr.size - 1
            val utf16Len = realUtf16Arr.lastOrNull() ?: 0
            val utf8Len = realUtf8Arr.lastOrNull() ?: 0

            return DisplayTextProjection(
                realText = text,
                displayText = displayText,
                isMasked = true,
                offsetMapping =
                    OffsetMapping(
                        realUtf8ToDisplayUtf16 = { utf8 ->
                            val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                            idx.coerceIn(0, codePointCount).coerceIn(0, displayLen)
                        },
                        displayUtf16ToRealUtf8 = { utf16 ->
                            val cpPos = utf16.coerceIn(0, codePointCount)
                            realUtf8Arr.getOrElse(cpPos) { utf8Len }.coerceIn(0, utf8Len)
                        },
                        realUtf16ToDisplayUtf16 = { realUtf16 ->
                            val idx = lookupByBinarySearch(realUtf16Arr, realUtf16)
                            idx.coerceIn(0, codePointCount).coerceIn(0, displayLen)
                        },
                        displayUtf16ToRealUtf16 = { displayUtf16 ->
                            val cpPos = displayUtf16.coerceIn(0, codePointCount)
                            realUtf16Arr.getOrElse(cpPos) { utf16Len }.coerceIn(0, utf16Len)
                        },
                        realUtf8ToRealUtf16 = { utf8 ->
                            val idx = lookupByBinarySearch(realUtf8Arr, utf8)
                            realUtf16Arr.getOrElse(idx) { utf16Len }.coerceIn(0, utf16Len)
                        },
                        realUtf16ToRealUtf8 = { utf16 ->
                            val idx = lookupByBinarySearch(realUtf16Arr, utf16)
                            realUtf8Arr.getOrElse(idx) { utf8Len }.coerceIn(0, utf8Len)
                        },
                        displayLengthUtf16 = displayLen,
                        realLengthUtf8 = utf8Len,
                        realLengthUtf16 = utf16Len,
                    ),
            )
        }
    }
}
