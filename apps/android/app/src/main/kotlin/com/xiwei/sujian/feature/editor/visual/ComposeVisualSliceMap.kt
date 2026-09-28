package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange

/**
 * 旧 unit 的 targetRange 按 offset map 切成存活 / ghost 片段，以及变更区间求差。
 *
 * 拆自 1372 行的 [ComposeVisualRebase]：切片逻辑只依赖
 * [ComposeVisualOffsetMapStage.entriesForFact]，是整条链上最靠前的一环，
 * 与 retained moves、几何计算没有共用状态。
 */
internal object ComposeVisualSliceMap {
    /**
     * #689 评论 5674631257 步骤6：把仍存活的 targetRange 通过 composed offset map 映射到下一份正文。
     *
     * timeline 用它把旧 unit 接到新 layout。映射失败（返回 null）就说明这段文字已被本次编辑
     * 覆盖/删除，转成 ghost fade-out，不要猜坐标。
     *
     * @param range 旧正文中的 UTF-16 range（T0 坐标）。
     * @param offsetMap 整条 chain 合成后的 T0->Tn offset map。
     * @param newTextLength 新正文长度 — 映射结果越界时返回 null。
     * @return 映射后的新正文 UTF-16 range；不在任何 entry 里（被编辑/删除）返回 null。
     */
    fun mapRangeForwardThroughOffsetMapPublic(
        range: TextRange,
        offsetMap: List<VisualOffsetMapEntry>,
        newTextLength: Int,
    ): TextRange? {
        if (range.start >= range.end) return null
        val sorted = offsetMap.sortedBy { it.oldStart }
        // 找完全包含 range 的 entry（存活正文内部）
        for (entry in sorted) {
            val oldEnd = entry.oldStart + entry.length
            if (range.start >= entry.oldStart && range.end <= oldEnd) {
                val newStart = entry.newStart + (range.start - entry.oldStart)
                val newEnd = entry.newStart + (range.end - entry.oldStart)
                if (newEnd <= newTextLength) {
                    return TextRange(newStart, newEnd)
                }
                return null
            }
        }
        // 不在任何 entry 里 → 被编辑/删除，转 ghost
        return null
    }

    /**
     * #689 评论 5675270164 缺陷5：把旧 unit 的 targetRange 按 offset-map entry 切成多个片段。
     *
     * 旧实现 [mapRangeForwardThroughOffsetMapPublic] 对整个 range 调一次映射，返回不了完整
     * 新 range 就整块判死。但一个 unit 跨过删除洞时（例如 [0,4)="ab\nc" 删除中间换行 [2,3)），
     * "ab" 和 "c" 实际都还活着，整块转 ghost 会把没删除的上一行一起重新接管/重画，制造抽动。
     *
     * 本函数按 offset-map entry 的边界把 [range] 切成子区间：
     * - 仍存活的 slice（被某 entry 完全包含）→ [MappedRangeSlice.newSubRange] 映到新正文
     * - 被删除/覆盖的 slice（不在任何 entry 里）→ [MappedRangeSlice.newSubRange] = null，转 ghost
     *
     * @param range 旧正文中的 UTF-16 range（T0 坐标）。
     * @param offsetMap 整条 chain 合成后的 T0->Tn offset map。
     * @return 切片列表；空 offsetMap 时整个 range 作为单个 GHOST slice 返回。
     */
    fun splitMappedRangeForward(
        range: TextRange,
        offsetMap: List<VisualOffsetMapEntry>,
    ): List<MappedRangeSlice> {
        if (range.start >= range.end) return emptyList()
        if (offsetMap.isEmpty()) {
            return listOf(MappedRangeSlice(range, null, MappedRangeSliceKind.GHOST))
        }
        val sorted = offsetMap.sortedBy { it.oldStart }
        val sortedCuts = collectSliceCutPoints(range, sorted)
        return buildSlicesFromCuts(sortedCuts, sorted)
    }

    private fun collectSliceCutPoints(
        range: TextRange,
        sorted: List<VisualOffsetMapEntry>,
    ): List<Int> {
        val cutPoints = sortedSetOf(range.start, range.end)
        for (entry in sorted) {
            val oldEnd = entry.oldStart + entry.length
            if (entry.oldStart > range.start && entry.oldStart < range.end) {
                cutPoints.add(entry.oldStart)
            }
            if (oldEnd > range.start && oldEnd < range.end) {
                cutPoints.add(oldEnd)
            }
        }
        return cutPoints.toList()
    }

