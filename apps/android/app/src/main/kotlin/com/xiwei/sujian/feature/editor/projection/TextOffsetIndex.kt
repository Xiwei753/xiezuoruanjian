package com.xiwei.sujian.feature.editor.projection

/**
 * 增量 UTF-8/UTF-16 偏移索引，按段落维护 codepoint 边界与段落级前缀和。
 *
 * 设计意图（Issue #624 评论4）：
 * - 旧实现里 [DisplayTextProjection.identity] 每键都执行 `buildRealBoundaries(text)`，
 *   对整章做一次完整 UTF-8/UTF-16 边界扫描；[DisplayTextMirror.applyPatches] 又会
 *   反复创建 `AndroidTextIndexMap(this)`，等于在普通输入路径上跑两套整章索引。
 * - [TextOffsetIndex] 由 Mirror 唯一持有，普通 patch 只更新受影响段落，避免每键
 *   复制整章并重建整章边界。identity projections 长期引用本索引，不再每键
 *   `mirror.getText()` / `buildRealBoundaries()`。
 *
 * 段落定义：以 `\n` 分隔的文本段（含末尾 `\n`）。空文本有 1 个空段落。
 * 例如 `"abc\ndef\n"` → 段落 0 = `"abc\n"`，段落 1 = `"def\n"`；
 *      `"abc\ndef"`   → 段落 0 = `"abc\n"`，段落 1 = `"def"`（无尾换行）。
 *
 * 内部数据结构：
 * - [paragraphStartUtf8] / [paragraphStartUtf16]：段落 i 的起始偏移（前缀和），
 *   长度 = `paragraphCount + 1`，末尾哨兵 = 总长度。
 * - [paragraphCpUtf8] / [paragraphCpUtf16]：段落 i 内每个 codepoint 的边界
 *   （相对段落起点），长度 = 段落 codepoint 数 + 1（首元素 0，末尾哨兵 = 段落长度）。
 *
 * 复杂度：
 * - 全量重建 [rebuildFromText]：O(N)，N = 文本 codepoint 数。
 * - 增量更新 [onBufferReplaced]：O(M + K)，M = 受影响范围 codepoint 数，
 *   K = 后续段落数（前缀 shift）。普通单字符编辑 M、K 都很小。
 * - 查询 [utf8ToUtf16] / [utf16ToUtf8]：O(log P + log C)，P = 段落数，
 *   C = 段落内 codepoint 数。
 *
 * 线程约束：非线程安全；所有访问必须在 UI 线程。
 */
class TextOffsetIndex {
    /** 段落 i 的起始 UTF-8 字节偏移（前缀和），长度 = `paragraphCount + 1`。 */
    private var paragraphStartUtf8: IntArray = IntArray(2)

    /** 段落 i 的起始 UTF-16 偏移（前缀和），长度 = `paragraphCount + 1`。 */
    private var paragraphStartUtf16: IntArray = IntArray(2)

    /** 段落 i 内 codepoint 的 UTF-8 边界（相对段落起点），长度 = codepoint 数 + 1。 */
    private var paragraphCpUtf8: Array<IntArray> = arrayOf(IntArray(1))

    /** 段落 i 内 codepoint 的 UTF-16 边界（相对段落起点），长度 = codepoint 数 + 1。 */
    private var paragraphCpUtf16: Array<IntArray> = arrayOf(IntArray(1))

    /** 段落数量。空文本返回 1（一个空段落）。 */
    fun paragraphCount(): Int = paragraphCpUtf8.size

    /** 全文 UTF-8 字节数。 */
    fun utf8Length(): Int = paragraphStartUtf8[paragraphCount()]

    /** 全文 UTF-16 单元数。 */
    fun utf16Length(): Int = paragraphStartUtf16[paragraphCount()]

