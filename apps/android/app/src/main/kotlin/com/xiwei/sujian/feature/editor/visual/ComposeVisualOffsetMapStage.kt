package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange

/**
 * visual offset map 的 stage 级合成与坐标映射。
 *
 * 原先和 [ComposeVisualRebase] 的 retained moves、切片切分挤在同一个 1372 行文件里，
 * 按依赖关系拆出来：这一层只依赖纯数据（[TextRange] / [VisualOffsetMapEntry] /
 * [EditorEditFact] / [VisualReplaceBounds]），是整条 visual rebase 链的叶节点。
 *
 * 职责：
 * - 把 chain 里每笔 fact 的 offsetMap 合成成一条 T0->Tn 的 map（[composeOffsetMapChain]）
 * - 把 units 沿 stage map 合成到最终坐标或回退到最初坐标（[composeNewUnitsToFinalStages]
 *   / [composeOldUnitsToBaseStages]）
 * - 单 range / 多 range 的正反向映射（[mapRangesForwardThroughOffsetMap] 等）
 * - 光标 offset 沿 chain 映射（[mapCursorOffsetThroughChain]）
 */
internal object ComposeVisualOffsetMapStage {
    /**
     * #684 评论 5669048233 Bug2 修复 + #694 评论 5691696678 问题3：通用 stage-map 版本 —
     * 把每个 stage 的 newUnits 沿后续 stage offset map 映射到最终 Tn 坐标。
     *
     * 不依赖 [EditorVisualIntent]，接收纯 [List]<[TextRange]> 和 [List]<[VisualOffsetMapEntry]?>，
     * 供 [ComposeVisualPatchBatch.compose] 合成本地输入 patch 的 insertedUnits（保留吐字顺序）。
     *
     * 算法和 [composeNewAnimationUnitsToFinal] 一致：每笔的 units 沿后续 stage offset map
     * 映射到最终 Tn（用 [mapRangesForwardThroughOffsetMap]），null offset map 跳过该 stage 映射
     * （和 `entries == null -> continue` 同语义），空 entries 清空 units。最后 [deduplicateRanges]。
     *
     * @param perStageNewUnits 每个 stage 的新动画 units（T_i 坐标）。
     * @param perStageOffsetMaps 每个 stage 的 offset map（T_i→T_{i+1}）；null 表示该 stage 无 offset map。
     * @return 合成到最终 Tn 坐标的 units 列表（去重保序）。
     */
    fun composeNewUnitsToFinalStages(
        perStageNewUnits: List<List<TextRange>>,
        perStageOffsetMaps: List<List<VisualOffsetMapEntry>?>,
    ): List<TextRange> {
        val n = perStageNewUnits.size
        if (n == 0) return emptyList()
        val result = mutableListOf<TextRange>()
        for (i in 0 until n) {
            var units: List<TextRange> = perStageNewUnits[i]
            mapForwardLoop@ for (j in (i + 1) until n) {
                val entries = perStageOffsetMaps[j]
                if (entries == null) continue@mapForwardLoop
                if (entries.isEmpty()) {
                    units = emptyList()
                    break@mapForwardLoop
                }
                units = mapRangesForwardThroughOffsetMap(units, entries)
            }
            result.addAll(units)
        }
        return deduplicateRanges(result)
    }