    private fun buildSlicesFromCuts(
        sortedCuts: List<Int>,
        sorted: List<VisualOffsetMapEntry>,
    ): List<MappedRangeSlice> {
        val slices = mutableListOf<MappedRangeSlice>()
        for (i in 0 until sortedCuts.size - 1) {
            val subStart = sortedCuts[i]
            val subEnd = sortedCuts[i + 1]
            if (subEnd <= subStart) continue
            val survivingEntry =
                sorted.firstOrNull { entry ->
                    val oldEnd = entry.oldStart + entry.length
                    subStart >= entry.oldStart && subEnd <= oldEnd
                }
            slices.add(buildSlice(subStart, subEnd, survivingEntry))
        }
        return slices
    }

    private fun buildSlice(
        subStart: Int,
        subEnd: Int,
        survivingEntry: VisualOffsetMapEntry?,
    ): MappedRangeSlice {
        if (survivingEntry != null) {
            val newStart = survivingEntry.newStart + (subStart - survivingEntry.oldStart)
            val newEnd = survivingEntry.newStart + (subEnd - survivingEntry.oldStart)
            return MappedRangeSlice(
                oldSubRange = TextRange(subStart, subEnd),
                newSubRange = TextRange(newStart, newEnd),
                kind = MappedRangeSliceKind.SURVIVING,
            )
        }
        return MappedRangeSlice(
            oldSubRange = TextRange(subStart, subEnd),
            newSubRange = null,
            kind = MappedRangeSliceKind.GHOST,
        )
    }

    /**
     * #708 评论 5725706551：把旧 unit 的 targetRange 切成 SURVIVING/GHOST slice —
     * Issue #737：旧 [ComposeVisualTimeline.mapSurvivingUnits] 已删除，本切片逻辑
     * 仍供 [ComposeVisualFrameCoordinator] 算 retained moves / offset map 合成使用。
     * [ComposeLocalHandoffRebase.rebase] 共用此切片逻辑。
     *
     * 优先用 [offsetMap]；没有则从 [intent] 用 [entriesForIntent] 生成 fallback survival map；
     * 都没有则检查 target 是否仍在新正文范围内（[target.end] <= [newTextLength]）：
     * - 在范围内 → 整段 SURVIVING（range 不变）；
     * - 超出范围 → 整段 GHOST。
     *
     * @param target 旧正文中的 UTF-16 range（T0 坐标）。
     * @param offsetMap 整条 chain 合成后的 T0→Tn offset map；null 表示没有。
     * @param newTextLength 新正文长度 — fallback 判断 target 是否仍存活。
     * @param editFact 编辑事实 — offsetMap==null 时用 replaceBounds 生成 fallback。
     * @return 切片列表。
     */
    internal fun computeSlices(
        target: TextRange,
        offsetMap: List<VisualOffsetMapEntry>?,
        newTextLength: Int,
        editFact: EditorEditFact? = null,
    ): List<MappedRangeSlice> {
        val effectiveMap =
            offsetMap ?: editFact?.let { ComposeVisualOffsetMapStage.entriesForFact(it) }
        if (effectiveMap != null) {
            return splitMappedRangeForward(target, effectiveMap)
        }
        // 无 offset map 且无 intent 信息：若 target 仍在新正文范围内，保留；否则转 ghost
        return if (target.end <= newTextLength) {
            listOf(
                MappedRangeSlice(
                    oldSubRange = target,
                    newSubRange = target,
                    kind = MappedRangeSliceKind.SURVIVING,
                ),
            )
        } else {
            listOf(
                MappedRangeSlice(
                    oldSubRange = target,
                    newSubRange = null,
                    kind = MappedRangeSliceKind.GHOST,
                ),
            )
        }
    }

    /**
     * #689 评论 5675270164 缺陷5：offset-map 切片结果。
     *
     * @param oldSubRange 旧正文中的子区间。
     * @param newSubRange 新正文中的映射区间；null 表示被删除/覆盖，应转 ghost。
     * @param kind SURVIVING=存活，GHOST=被删除/覆盖。
     */
    data class MappedRangeSlice(
        val oldSubRange: TextRange,
        val newSubRange: TextRange?,
        val kind: MappedRangeSliceKind,
    )

    /**
     * #689 评论 5675270164 缺陷5：切片类型。
     */
    enum class MappedRangeSliceKind {
        /** 仍存活，映到新正文。 */
        SURVIVING,