    /**
     * 全量重建索引。用于加载/快照路径，扫描整个 [text] 按 `\n` 分段构建每段 codepoint 边界。
     *
     * 复杂度 O(N)，N = [text] 的 codepoint 数。
     */
    fun rebuildFromText(text: String) {
        val paragraphs = scanParagraphs(text, 0, text.length)
        val count = paragraphs.size
        val newStartUtf8 = IntArray(count + 1)
        val newStartUtf16 = IntArray(count + 1)
        val newCpUtf8 = Array(count) { IntArray(1) }
        val newCpUtf16 = Array(count) { IntArray(1) }
        var accUtf8 = 0
        var accUtf16 = 0
        for (i in 0 until count) {
            newStartUtf8[i] = accUtf8
            newStartUtf16[i] = accUtf16
            newCpUtf8[i] = paragraphs[i].cpUtf8
            newCpUtf16[i] = paragraphs[i].cpUtf16
            accUtf8 += paragraphs[i].cpUtf8.last()
            accUtf16 += paragraphs[i].cpUtf16.last()
        }
        newStartUtf8[count] = accUtf8
        newStartUtf16[count] = accUtf16
        paragraphStartUtf8 = newStartUtf8
        paragraphStartUtf16 = newStartUtf16
        paragraphCpUtf8 = newCpUtf8
        paragraphCpUtf16 = newCpUtf16
    }

    /**
     * 增量更新索引。在 `buffer.replace(replaceStartUtf16, replaceEndUtf16, insertedText)`
     * **之后**调用——[buffer] 已是新内容。
     *
     * 只扫描受影响段落范围并替换，后续段落前缀 O(后续段落数) shift。普通单字符编辑
     * 只触及 1 个段落，避免整章扫描。
     *
     * 算法：
     * 1. 用旧索引找到 [replaceStartUtf16] 所在段落 [startParagraph] 和
     *    [replaceEndUtf16] 所在段落 [endParagraph]（基于旧 paragraphStartUtf16 二分）。
     * 2. 旧受影响范围在新 buffer 中的对应：
     *    - 起点 = `paragraphStartUtf16[startParagraph]`（不变）
     *    - 终点 = `paragraphStartUtf16[endParagraph + 1]`（旧值）+ delta，
     *      delta = `insertedText.length - (replaceEndUtf16 - replaceStartUtf16)`
     * 3. 扫描新 buffer 的 `[oldAffectedStartUtf16, newAffectedEndUtf16)` 范围，
     *    按 `\n` 分段，构建新段落的 codepoint 边界。
     * 4. 替换旧段落 `[startParagraph, endParagraph]` 为新段落，shift 后续段落前缀。
     *
     * @param replaceStartUtf16 旧 buffer 中被替换的起始 UTF-16 偏移
     * @param replaceEndUtf16   旧 buffer 中被替换的结束 UTF-16 偏移
     * @param insertedText      替换后插入的文本
     * @param buffer            新 buffer（已替换完毕）
     */
    fun onBufferReplaced(
        replaceStartUtf16: Int,
        replaceEndUtf16: Int,
        insertedText: String,
        buffer: CharSequence,
    ) {
        val oldCount = paragraphCount()
        val oldUtf16Len = paragraphStartUtf16[oldCount]
        val safeStart = replaceStartUtf16.coerceIn(0, oldUtf16Len)
        val safeEnd = replaceEndUtf16.coerceIn(safeStart, oldUtf16Len)

        val startParagraph = findParagraphForUtf16(safeStart)
        val endParagraph = findParagraphForUtf16(safeEnd)

        val delta = insertedText.length - (safeEnd - safeStart)

        val oldAffectedStartUtf16 = paragraphStartUtf16[startParagraph]
        val oldEndParagraphEndUtf16 = paragraphStartUtf16[endParagraph + 1]
        val newAffectedEndUtf16 = oldEndParagraphEndUtf16 + delta

        // 扫描新 buffer 的受影响范围，构建新段落 codepoint 边界。
        // 若受影响范围在新 buffer 中为空（删除整段且不插入），用空列表替换受影响段落，
        // 而非 scanParagraphs 的"空范围→1 个空段落"语义（那会多出空段落）。
        val newParagraphs =
            if (newAffectedEndUtf16 > oldAffectedStartUtf16) {
                scanParagraphs(buffer, oldAffectedStartUtf16, newAffectedEndUtf16)
            } else {
                emptyList()
            }
        replaceParagraphs(startParagraph, endParagraph, newParagraphs)
    }

