package com.xiwei.sujian.feature.editor.projection

import java.util.Random

/**
 * 增量 UTF-8/UTF-16 偏移索引，按段落维护 codepoint 边界。
 *
 * 设计意图（Issue #624 评论5）：
 * - 旧实现内部是平铺数组，[onBufferReplaced] 每次编辑都重新分配 `finalCount` 大小
 *   的数组并复制编辑点前后所有段落，热路径仍是 O(全文段落数 P)。
 * - 现改为按段落的隐式 Treap（平衡树）：每个节点代表一个段落，保存本段 codepoint
 *   UTF-8/UTF-16 边界及子树段落/字节/单元汇总。普通输入只 split/merge 受影响段落
 *   邻域，复杂度 O(受影响段落 + 树高)，不再分配整章数组或 shift 全文前缀。
 *
 * 段落定义：以 `\n` 分隔的文本段（含末尾 `\n`）。空文本有 1 个空段落。
 * 例如 `"abc\ndef\n"` → 段落 0 = `"abc\n"`，段落 1 = `"def\n"`；
 *      `"abc\ndef"`   → 段落 0 = `"abc\n"`，段落 1 = `"def"`（无尾换行）。
 *
 * 内部数据结构（隐式 Treap）：
 * - 隐式键 = 段落在中序遍历中的位置（段落索引）。
 * - [ParagraphNode] 保存本段 [ParagraphNode.cpUtf8] / [ParagraphNode.cpUtf16] 边界
 *   及子树 [ParagraphNode.subtreeParagraphs] / [ParagraphNode.subtreeUtf8] /
 *   [ParagraphNode.subtreeUtf16] 汇总。
 * - [root] 永远非 null（至少 1 个空段落节点）。
 *
 * 复杂度：
 * - 全量重建 [rebuildFromText]：O(N)，N = 文本 codepoint 数。
 * - 增量更新 [onBufferReplaced]：O(M + log P)，M = 受影响范围 codepoint 数，
 *   P = 段落数。普通单字符编辑 M 很小，整体 O(log P)。
 * - 查询 [utf8ToUtf16] / [utf16ToUtf8]：O(log P + log C)，P = 段落数，
 *   C = 段落内 codepoint 数。
 *
 * 线程约束：非线程安全；所有访问必须在 UI 线程。
 */
class TextOffsetIndex {
    /** 随机堆优先级来源（固定种子，单线程无需同步）。 */
    private val random: Random = Random(0x624L)

    /** 隐式 Treap 根节点，永远非 null（至少 1 个空段落）。 */
    private var root: ParagraphNode? = null

    init {
        root = newEmptyParagraphNode()
    }

    /** 段落数量。空文本返回 1（一个空段落）。 */
    fun paragraphCount(): Int = root?.subtreeParagraphs ?: 0

    /** 全文 UTF-8 字节数。 */
    fun utf8Length(): Int = root?.subtreeUtf8 ?: 0

    /** 全文 UTF-16 单元数。 */
    fun utf16Length(): Int = root?.subtreeUtf16 ?: 0

    /**
     * 全量重建索引。用于加载/快照路径，扫描整个 [text] 按 `\n` 分段构建每段 codepoint 边界，
     * 再分治构建隐式 Treap。
     *
     * 复杂度 O(N)，N = [text] 的 codepoint 数。
     */
    fun rebuildFromText(text: String) {
        val paragraphs = scanParagraphs(text, 0, text.length)
        val nodes = ArrayList<ParagraphNode>(paragraphs.size)
        for (p in paragraphs) {
            nodes.add(ParagraphNode(p.cpUtf8, p.cpUtf16, nextPriority()))
        }
        root = buildTreapFromList(nodes, 0, nodes.size - 1) ?: newEmptyParagraphNode()
    }