        /** 被删除/覆盖，转 ghost。 */
        GHOST,
    }

    /**
     * #684 评论 5664636035 Bug1：从 T0→Tn composed offset map 的补集算屏幕事务的 old/new changed ranges。
     */
    fun changedRangesFromComposedMap(
        map: List<VisualOffsetMapEntry>?,
        oldLength: Int,
        newLength: Int,
    ): FrameChangedRanges {
        if (map == null) return FrameChangedRanges(emptyList(), emptyList())
        val oldRanges = complementRanges(map.map { TextRange(it.oldStart, it.oldStart + it.length) }, oldLength)
        val newRanges = complementRanges(map.map { TextRange(it.newStart, it.newStart + it.length) }, newLength)
        return FrameChangedRanges(oldRanges, newRanges)
    }

    /**
     * 计算 [0, totalLength) 中没有被 [covered] 区间覆盖的部分 — 补集。
     */
    private fun complementRanges(
        covered: List<TextRange>,
        totalLength: Int,
    ): List<TextRange> {
        if (totalLength <= 0) return emptyList()
        val sorted = covered.filter { it.start < it.end }.sortedBy { it.start }
        val result = mutableListOf<TextRange>()
        var pos = 0
        for (range in sorted) {
            if (range.start > pos) {
                result.add(TextRange(pos, minOf(range.start, totalLength)))
            }
            pos = maxOf(pos, range.end)
            if (pos >= totalLength) break
        }
        if (pos < totalLength) {
            result.add(TextRange(pos, totalLength))
        }
        return result
    }

    /**
     * #684 评论 5664636035 Bug1：屏幕事务的 old/new changed ranges — 从 composed offset map 补集算出。
     */
    data class FrameChangedRanges(
        val oldRanges: List<TextRange>,
        val newRanges: List<TextRange>,
    )

    /**
     * #684 评论 5663862982 Bug2：把 suppressed ranges（T0 坐标）按 composed offset map
     * 映射到 Tn 坐标，只返回 surviving 的 new ranges。
     *
     * #689：保留供 [ComposeVisualFrameCoordinator] 在构建 patch 时使用（虽然新 timeline
     * 不再继承 suppressed ranges，但 offset map 映射工具仍有用）。
     */
    fun mapSuppressedRangesThroughOffsetMap(
        ranges: List<TextRange>,
        offsetMap: List<VisualOffsetMapEntry>?,
    ): List<TextRange> {
        if (offsetMap == null || offsetMap.isEmpty()) return emptyList()
        val sortedEntries = offsetMap.sortedBy { it.oldStart }
        val result = mutableListOf<TextRange>()
        for (range in ranges) {
            if (range.start >= range.end) continue
            for (entry in sortedEntries) {
                val entryOldEnd = entry.oldStart + entry.length
                val overlapStart = maxOf(range.start, entry.oldStart)
                val overlapEnd = minOf(range.end, entryOldEnd)
                if (overlapStart < overlapEnd) {
                    val newStart = entry.newStart + (overlapStart - entry.oldStart)
                    result.add(TextRange(newStart, newStart + (overlapEnd - overlapStart)))
                }
            }
        }
        return result
    }

    /**
     * #641 评论 5459531909 第2项：从 [candidates] 中减去 [blockers] 覆盖的部分。
     */
    fun subtractRanges(
        candidates: List<TextRange>,
        blockers: List<TextRange>,
    ): List<TextRange> {
        if (candidates.isEmpty() || blockers.isEmpty()) return candidates
        return candidates.flatMap { candidate -> subtractCandidate(candidate, blockers) }
    }

    private fun subtractCandidate(
        candidate: TextRange,
        blockers: List<TextRange>,
    ): List<TextRange> {
        if (candidate.start >= candidate.end) return emptyList()
        val relevantBlockers =
            blockers
                .filter { it.start < candidate.end && it.end > candidate.start }
                .sortedBy { it.start }
        if (relevantBlockers.isEmpty()) return listOf(candidate)
        val result = mutableListOf<TextRange>()
        var currentStart = candidate.start
        for (blocker in relevantBlockers) {
            if (blocker.start > currentStart) {
                result.add(TextRange(currentStart, minOf(blocker.start, candidate.end)))
            }
            currentStart = maxOf(currentStart, blocker.end)
            if (currentStart >= candidate.end) break
        }
        if (currentStart < candidate.end) {
            result.add(TextRange(currentStart, candidate.end))
        }
        return result
    }
}