    /**
     * UTF-8 字节偏移 → UTF-16 偏移。[byteOffset] 落在 codepoint 中间字节时 snap 到该
     * codepoint 起始（向下取整到 codepoint 边界）。
     *
     * - `byteOffset <= 0` 返回 0
     * - `byteOffset >= utf8Length()` 返回 `utf16Length()`
     *
     * 复杂度 O(log P + log C)。
     */
    fun utf8ToUtf16(byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        val count = paragraphCount()
        val totalUtf8 = paragraphStartUtf8[count]
        if (byteOffset >= totalUtf8) return paragraphStartUtf16[count]

        val paraIdx = findParagraphForUtf8(byteOffset)
        val paraStartUtf8 = paragraphStartUtf8[paraIdx]
        val paraStartUtf16 = paragraphStartUtf16[paraIdx]
        val localByteOffset = byteOffset - paraStartUtf8
        val cpUtf8 = paragraphCpUtf8[paraIdx]
        val cpUtf16 = paragraphCpUtf16[paraIdx]
        val cpIdx = binarySearchCeiling(cpUtf8, localByteOffset)
        return paraStartUtf16 + cpUtf16[cpIdx]
    }

    /**
     * UTF-16 偏移 → UTF-8 字节偏移。[utf16Offset] 落在 surrogate pair 中间时 snap 到该
     * codepoint 起始（向下取整到 codepoint 边界）。
     *
     * - `utf16Offset <= 0` 返回 0
     * - `utf16Offset >= utf16Length()` 返回 `utf8Length()`
     *
     * 复杂度 O(log P + log C)。
     */
    fun utf16ToUtf8(utf16Offset: Int): Int {
        if (utf16Offset <= 0) return 0
        val count = paragraphCount()
        val totalUtf16 = paragraphStartUtf16[count]
        if (utf16Offset >= totalUtf16) return paragraphStartUtf8[count]

        val paraIdx = findParagraphForUtf16(utf16Offset)
        val paraStartUtf8 = paragraphStartUtf8[paraIdx]
        val paraStartUtf16 = paragraphStartUtf16[paraIdx]
        val localUtf16Offset = utf16Offset - paraStartUtf16
        val cpUtf8 = paragraphCpUtf8[paraIdx]
        val cpUtf16 = paragraphCpUtf16[paraIdx]
        val cpIdx = binarySearchCeiling(cpUtf16, localUtf16Offset)
        return paraStartUtf8 + cpUtf8[cpIdx]
    }

    // ---- 内部辅助 ----

    /**
     * 二分 [paragraphStartUtf8] 找到 [byteOffset] 所在段落（最大的 i 使
     * `paragraphStartUtf8[i] <= byteOffset` 且 `i < paragraphCount`）。
     */
    private fun findParagraphForUtf8(byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        val count = paragraphCount()
        val last = paragraphStartUtf8[count]
        if (byteOffset >= last) return count - 1
        val idx = paragraphStartUtf8.binarySearch(byteOffset)
        return (if (idx >= 0) idx else -(idx + 1) - 1).coerceIn(0, count - 1)
    }

    /**
     * 二分 [paragraphStartUtf16] 找到 [utf16Offset] 所在段落（最大的 i 使
     * `paragraphStartUtf16[i] <= utf16Offset` 且 `i < paragraphCount`）。
     */
    private fun findParagraphForUtf16(utf16Offset: Int): Int {
        if (utf16Offset <= 0) return 0
        val count = paragraphCount()
        val last = paragraphStartUtf16[count]
        if (utf16Offset >= last) return count - 1
        val idx = paragraphStartUtf16.binarySearch(utf16Offset)
        return (if (idx >= 0) idx else -(idx + 1) - 1).coerceIn(0, count - 1)
    }

