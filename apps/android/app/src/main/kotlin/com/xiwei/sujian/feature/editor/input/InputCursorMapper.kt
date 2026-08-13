package com.xiwei.sujian.feature.editor.input

import com.xiwei.sujian.feature.editor.projection.TextOffsetIndex

/**
 * #624 评论7：InputConnection.newCursorPosition 计算 — 只消费 [TextOffsetIndex] +
 * 替换区间 + 插入文本，不构造整篇 virtualText。
 *
 * 旧正文查现有 index，replacement 只在局部扫描 codepoint，替换区后按 suffix 位移换算。
 * 复杂度 O(log P + replacement 长度)，不再随整章字符数线性增长。
 */
internal object InputCursorMapper {
    /**
     * 计算替换后的目标选区（UTF-8 字节偏移，collapsed）。
     *
     * [index] 是替换**前** committed text 的偏移索引。
     * [replaceStartUtf8]/[replaceEndUtf8] 是旧 committed text 的 UTF-8 半开区间。
     * [replacementText] 是插入文本。[newCursorPosition] 遵循 Android InputConnection 约定：
     * >0 相对 replacement end - 1；<=0 相对 replacement start。
     *
     * 返回 Pair(targetUtf8, targetUtf8)。
     */
    fun computeResultingSelectionUtf8(
        index: TextOffsetIndex,
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        replacementText: String,
        newCursorPosition: Int,
    ): Pair<Int, Int> {
        val totalCommittedUtf8 = index.utf8Length()
        val totalCommittedUtf16 = index.utf16Length()
        val safeStart = replaceStartUtf8.coerceIn(0, totalCommittedUtf8)
        val safeEnd = replaceEndUtf8.coerceIn(safeStart, totalCommittedUtf8)

        val replaceStartUtf16 = index.utf8ToUtf16(safeStart)
        val replaceEndUtf16 = index.utf8ToUtf16(safeEnd)
        val replacementUtf16Len = replacementText.length
        val replacementUtf8Len = utf8ByteLengthOf(replacementText)
        val newReplaceEndUtf16 = replaceStartUtf16 + replacementUtf16Len
        val totalUtf16 = replaceStartUtf16 + replacementUtf16Len + (totalCommittedUtf16 - replaceEndUtf16)

        val targetUtf16: Int = if (newCursorPosition > 0) {
            (newReplaceEndUtf16 + newCursorPosition - 1).coerceIn(0, totalUtf16)
        } else {
            (replaceStartUtf16 + newCursorPosition).coerceIn(0, totalUtf16)
        }

        // virtual text = old[0,safeStart) + replacement + old[safeEnd,end)；按三段映射，snap 到 codepoint 边界
        val targetUtf8 = when {
            // 旧前缀
            targetUtf16 <= replaceStartUtf16 -> index.utf16ToUtf8(targetUtf16)
            // replacement 内
            targetUtf16 < newReplaceEndUtf16 -> {
                val localUtf16 = targetUtf16 - replaceStartUtf16
                safeStart + utf16ToUtf8Local(replacementText, localUtf16)
            }
            // 旧后缀
            else -> {
                val suffixUtf16 = targetUtf16 - newReplaceEndUtf16
                val oldUtf16 = replaceEndUtf16 + suffixUtf16
                val oldUtf8 = index.utf16ToUtf8(oldUtf16)
                safeStart + replacementUtf8Len + (oldUtf8 - safeEnd)
            }
        }
        return Pair(targetUtf8, targetUtf8)
    }

    /** replacementText 内 UTF-16 偏移 → UTF-8 偏移，snap 到 codepoint 起始（向下取整）。 */
    private fun utf16ToUtf8Local(text: String, utf16Offset: Int): Int {
        if (utf16Offset <= 0) return 0
        var byteLen = 0
        var i = 0
        while (i < text.length) {
            if (i >= utf16Offset) break
            val cp = text.codePointAt(i)
            val charLen = Character.charCount(cp)
            if (i + charLen > utf16Offset) break // snap 到本 codepoint 起始
            byteLen += utf8ByteLength(cp)
            i += charLen
        }
        return byteLen
    }

    private fun utf8ByteLengthOf(s: String): Int {
        var len = 0
        var i = 0
        while (i < s.length) {
            val cp = s.codePointAt(i)
            len += utf8ByteLength(cp)
            i += Character.charCount(cp)
        }
        return len
    }

    private fun utf8ByteLength(codePoint: Int): Int = when {
        codePoint <= 0x7F -> 1
        codePoint <= 0x7FF -> 2
        codePoint <= 0xFFFF -> 3
        else -> 4
    }
}