    /**
     * #684 评论 5669048233 Bug2 修复 + #694 评论 5691696678 问题3：通用 stage-map 版本 —
     * 把每个 stage 的 oldUnits 沿前面 stage offset map 映射回最初 T0 坐标。
     *
     * 不依赖 [EditorVisualIntent]，供 [ComposeVisualPatchBatch.compose] 合成 deletedUnits。
     *
     * 算法和 [composeOldAnimationUnitsToBase] 一致：每笔的 units 沿前面 stage offset map
     * 映射回最初 T0（用 [mapRangesBackwardThroughOffsetMap]），null offset map 跳过该 stage 映射，
     * 空 entries 清空 units。最后 [deduplicateRanges]。
     *
     * @param perStageOldUnits 每个 stage 的旧动画 units（T_{i+1} 坐标）。
     * @param perStageOffsetMaps 每个 stage 的 offset map（T_i→T_{i+1}）；null 表示该 stage 无 offset map。
     * @return 合成回最初 T0 坐标的 units 列表（去重保序）。
     */
    fun composeOldUnitsToBaseStages(
        perStageOldUnits: List<List<TextRange>>,
        perStageOffsetMaps: List<List<VisualOffsetMapEntry>?>,
    ): List<TextRange> {
        val n = perStageOldUnits.size
        if (n == 0) return emptyList()
        val result = mutableListOf<TextRange>()
        for (i in 0 until n) {
            var units: List<TextRange> = perStageOldUnits[i]
            mapBackwardLoop@ for (j in (i - 1) downTo 0) {
                val entries = perStageOffsetMaps[j]
                if (entries == null) continue@mapBackwardLoop
                if (entries.isEmpty()) {
                    units = emptyList()
                    break@mapBackwardLoop
                }
                units = mapRangesBackwardThroughOffsetMap(units, entries)
            }
            result.addAll(units)
        }
        return deduplicateRanges(result)
    }

    /**
     * #684 评论 5669048233 Bug2 修复：把 chain 中每笔 fact 的 newAnimationUnits
     * 合成到最终 Tn 坐标。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun composeNewAnimationUnitsToFinal(chain: List<EditorEditFact>): List<TextRange> =
        composeNewUnitsToFinalStages(
            perStageNewUnits = chain.map { it.newAnimationUnits },
            perStageOffsetMaps = chain.map { it.offsetMap?.entries },
        )

    /**
     * #684 评论 5669048233 Bug2 修复：把 chain 中每笔 fact 的 oldAnimationUnits
     * 合成回最初 T0 坐标。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun composeOldAnimationUnitsToBase(chain: List<EditorEditFact>): List<TextRange> =
        composeOldUnitsToBaseStages(
            perStageOldUnits = chain.map { it.oldAnimationUnits },
            perStageOffsetMaps = chain.map { it.offsetMap?.entries },
        )

    /**
     * #684 评论 5673811415：把某笔 fact 的 cursor offset 沿后续 offset maps 映射到最终 Tn 坐标。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun mapCursorOffsetThroughChain(
        chain: List<EditorEditFact>,
        factIndex: Int,
        offset: Int,
    ): Int? {
        var currentOffset = offset
        for (j in (factIndex + 1) until chain.size) {
            val fact = chain[j]
            val mapped = mapCaretThroughFact(fact, currentOffset)
            if (mapped == null) {
                return null
            }
            currentOffset = mapped
        }
        return currentOffset
    }

    private fun mapCaretThroughFact(
        fact: EditorEditFact,
        offset: Int,
    ): Int? {
        val replaceBounds = fact.replaceBounds
        if (replaceBounds != null) {
            return mapCaretThroughReplaceBounds(replaceBounds, offset)
        }

        val entries = fact.offsetMap?.entries
        if (entries == null) return offset
        if (entries.isEmpty()) {
            return null
        }
        return mapCaretThroughOffsetMapEntries(entries, offset)
    }

    private fun mapCaretThroughReplaceBounds(
        rb: VisualReplaceBounds,
        offset: Int,
    ): Int? {
        return when {
            offset < rb.oldStart -> offset
            offset == rb.oldStart -> rb.newStart
            offset < rb.oldEnd -> null
            offset == rb.oldEnd -> {
                if (rb.oldStart < rb.oldEnd) rb.newEnd else rb.newStart
            }
            else -> offset + (rb.newEnd - rb.oldEnd)
        }
    }

    private fun mapCaretThroughOffsetMapEntries(
        entries: List<VisualOffsetMapEntry>,
        offset: Int,
    ): Int? {
        val sorted = entries.sortedBy { it.oldStart }

        for (entry in sorted) {
            val oldEnd = entry.oldStart + entry.length
            if (offset in entry.oldStart..oldEnd) {
                return when {
                    offset == entry.oldStart -> entry.newStart
                    offset == oldEnd -> entry.newStart + entry.length
                    else -> entry.newStart + (offset - entry.oldStart)
                }
            }
        }

        val firstOldStart = sorted.first().oldStart
        if (offset < firstOldStart) {
            return offset
        }

        val lastEntry = sorted.last()
        val lastOldEnd = lastEntry.oldStart + lastEntry.length
        if (offset > lastOldEnd) {
            val delta = (lastEntry.newStart + lastEntry.length) - lastOldEnd
            return offset + delta
        }

        return null
    }

    /**
     * 把 ranges 沿 offsetMap entries 的 old→new 方向映射。
     *
     * #694 评论 5691696678 问题3：internal 可见性 — 供 [composeNewUnitsToFinalStages] 调用。
     */
    internal fun mapRangesForwardThroughOffsetMap(
        ranges: List<TextRange>,
        entries: List<VisualOffsetMapEntry>,
    ): List<TextRange> {
        val result = mutableListOf<TextRange>()
        for (unit in ranges) {
            if (unit.start >= unit.end) continue
            for (entry in entries) {
                val oldStart = entry.oldStart
                val oldEnd = entry.oldStart + entry.length
                val overlapStart = maxOf(unit.start, oldStart)
                val overlapEnd = minOf(unit.end, oldEnd)
                if (overlapStart >= overlapEnd) continue
                val newStart = entry.newStart + (overlapStart - oldStart)
                val newEnd = entry.newStart + (overlapEnd - oldStart)
                result.add(TextRange(newStart, newEnd))
            }
        }
        return result
    }