    /**
     * 返回最大的 i 使 `arr[i] <= value`（codepoint snap）。
     * [arr] 升序且 `arr[0] == 0`。
     */
    private fun binarySearchCeiling(
        arr: IntArray,
        value: Int,
    ): Int {
        if (value <= 0) return 0
        val lastIdx = arr.size - 1
        val last = arr[lastIdx]
        if (value >= last) return lastIdx
        val idx = arr.binarySearch(value)
        return (if (idx >= 0) idx else -(idx + 1) - 1).coerceIn(0, lastIdx)
    }

    /** codepoint 的 UTF-8 字节长度（1/2/3/4）。 */
    private fun utf8ByteLength(codePoint: Int): Int =
        when {
            codePoint <= 0x7F -> 1
            codePoint <= 0x7FF -> 2
            codePoint <= 0xFFFF -> 3
            else -> 4
        }

    /** 单段 codepoint 边界（相对段落起点）。`cpUtf8[0] == cpUtf16[0] == 0`。 */
    private data class ParagraphCpBoundaries(
        val cpUtf8: IntArray,
        val cpUtf16: IntArray,
    )

    /**
     * 扫描 [buffer] 的 `[startUtf16, endUtf16)` 范围，按 `\n` 分段，返回每段 codepoint 边界。
     *
     * - 范围不以 `\n` 结尾时最后一段是不完整段落（仍作为一段）。
     * - 空范围（`start >= end`）返回一个空段落（`cpUtf8 = cpUtf16 = [0]`）。
     * - 范围以 `\n` 结尾时不追加多余空段落。
     *
     * 用 `Character.codePointAt` 而非 Kotlin 扩展，确保对任意 [CharSequence]（含
     * `SpannableStringBuilder`）通用。
     */
    private fun scanParagraphs(
        buffer: CharSequence,
        startUtf16: Int,
        endUtf16: Int,
    ): List<ParagraphCpBoundaries> {
        if (startUtf16 >= endUtf16) {
            return listOf(ParagraphCpBoundaries(IntArray(1), IntArray(1)))
        }

        val result = mutableListOf<ParagraphCpBoundaries>()
        var curUtf8 = ArrayList<Int>()
        var curUtf16 = ArrayList<Int>()
        curUtf8.add(0)
        curUtf16.add(0)
        var bytePos = 0
        var utf16Pos = 0
        var i = startUtf16
        while (i < endUtf16) {
            val codePoint = Character.codePointAt(buffer, i)
            val cpByteLen = utf8ByteLength(codePoint)
            val cpUtf16Len = Character.charCount(codePoint)
            bytePos += cpByteLen
            utf16Pos += cpUtf16Len
            curUtf8.add(bytePos)
            curUtf16.add(utf16Pos)
            i += cpUtf16Len
            if (codePoint == '\n'.code) {
                // 段落以 \n 结束，提交并开新段。
                result.add(ParagraphCpBoundaries(curUtf8.toIntArray(), curUtf16.toIntArray()))
                curUtf8 = ArrayList<Int>()
                curUtf16 = ArrayList<Int>()
                curUtf8.add(0)
                curUtf16.add(0)
                bytePos = 0
                utf16Pos = 0
            }
        }
        // 处理最后一段：若末尾正好是 \n，curUtf8 已被重置为 [0]，不追加；
        // 否则 curUtf8 含未提交的 codepoint，追加为不完整段落。
        if (curUtf8.size > 1) {
            result.add(ParagraphCpBoundaries(curUtf8.toIntArray(), curUtf16.toIntArray()))
        }
        // 兜底：理论上不会触发（空范围已在前面返回），保留以防逻辑遗漏。
        if (result.isEmpty()) {
            result.add(ParagraphCpBoundaries(IntArray(1), IntArray(1)))
        }
        return result
    }

