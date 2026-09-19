package com.xiwei.sujian.feature.editor.layout

import android.icu.text.BreakIterator
import androidx.compose.ui.text.TextRange

/**
 * Issue #717 评论 5741910919 / 评论 5742273757 修复2：西文软断行显示投影。
 *
 * 长西文单词在默认换行算法下会被整词推到下一行，上一行留下大块空白。本投影在长单词
 * 内部的合法 grapheme boundary 插入 U+200B（零宽空格），让换行算法可以在这些位置断行，
 * 同时不改变正文存储（纯显示层）。
 *
 * 评论 5742273757 修复2：用 [BreakIterator.getCharacterInstance] 取 Unicode 逻辑字符
 * boundary，替代旧的固定6字符分段。word run 识别扩展为包含非 ASCII 拉丁字母和组合附加符号。
 *
 * Issue #717 评论 5742904417 修复3：去掉西文 6 字符阈值，改用 Unicode script。
 * 西文 word run 只要有两个 grapheme，就给所有内部 grapheme boundary 断点。
 * isLatinWordChar 改用 [Character.UnicodeScript.LATIN] 判断 base code point。
 *
 * Issue #717 评论 5743443030 修复2：isLatinWordChar 改用 [Character.codePointAt] 按 code point
 * 判断，覆盖补充平面 Latin Extended-F/G 字符和非 ASCII 数字。scanWordRuns 支持连接符 '、’、_
 * 在左右均为 Latin/数字 grapheme 时并入 word run（连字符 - 不处理）。
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

    /**
     * 评论 5742273757 修复3：raw TextRange → display TextRange。
     *
     * 凡是 range 来自正文/visual unit（raw 坐标），传给 TextLayoutResult 之前都必须经过此映射。
     */
    fun toDisplayRange(rawRange: TextRange): TextRange =
        TextRange(rawToDisplay(rawRange.start), rawToDisplay(rawRange.end))

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

        /** 恒等投影（不插入任何 U+200B）。 */
        fun identity(): EditorSoftBreakProjection = EditorSoftBreakProjection(0, emptyList())

        /**
         * 从原始正文计算软断行投影。
         *
         * 评论 5742273757 修复2算法：
         * 1. 用 [BreakIterator.getCharacterInstance] 遍历文本，识别"西文 word run"
         *    （连续的拉丁字母/数字/组合符号，不含 CJK、空格、标点）
         * 2. 对长度 >= 2 个 grapheme 的 word run，在每个 grapheme boundary
         *    （除了开头和结尾）插入 U+200B
         *
         * Issue #717 评论 5742904417 修复3：去掉 longWordThreshold 参数，
         * 西文 word run 只要有两个 grapheme 就断点。
         *
         * @param raw 原始正文
         */
        fun fromRawText(raw: CharSequence): EditorSoftBreakProjection {
            if (raw.isEmpty()) {
                return EditorSoftBreakProjection(raw.length, emptyList())
            }
            val insertPoints = mutableListOf<Int>()
            scanWordRuns(raw) { wordStart, wordEnd ->
                addInsertPointsForWord(insertPoints, raw, wordStart, wordEnd)
            }
            return EditorSoftBreakProjection(raw.length, insertPoints)
        }

        /**
         * 用 [BreakIterator.getCharacterInstance] 遍历文本，识别"西文 word run"
         * （连续的拉丁字母/数字/组合符号），对每个 word run 调用 [onWordRun]。
         *
         * Issue #717 评论 5743443030 修复2：支持连接符 '、’、_ 在左右均为 Latin/数字 grapheme
         * 时并入 word run。先收集所有 grapheme boundary 到列表，再按索引遍历，方便看前一个和
         * 后一个 grapheme。连字符 - 不处理（本身已有换行机会）。
         */
        @Suppress("CognitiveComplexMethod", "NestedBlockDepth")
        private inline fun scanWordRuns(
            raw: CharSequence,
            onWordRun: (wordStart: Int, wordEnd: Int) -> Unit,
        ) {
            val charIterator = BreakIterator.getCharacterInstance()
            charIterator.setText(raw.toString())

            // 先收集所有 grapheme boundary 到列表，方便看前一个和后一个 grapheme
            val boundaries = mutableListOf<Int>()
            var boundary = charIterator.first()
            while (boundary != BreakIterator.DONE) {
                boundaries.add(boundary)
                boundary = charIterator.next()
            }
            if (boundaries.size <= 1) return

            var wordStart = -1
            var wordEnd = 0
            val lastGraphemeIndex = boundaries.size - 2
            for (i in 0..lastGraphemeIndex) {
                val prevBoundary = boundaries[i]
                val nextBoundary = boundaries[i + 1]
                when {
                    isLatinWordChar(raw, prevBoundary, nextBoundary) -> {
                        if (wordStart < 0) wordStart = prevBoundary
                        wordEnd = nextBoundary
                    }
                    isConnectorChar(raw, prevBoundary, nextBoundary) -> {
                        // 连接符：若左边有 Latin/数字 run 且下一个 grapheme 是 Latin/数字 → 并入 run
                        val nextIsLatinWord =
                            i < lastGraphemeIndex && isLatinWordChar(raw, nextBoundary, boundaries[i + 2])
                        if (wordStart >= 0 && nextIsLatinWord) {
                            // 并入 run（连接符本身也算 run 的一部分）
                            wordEnd = nextBoundary
                        } else {
                            // 不并入：结束当前 word run
                            if (wordStart >= 0) {
                                onWordRun(wordStart, wordEnd)
                                wordStart = -1
                            }
                        }
                    }
                    else -> {
                        // 非西文 word 字符：结束当前 word run
                        if (wordStart >= 0) {
                            onWordRun(wordStart, wordEnd)
                            wordStart = -1
                        }
                    }
                }
            }
            // 文本结束：收尾当前 word run
            if (wordStart >= 0) onWordRun(wordStart, raw.length)
        }

        /**
         * Issue #717 评论 5743443030 修复2：判断 [start, end) 这段 grapheme 是否是连接符。
         *
         * 连接符：'（U+0027）、’（U+2019 RIGHT SINGLE QUOTATION MARK）、_（U+005F）。
         * 连字符 - 不处理（本身已有换行机会）。
         *
         * 单字符，BMP 内，用 raw[start] 安全。
         */
        private fun isConnectorChar(
            raw: CharSequence,
            start: Int,
            end: Int,
        ): Boolean {
            if (start >= end || start >= raw.length) return false
            val c = raw[start]
            return c == '\'' || c == '\u2019' || c == '_'
        }

        /**
         * 对一个西文 word run [wordStart, wordEnd) 计算内部 U+200B 插入点并追加到 [insertPoints]。
         *
         * Issue #717 评论 5742904417 修复3：去掉 longWordThreshold 检查，
         * 只要有 >= 2 个 grapheme boundary 就插入。用 [BreakIterator.getCharacterInstance]
         * 在 word run 内部找所有 grapheme boundary（除开头和结尾），每个 boundary 前插入 U+200B。
         * 如果 word run 只有 1 个 grapheme（wordLen == 1 个 grapheme），
         * next() 直接到 DONE，不插入，自然正确。
         */
        private fun addInsertPointsForWord(
            insertPoints: MutableList<Int>,
            raw: CharSequence,
            wordStart: Int,
            wordEnd: Int,
        ) {
            val wordLen = wordEnd - wordStart
            val wordIterator = BreakIterator.getCharacterInstance()
            wordIterator.setText(raw.subSequence(wordStart, wordEnd).toString())
            // 跳过开头 boundary（不在单词开头插入）
            var boundary = wordIterator.next()
            while (boundary != BreakIterator.DONE && boundary < wordLen) {
                // boundary 是相对于 wordStart 的偏移；不在单词末尾插入
                if (boundary < wordLen) {
                    insertPoints.add(wordStart + boundary)
                }
                boundary = wordIterator.next()
            }
        }

        /**
         * 评论 5742273757 修复2：判断 [start, end) 这段 grapheme 是否属于西文 word run。
         *
         * Issue #717 评论 5742904417 修复3：改用 [Character.UnicodeScript.LATIN] 判断
         * base code point 是否属于 LATIN script，数字单独允许，组合符号跟随所属 grapheme。
         *
         * Issue #717 评论 5743443030 修复2：改用 [Character.codePointAt] 按 code point 判断，
         * 覆盖补充平面 Latin Extended-F/G 字符（如 𝔸 U+1D538）和非 ASCII 数字（如 𝟏 U+1D7CF）。
         *
         * 包含：
         * - Unicode 数字（[Character.isDigit]，覆盖 ASCII 0-9 和非 ASCII 数字如数学粗体数字）
         * - Unicode LATIN script 字符（ASCII 字母、é、ñ、ü、ø、À-ÿ 及补充平面 Latin Extended-F/G）
         * - 组合附加符号 (U+0300..U+036F — combining diacritical marks)
         *
         * 不包含 CJK、空格、标点等。
         *
         * 注意：ASCII 数字 0-9 的 [Character.UnicodeScript.of] 返回 COMMON 而非 LATIN，
         * 所以数字必须单独用 [Character.isDigit] 判断，放在 LATIN 判断之前。
         */
        private fun isLatinWordChar(
            raw: CharSequence,
            start: Int,
            end: Int,
        ): Boolean {
            if (start >= end || start >= raw.length) return false
            val codePoint = Character.codePointAt(raw, start)
            // 数字（覆盖非 ASCII 数字，如数学粗体数字 U+1D7CF）
            if (Character.isDigit(codePoint)) return true
            // 按 Unicode script 判断 base code point 是否属于 LATIN
            // （覆盖 ASCII 字母和所有拉丁扩展，包括补充平面 Latin Extended-F/G）
            if (Character.UnicodeScript.of(codePoint) == Character.UnicodeScript.LATIN) return true
            // 组合附加符号 (U+0300..U+036F)：combining diacritical marks，跟随所属 grapheme
            if (codePoint in 0x0300..0x036F) return true
            return false
        }
    }
}