    /**
     * 增量更新索引。在 `buffer.replace(replaceStartUtf16, replaceEndUtf16, insertedText)`
     * **之后**调用——[buffer] 已是新内容。
     *
     * 只扫描受影响段落范围并替换，前后段落通过隐式 Treap 的 split/merge 重连指针，
     * 不分配整章数组、不 shift 全文前缀。普通单字符编辑只触及 1 个段落。
     *
     * 算法：
     * 1. 用旧索引按累计 UTF-16 下行定位 [replaceStartUtf16] 所在段落 [startPara] 和
     *    [replaceEndUtf16] 所在段落 [endPara]。
     * 2. 旧受影响范围在新 buffer 中的对应：
     *    - 起点 = [startPara] 段落起点累计 UTF-16（不变）
     *    - 终点 = [endPara] 段落末尾累计 UTF-16（旧值）+ delta，
     *      delta = `insertedText.length - (replaceEndUtf16 - replaceStartUtf16)`
     * 3. 扫描新 buffer 的 `[oldAffectedStartUtf16, newAffectedEndUtf16)` 范围，
     *    按 `\n` 分段，构建新段落 codepoint 边界与新节点。
     * 4. split 出 prefix / affected / suffix 三段，丢弃 affected，
     *    merge(prefix, newRoot, suffix) 重连。若结果为空（删除所有段落）保留 1 个空段落。
     *
     * 复杂度 O(M + log P)，M = 受影响范围 codepoint 数，P = 段落数。
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
        val r = root ?: return
        val oldUtf16Len = r.subtreeUtf16
        val safeStart = replaceStartUtf16.coerceIn(0, oldUtf16Len)
        val safeEnd = replaceEndUtf16.coerceIn(safeStart, oldUtf16Len)

        val startLookup = findParagraphByUtf16(r, safeStart)
        val endLookup = findParagraphByUtf16(r, safeEnd)
        val startPara = startLookup.paragraphIndex
        val endPara = endLookup.paragraphIndex
        val oldAffectedStartUtf16 = startLookup.cumUtf16Start
        val endNode = endLookup.node ?: return
        val oldEndParagraphEndUtf16 = endLookup.cumUtf16Start + endNode.cpUtf16.last()

        val delta = insertedText.length - (safeEnd - safeStart)
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

        val affectedCount = endPara - startPara + 1

        val (prefix, rest) = split(r, startPara)
        val (_, suffix) = split(rest, affectedCount)

        val newNodes = ArrayList<ParagraphNode>(newParagraphs.size)
        for (p in newParagraphs) {
            newNodes.add(ParagraphNode(p.cpUtf8, p.cpUtf16, nextPriority()))
        }
        val newRoot = buildTreapFromList(newNodes, 0, newNodes.size - 1)

        val merged = merge(merge(prefix, newRoot), suffix)
        root = merged ?: newEmptyParagraphNode()
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
        val r = root ?: return 0
        val totalUtf8 = r.subtreeUtf8
        if (byteOffset >= totalUtf8) return r.subtreeUtf16

        val lookup = findParagraphByUtf8(r, byteOffset)
        val node = lookup.node ?: return 0
        val localByteOffset = byteOffset - lookup.cumUtf8Start
        val cpIdx = binarySearchCeiling(node.cpUtf8, localByteOffset)
        return lookup.cumUtf16Start + node.cpUtf16[cpIdx]
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
        val r = root ?: return 0
        val totalUtf16 = r.subtreeUtf16
        if (utf16Offset >= totalUtf16) return r.subtreeUtf8

        val lookup = findParagraphByUtf16(r, utf16Offset)
        val node = lookup.node ?: return 0
        val localUtf16Offset = utf16Offset - lookup.cumUtf16Start
        val cpIdx = binarySearchCeiling(node.cpUtf16, localUtf16Offset)
        return lookup.cumUtf8Start + node.cpUtf8[cpIdx]
    }

    // ---- 隐式 Treap 节点与操作 ----

    /**
     * 隐式 Treap 节点，代表一个段落。
     *
     * - [cpUtf8] / [cpUtf16]：本段 codepoint UTF-8/UTF-16 边界（相对段起点），
     *   首元素 0，末尾 = 段落长度。空段落为 `[0]`。
     * - [priority]：随机堆优先级，维护 Treap 堆性质。
     * - [subtreeParagraphs] / [subtreeUtf8] / [subtreeUtf16]：子树汇总，
     *   由 [updateNode] 重算。
     */
    private class ParagraphNode(
        val cpUtf8: IntArray,
        val cpUtf16: IntArray,
        val priority: Int,
    ) {
        var left: ParagraphNode? = null
        var right: ParagraphNode? = null
        var subtreeParagraphs: Int = 1
        var subtreeUtf8: Int = cpUtf8.last()
        var subtreeUtf16: Int = cpUtf16.last()
    }