    /**
     * 替换旧段落 `[startParagraph, endParagraph]`（含端点）为 [newParagraphs]，
     * shift 后续段落前缀。
     *
     * - `[0, startParagraph)` 不变。
     * - `[startParagraph, startParagraph + newCount)` 填充新段落，起点前缀连续累加。
     * - 旧 `[endParagraph + 1, oldCount)` shift 到新 `[startParagraph + newCount, finalCount)`，
     *   内部 codepoint 边界不变，起点前缀加 delta（新受影响范围末尾 - 旧受影响范围末尾）。
     */
    private fun replaceParagraphs(
        startParagraph: Int,
        endParagraph: Int,
        newParagraphs: List<ParagraphCpBoundaries>,
    ) {
        val oldCount = paragraphCount()
        val replacedCount = endParagraph - startParagraph + 1
        val newCount = newParagraphs.size
        val deltaCount = newCount - replacedCount
        val finalCount = oldCount + deltaCount

        if (finalCount == 0) {
            // 删除所有段落后保留 1 个空段落，与 rebuildFromText("") 一致。
            paragraphStartUtf8 = IntArray(2)
            paragraphStartUtf16 = IntArray(2)
            paragraphCpUtf8 = arrayOf(IntArray(1))
            paragraphCpUtf16 = arrayOf(IntArray(1))
            return
        }

        val newStartUtf8 = IntArray(finalCount + 1)
        val newStartUtf16 = IntArray(finalCount + 1)
        val newCpUtf8 = Array(finalCount) { IntArray(1) }
        val newCpUtf16 = Array(finalCount) { IntArray(1) }

        // 1. 复制 [0, startParagraph) 不变。
        for (i in 0 until startParagraph) {
            newStartUtf8[i] = paragraphStartUtf8[i]
            newStartUtf16[i] = paragraphStartUtf16[i]
            newCpUtf8[i] = paragraphCpUtf8[i]
            newCpUtf16[i] = paragraphCpUtf16[i]
        }

        // 2. 填充新段落，起点前缀从 paragraphStart[startParagraph] 连续累加。
        var curStartUtf8 = paragraphStartUtf8[startParagraph]
        var curStartUtf16 = paragraphStartUtf16[startParagraph]
        for (j in 0 until newCount) {
            val newIdx = startParagraph + j
            newStartUtf8[newIdx] = curStartUtf8
            newStartUtf16[newIdx] = curStartUtf16
            newCpUtf8[newIdx] = newParagraphs[j].cpUtf8
            newCpUtf16[newIdx] = newParagraphs[j].cpUtf16
            curStartUtf8 += newParagraphs[j].cpUtf8.last()
            curStartUtf16 += newParagraphs[j].cpUtf16.last()
        }

        // 3. shift 后续段落：内部 codepoint 边界不变，起点前缀加 delta。
        val deltaUtf8 = curStartUtf8 - paragraphStartUtf8[endParagraph + 1]
        val deltaUtf16 = curStartUtf16 - paragraphStartUtf16[endParagraph + 1]
        for (i in (endParagraph + 1)..oldCount) {
            newStartUtf8[i + deltaCount] = paragraphStartUtf8[i] + deltaUtf8
            newStartUtf16[i + deltaCount] = paragraphStartUtf16[i] + deltaUtf16
        }
        for (i in (endParagraph + 1) until oldCount) {
            newCpUtf8[i + deltaCount] = paragraphCpUtf8[i]
            newCpUtf16[i + deltaCount] = paragraphCpUtf16[i]
        }

        paragraphStartUtf8 = newStartUtf8
        paragraphStartUtf16 = newStartUtf16
        paragraphCpUtf8 = newCpUtf8
        paragraphCpUtf16 = newCpUtf16
    }
}
