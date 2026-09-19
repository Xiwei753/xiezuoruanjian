package com.xiwei.sujian.feature.editor.layout

/**
 * Issue #717 评论 5741910919：西文软断行显示投影。
 *
 * 长西文单词（连续 ASCII 字母/数字）在默认换行算法下会被整词推到下一行，
 * 上一行留下大块空白。本投影在长单词内部每隔若干字符插入 U+200B（零宽空格），
 * 让换行算法可以在这些位置断行，同时不改变正文存储（纯显示层）。
 *
 * 不在 CJK 字符间插入（CJK 本身可任意断行）。不在单词开头/结尾插入（只在内部）。
 *
 * @param rawLength 原始文本长度
 * @param insertPoints 有序的 raw offset 列表，在每个 offset 前插入一个 U+200B
 */
data class EditorSoftBreakProjection(
    val rawLength: Int,
    val insertPoints: List<Int>,
) {
    val displayLength: Int get() = rawLength + insertPoints.size

    /** raw offset → display offset。identity 快速路径：insertPoints 为空直接返回。 */
    fun rawToDisplay(rawOffset: Int): Int {
        if (insertPoints.isEmpty()) return rawOffset
        val safe = rawOffset.coerceIn(0, rawLength)
        return safe + countInsertsUpTo(safe)
    }

    /** display offset → raw offset。identity 快速路径。 */
    fun displayToRaw(displayOffset: Int): Int {
        if (insertPoints.isEmpty()) return displayOffset
        val safe = displayOffset.coerceIn(0, displayLength)
        // 二分搜索最大的 rawOffset 使得 rawToDisplay(rawOffset) <= safe
        var lo = 0
        var hi = rawLength
        while (lo < hi) {
            val mid = (lo + hi + 1) / 2
            if (mid + countInsertsUpTo(mid) <= safe) lo = mid else hi = mid - 1
        }
        return lo
    }

    /** 二分搜索 insertPoints 中 <= rawOffset 的数量。 */
    private fun countInsertsUpTo(rawOffset: Int): Int {
        var lo = 0
        var hi = insertPoints.size
        while (lo < hi) {
            val mid = (lo + hi) / 2
            if (insertPoints[mid] <= rawOffset) lo = mid + 1 else hi = mid
        }
        return lo
    }

    companion object {
        const val ZERO_WIDTH_SPACE = '\u200B'
        const val LONG_WORD_THRESHOLD = 13
        const val SEGMENT_SIZE = 6

        /** 恒等投影（不插入任何 U+200B）。 */
        fun identity(): EditorSoftBreakProjection = EditorSoftBreakProjection(0, emptyList())

        /**
         * 从原始正文计算软断行投影。
         *
         * 算法：识别连续 ASCII 字母/数字组成的"西文单词"，长度 >= [longWordThreshold]
         * 的单词内部每隔 [segmentSize] 个字符插入一个 U+200B。
         *
         * 插入点 = wordStart + segmentSize, wordStart + 2*segmentSize, ...，
         * 且必须 < wordEnd（不在单词末尾插入，避免在单词边界多加零宽空格）。
         */
        fun fromRawText(
            raw: CharSequence,
            longWordThreshold: Int = LONG_WORD_THRESHOLD,
            segmentSize: Int = SEGMENT_SIZE,
        ): EditorSoftBreakProjection {
            if (segmentSize <= 0 || longWordThreshold <= 0) {
                return EditorSoftBreakProjection(raw.length, emptyList())
            }
            val insertPoints = mutableListOf<Int>()
            val len = raw.length
            var i = 0
            while (i < len) {
                if (isAsciiLetterOrDigit(raw[i])) {
                    val wordEnd = scanWordEnd(raw, i, len)
                    addInsertPointsForWord(
                        insertPoints,
                        wordStart = i,
                        wordEnd = wordEnd,
                        longWordThreshold = longWordThreshold,
                        segmentSize = segmentSize,
                    )
                    i = wordEnd
                } else {
                    i += 1
                }
            }
            return EditorSoftBreakProjection(len, insertPoints)
        }

        /** 从 [start] 起扫描连续西文字符，返回单词结束位置（exclusive）。 */
        private fun scanWordEnd(
            raw: CharSequence,
            start: Int,
            len: Int,
        ): Int {
            var i = start
            while (i < len && isAsciiLetterOrDigit(raw[i])) {
                i += 1
            }
            return i
        }

        /**
         * 对一个西文单词 [wordStart, wordEnd) 计算内部 U+200B 插入点并追加到 [insertPoints]。
         * 仅当单词长度 >= [longWordThreshold] 时插入；从 wordStart+segmentSize 起每隔
         * segmentSize 记录一个插入点，严格 < wordEnd（不在单词末尾插入）。
         */
        private fun addInsertPointsForWord(
            insertPoints: MutableList<Int>,
            wordStart: Int,
            wordEnd: Int,
            longWordThreshold: Int,
            segmentSize: Int,
        ) {
            if (wordEnd - wordStart < longWordThreshold) return
            var next = wordStart + segmentSize
            while (next < wordEnd) {
                insertPoints.add(next)
                next += segmentSize
            }
        }

        private fun isAsciiLetterOrDigit(c: Char): Boolean = c in 'a'..'z' || c in 'A'..'Z' || c in '0'..'9'
    }
}