    /** 段落定位结果：段落索引、起点累计 UTF-16/UTF-8 偏移、节点引用。 */
    private data class ParagraphLookup(
        val paragraphIndex: Int,
        val cumUtf16Start: Int,
        val cumUtf8Start: Int,
        val node: ParagraphNode?,
    )

    /** 重算 [node] 的子树汇总（假设左右子树汇总已正确）。 */
    private fun updateNode(node: ParagraphNode) {
        val l = node.left
        val r = node.right
        node.subtreeParagraphs = 1 + (l?.subtreeParagraphs ?: 0) + (r?.subtreeParagraphs ?: 0)
        node.subtreeUtf8 = node.cpUtf8.last() + (l?.subtreeUtf8 ?: 0) + (r?.subtreeUtf8 ?: 0)
        node.subtreeUtf16 = node.cpUtf16.last() + (l?.subtreeUtf16 ?: 0) + (r?.subtreeUtf16 ?: 0)
    }

    /**
     * 隐式 Treap split：[root] 的前 [k] 个段落归 L，剩余归 R。
     *
     * 边界：k <= 0 → (null, root)；k >= total → (root, null)。
     * 只重连指针 + updateNode，不分配新节点。
     */
    private fun split(
        root: ParagraphNode?,
        k: Int,
    ): Pair<ParagraphNode?, ParagraphNode?> {
        if (root == null) {
            return null to null
        }
        val leftSize = root.left?.subtreeParagraphs ?: 0
        return if (k <= leftSize) {
            val (ll, lr) = split(root.left, k)
            root.left = lr
            updateNode(root)
            ll to root
        } else {
            // k >= leftSize + 1：root 归 L，递归 split 右子树取 (k - leftSize - 1) 个到 L。
            val (rl, rr) = split(root.right, k - leftSize - 1)
            root.right = rl
            updateNode(root)
            root to rr
        }
    }

    /**
     * 隐式 Treap merge：合并 [left] 和 [right]，要求 left 的所有段落索引 < right 的。
     * 按 [ParagraphNode.priority] 维护堆性质。只重连指针 + updateNode，不分配新节点。
     */
    private fun merge(
        left: ParagraphNode?,
        right: ParagraphNode?,
    ): ParagraphNode? {
        if (left == null) return right
        if (right == null) return left
        return if (left.priority > right.priority) {
            left.right = merge(left.right, right)
            updateNode(left)
            left
        } else {
            right.left = merge(left, right.left)
            updateNode(right)
            right
        }
    }

    /** 生成下一个随机堆优先级。 */
    private fun nextPriority(): Int = random.nextInt()

    /** 创建一个空段落节点。 */
    private fun newEmptyParagraphNode(): ParagraphNode = ParagraphNode(IntArray(1), IntArray(1), nextPriority())