    /**
     * 把 ranges 沿 offsetMap entries 的 new→old 方向（逆映射）映射。
     *
     * #694 评论 5691696678 问题3：internal 可见性 — 供 [composeOldUnitsToBaseStages] 调用。
     */
    internal fun mapRangesBackwardThroughOffsetMap(
        ranges: List<TextRange>,
        entries: List<VisualOffsetMapEntry>,
    ): List<TextRange> {
        val result = mutableListOf<TextRange>()
        for (unit in ranges) {
            if (unit.start >= unit.end) continue
            for (entry in entries) {
                val newStart = entry.newStart
                val newEnd = entry.newStart + entry.length
                val overlapStart = maxOf(unit.start, newStart)
                val overlapEnd = minOf(unit.end, newEnd)
                if (overlapStart >= overlapEnd) continue
                val oldStart = entry.oldStart + (overlapStart - newStart)
                val oldEnd = entry.oldStart + (overlapEnd - newStart)
                result.add(TextRange(oldStart, oldEnd))
            }
        }
        return result
    }

    private fun deduplicateRanges(ranges: List<TextRange>): List<TextRange> {
        val seen = LinkedHashSet<Pair<Int, Int>>()
        val result = mutableListOf<TextRange>()
        for (range in ranges) {
            val key = range.start to range.end
            if (seen.add(key)) {
                result.add(range)
            }
        }
        return result
    }