    /**
     * 分治构建隐式 Treap：`build(lo, hi) = merge(build(lo, mid), build(mid+1, hi))`，
     * 空区间返回 null。O(n) 期望，避免退化为链。
     */
    private fun buildTreapFromList(
        nodes: List<ParagraphNode>,
        lo: Int,
        hi: Int,
    ): ParagraphNode? {
        if (lo > hi) return null
        if (lo == hi) return nodes[lo]
        val mid = (lo + hi) ushr 1
        return merge(
            buildTreapFromList(nodes, lo, mid),
            buildTreapFromList(nodes, mid + 1, hi),
        )
    }

    /**
     * 按累计 UTF-16 下行定位包含 [utf16Offset] 的段落。
     *
     * 返回 (段落索引, 段落起点累计 UTF-16, 段落起点累计 UTF-8, 节点)。
     * - `utf16Offset <= 0` 返回最左段落 (0, 0, 0, leftmost)。
     * - `utf16Offset >= 总长` 返回最右段落。
     *
     * 不分配，O(树高)。
     */
    private fun findParagraphByUtf16(
        root: ParagraphNode,
        utf16Offset: Int,
    ): ParagraphLookup {
        if (utf16Offset <= 0) {
            var n = root
            while (true) {
                val l = n.left ?: break
                n = l
            }
            return ParagraphLookup(0, 0, 0, n)
        }
        var node: ParagraphNode = root
        var idx = 0
        var acc16 = 0
        var acc8 = 0
        while (true) {
            val left = node.left
            val ls = left?.subtreeParagraphs ?: 0
            val l16 = left?.subtreeUtf16 ?: 0
            val l8 = left?.subtreeUtf8 ?: 0
            val p16 = node.cpUtf16.last()
            val p8 = node.cpUtf8.last()
            if (utf16Offset < acc16 + l16) {
                // l16 > 0 蕴含 left != null；防御性兜底。
                if (left == null) {
                    return ParagraphLookup(idx + ls, acc16 + l16, acc8 + l8, node)
                }
                node = left
            } else if (utf16Offset < acc16 + l16 + p16) {
                return ParagraphLookup(idx + ls, acc16 + l16, acc8 + l8, node)
            } else {
                acc16 += l16 + p16
                acc8 += l8 + p8
                idx += ls + 1
                val right = node.right
                if (right == null) {
                    // 偏移 >= 总长，返回当前最后一个段落。
                    return ParagraphLookup(idx - 1, acc16 - p16, acc8 - p8, node)
                }
                node = right
            }
        }
    }

    /**
     * 按累计 UTF-8 下行定位包含 [byteOffset] 的段落。对称于 [findParagraphByUtf16]。
     */
    private fun findParagraphByUtf8(
        root: ParagraphNode,
        byteOffset: Int,
    ): ParagraphLookup {
        if (byteOffset <= 0) {
            var n = root
            while (true) {
                val l = n.left ?: break
                n = l
            }
            return ParagraphLookup(0, 0, 0, n)
        }
        var node: ParagraphNode = root
        var idx = 0
        var acc16 = 0
        var acc8 = 0
        while (true) {
            val left = node.left
            val ls = left?.subtreeParagraphs ?: 0
            val l16 = left?.subtreeUtf16 ?: 0
            val l8 = left?.subtreeUtf8 ?: 0
            val p16 = node.cpUtf16.last()
            val p8 = node.cpUtf8.last()
            if (byteOffset < acc8 + l8) {
                if (left == null) {
                    return ParagraphLookup(idx + ls, acc16 + l16, acc8 + l8, node)
                }
                node = left
            } else if (byteOffset < acc8 + l8 + p8) {
                return ParagraphLookup(idx + ls, acc16 + l16, acc8 + l8, node)
            } else {
                acc16 += l16 + p16
                acc8 += l8 + p8
                idx += ls + 1
                val right = node.right
                if (right == null) {
                    return ParagraphLookup(idx - 1, acc16 - p16, acc8 - p8, node)
                }
                node = right
            }
        }
    }

    // ---- 段落扫描与 codepoint 边界 ----

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
}