    /**
     * #689 评论 5676120929 问题3：获取 fact 的 entries — 优先用 offsetMap，没有则根据
     * replaceBounds + expectedOldText/expectedNewText 生成 fallback survival map。
     *
     * Issue #735 评论 5771063665：参数从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun entriesForFact(fact: EditorEditFact): List<VisualOffsetMapEntry> {
        fact.offsetMap?.entries?.let { return it }
        return buildFallbackEntriesFromReplaceBounds(
            fact.replaceBounds,
            fact.expectedOldText.length,
            fact.expectedNewText.length,
        )
    }

    /**
     * 根据 replaceBounds + oldLen + newLen
     * 生成 fallback survival map：
     * - replace 前面的前缀：old [0, oldStart) -> new [0, newStart)
     * - replace 后面的后缀：old [oldEnd, oldLen) -> new [newEnd, newLen)
     * - 被替换的中间区域没有 entry（被编辑/删除，不存活）。
     *
     * Issue #735 评论 5771063665：不再接收 [EditorVisualIntent]，改为接收纯数据。
     */
    private fun buildFallbackEntriesFromReplaceBounds(
        replaceBounds: VisualReplaceBounds?,
        oldLen: Int,
        newLen: Int,
    ): List<VisualOffsetMapEntry> {
        if (replaceBounds == null || oldLen == 0 || newLen == 0) return emptyList()

        val entries = mutableListOf<VisualOffsetMapEntry>()

        // 前缀：old [0, oldStart) -> new [0, newStart)
        if (replaceBounds.oldStart > 0 && replaceBounds.newStart > 0) {
            val prefixLen = minOf(replaceBounds.oldStart, replaceBounds.newStart)
            if (prefixLen > 0) {
                entries.add(
                    VisualOffsetMapEntry(
                        oldStart = 0,
                        newStart = 0,
                        length = prefixLen,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
        }

        // 后缀：old [oldEnd, oldLen) -> new [newEnd, newLen)
        val oldSuffixStart = replaceBounds.oldEnd
        val newSuffixStart = replaceBounds.newEnd
        if (oldSuffixStart < oldLen && newSuffixStart < newLen) {
            val suffixLen = minOf(oldLen - oldSuffixStart, newLen - newSuffixStart)
            if (suffixLen > 0) {
                entries.add(
                    VisualOffsetMapEntry(
                        oldStart = oldSuffixStart,
                        newStart = newSuffixStart,
                        length = suffixLen,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
        }

        return entries
    }

    /**
     * #644 评论 #684：合成整条 offset map chain。
     *
     * #689 评论 5676120929 问题3：不再因某一笔 offsetMap == null 就返回 null，
     * 而是每笔都用 [entriesForFact] 拿 entries（优先 offsetMap，没有则从 replaceBounds 生成 fallback），
     * 保证等长替换时被替换区域不会被错当成存活。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun composeOffsetMapChain(chain: List<EditorEditFact>): List<VisualOffsetMapEntry>? {
        if (chain.isEmpty()) return null

        val initialOldLen = chain.first().expectedOldText.length
        var acc: List<AccSegment> =
            listOf(
                AccSegment(
                    oldStart = 0,
                    newStart = 0,
                    length = initialOldLen,
                    kind = VisualOffsetMapKind.IDENTITY,
                ),
            )

        for (fact in chain) {
            val entries = entriesForFact(fact)
            val stage = buildStageSegments(entries)
            acc = composeStage(acc, stage)
        }

        return acc.map {
            VisualOffsetMapEntry(
                oldStart = it.oldStart,
                newStart = it.newStart,
                length = it.length,
                kind = it.kind,
            )
        }
    }

    private fun buildStageSegments(entries: List<VisualOffsetMapEntry>): List<StageSegment> {
        return entries
            .sortedBy { it.oldStart }
            .map { e ->
                StageSegment(
                    oldStart = e.oldStart,
                    newStart = e.newStart,
                    length = e.length,
                    kind = e.kind,
                )
            }
    }

    private fun composeStage(
        acc: List<AccSegment>,
        stage: List<StageSegment>,
    ): List<AccSegment> {
        val result = mutableListOf<AccSegment>()
        for (a in acc) {
            val aNewStart = a.newStart
            val aEnd = a.newStart + a.length
            for (s in stage) {
                val overlapStart = maxOf(aNewStart, s.oldStart)
                val overlapEnd = minOf(aEnd, s.oldStart + s.length)
                if (overlapStart >= overlapEnd) continue
                val offsetInAcc = overlapStart - aNewStart
                val oldStartInitial = a.oldStart + offsetInAcc
                val newStartFrontier = s.newStart + (overlapStart - s.oldStart)
                val kind =
                    if (a.kind == VisualOffsetMapKind.SHIFTED ||
                        s.kind == VisualOffsetMapKind.SHIFTED
                    ) {
                        VisualOffsetMapKind.SHIFTED
                    } else {
                        VisualOffsetMapKind.IDENTITY
                    }
                result.add(
                    AccSegment(
                        oldStart = oldStartInitial,
                        newStart = newStartFrontier,
                        length = overlapEnd - overlapStart,
                        kind = kind,
                    ),
                )
            }
        }
        return result
    }

    private data class AccSegment(
        val oldStart: Int,
        val newStart: Int,
        val length: Int,
        val kind: VisualOffsetMapKind,
    )

    private data class StageSegment(
        val oldStart: Int,
        val newStart: Int,
        val length: Int,
        val kind: VisualOffsetMapKind,
    )
}
