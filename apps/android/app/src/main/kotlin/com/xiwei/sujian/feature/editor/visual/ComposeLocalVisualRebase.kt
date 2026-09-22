package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange

/**
 * #694 评论第 4 步 / Issue #735 评论 5771063665：本地输入视觉 rebase —
 * 把"其实只需要 old/new ranges + old/new layout"的计算从 Core 解耦出来。
 *
 * Issue #735：Core 已删除 `classify_local_visual_plan` / `LocalVisualPlanDto` /
 * `LocalVisualSliceDto` 等纯视觉 FFI 契约。本 object 只做 Android 自己的
 * UTF-16/TextLayoutResult/当前 motion 映射，不再通过 FFI 问 Core "该播哪种动画、
 * 切几个 unit"。
 *
 * grapheme cluster 拆分用 [java.text.BreakIterator]（Android 平台能力），
 * Core 只保证实际编辑 range 正确。
 *
 * 特别是删除换行：[buildOffsetMap] 必须保住换行两侧仍然存在的文字。
 * 删除掉的只生成 ghost；上一行/下一行仍存活的文字只在 oldRect != newRect 时生成 retained move。
 * 这样删除到上一行时不会把整段幸存文字重新接管一遍。
 */
@Suppress("TooManyFunctions")
internal object ComposeLocalVisualRebase {
    /**
     * #694 评论 5691696678 问题1：把连续本地输入 chain 中每笔 [LocalInputVisualEdit] 的
     * changes 逐 stage 合成成 T0→Tn 的 unchanged offset map。
     *
     * @param chain 连续本地输入链（按入队顺序，chain[i+1].oldText == chain[i].newText）。
     * @return 合成后的 T0→Tn offset map entries；空 chain 返回空列表。
     */
    fun composeLocalChainOffsetMap(chain: List<LocalInputVisualEdit>): List<VisualOffsetMapEntry> {
        if (chain.isEmpty()) return emptyList()
        val first = chain.first()
        var acc: List<VisualOffsetMapEntry> =
            if (first.oldText.isNotEmpty()) {
                listOf(
                    VisualOffsetMapEntry(
                        oldStart = 0,
                        newStart = 0,
                        length = first.oldText.length,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            } else {
                emptyList()
            }
        for (edit in chain) {
            val stage = buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
            acc = composeTwoMaps(acc, stage)
            if (acc.isEmpty()) break
        }
        return acc
    }

    /**
     * 组合两段 offset map（acc: T0→T_i, stage: T_i→T_{i+1}）成 T0→T_{i+1}。
     */
    private fun composeTwoMaps(
        acc: List<VisualOffsetMapEntry>,
        stage: List<VisualOffsetMapEntry>,
    ): List<VisualOffsetMapEntry> {
        if (acc.isEmpty() || stage.isEmpty()) return emptyList()
        val result = mutableListOf<VisualOffsetMapEntry>()
        for (a in acc) {
            val aNewEnd = a.newStart + a.length
            for (s in stage) {
                val sOldEnd = s.oldStart + s.length
                val overlapStart = maxOf(a.newStart, s.oldStart)
                val overlapEnd = minOf(aNewEnd, sOldEnd)
                if (overlapStart >= overlapEnd) continue
                val offsetInAcc = overlapStart - a.newStart
                val oldStartInitial = a.oldStart + offsetInAcc
                val newStartFrontier = s.newStart + (overlapStart - s.oldStart)
                val kind =
                    if (a.kind == VisualOffsetMapKind.SHIFTED || s.kind == VisualOffsetMapKind.SHIFTED) {
                        VisualOffsetMapKind.SHIFTED
                    } else {
                        VisualOffsetMapKind.IDENTITY
                    }
                result.add(
                    VisualOffsetMapEntry(
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

    /**
     * #694 评论第 4 步：从 [LocalInputChange.oldRange]/[LocalInputChange.newRange]
     * 构造 T0→Tn 的 unchanged offset map。
     *
     * @param changes 本次本地输入的 change 列表（来自 TextFieldBuffer.forEachChange）。
     * @param oldLength 旧正文长度（UTF-16）。
     * @param newLength 新正文长度（UTF-16）。
     * @return offset map entries；空 changes 时若 oldLength==newLength 返回整段 identity。
     */
    fun buildOffsetMap(
        changes: List<LocalInputChange>,
        oldLength: Int,
        newLength: Int,
    ): List<VisualOffsetMapEntry> {
        if (changes.isEmpty()) {
            if (oldLength > 0 && newLength > 0 && oldLength == newLength) {
                return listOf(
                    VisualOffsetMapEntry(
                        oldStart = 0,
                        newStart = 0,
                        length = oldLength,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
            return emptyList()
        }
        val sorted = changes.sortedBy { it.oldRange.start }
        val entries = mutableListOf<VisualOffsetMapEntry>()
        var oldCursor = 0
        var newCursor = 0
        for (change in sorted) {
            val oldStart = change.oldRange.start
            val newStart = change.newRange.start
            if (oldCursor < oldStart && newCursor < newStart) {
                val len = minOf(oldStart - oldCursor, newStart - newCursor)
                if (len > 0) {
                    entries.add(
                        VisualOffsetMapEntry(
                            oldStart = oldCursor,
                            newStart = newCursor,
                            length = len,
                            kind = VisualOffsetMapKind.IDENTITY,
                        ),
                    )
                }
            }
            oldCursor = change.oldRange.end
            newCursor = change.newRange.end
        }
        if (oldCursor < oldLength && newCursor < newLength) {
            val len = minOf(oldLength - oldCursor, newLength - newCursor)
            if (len > 0) {
                entries.add(
                    VisualOffsetMapEntry(
                        oldStart = oldCursor,
                        newStart = newCursor,
                        length = len,
                        kind = VisualOffsetMapKind.IDENTITY,
                    ),
                )
            }
        }
        return entries
    }

    /**
     * #694 评论第 4 步：从 offset map 的补集算 deletedUnits/insertedUnits。
     * 复用 [ComposeVisualRebase.changedRangesFromComposedMap]。
     */
    fun changedRangesFromOffsetMap(
        offsetMap: List<VisualOffsetMapEntry>,
        oldLength: Int,
        newLength: Int,
    ): ComposeVisualRebase.FrameChangedRanges =
        ComposeVisualRebase.changedRangesFromComposedMap(offsetMap, oldLength, newLength)

    /**
     * #694 评论 5691696678 问题3：把连续本地输入 chain 中每笔 edit 的 insertedUnits
     * 逐 stage 合成到最终 Tn 坐标，保留多字符吐字顺序。
     *
     * @param chain 连续本地输入链（按入队顺序）。
     * @return 合成到最终 Tn 坐标的 insertedUnits 列表（去重保序）。
     */
    fun composeLocalChainInsertedUnits(chain: List<LocalInputVisualEdit>): List<TextRange> {
        if (chain.isEmpty()) return emptyList()
        val perStageNewUnits =
            chain.map { edit ->
                val stageMap = buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
                changedRangesFromOffsetMap(stageMap, edit.oldText.length, edit.newText.length).newRanges
            }
        val perStageOffsetMaps =
            chain.map { edit ->
                buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
            }
        return ComposeVisualRebase.composeNewUnitsToFinalStages(perStageNewUnits, perStageOffsetMaps)
    }

    /**
     * #694 评论 5691696678 问题3：把连续本地输入 chain 中每笔 edit 的 deletedUnits
     * 逐 stage 合成回最初 T0 坐标，保留多字符吞字顺序。
     *
     * @param chain 连续本地输入链（按入队顺序）。
     * @return 合成回最初 T0 坐标的 deletedUnits 列表（去重保序）。
     */
    fun composeLocalChainDeletedUnits(chain: List<LocalInputVisualEdit>): List<TextRange> {
        if (chain.isEmpty()) return emptyList()
        val perStageOldUnits =
            chain.map { edit ->
                val stageMap = buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
                changedRangesFromOffsetMap(stageMap, edit.oldText.length, edit.newText.length).oldRanges
            }
        val perStageOffsetMaps =
            chain.map { edit ->
                buildOffsetMap(edit.changes, edit.oldText.length, edit.newText.length)
            }
        return ComposeVisualRebase.composeOldUnitsToBaseStages(perStageOldUnits, perStageOffsetMaps)
    }

    /**
     * Issue #735 评论 5771063665：Android 自己的本地视觉 plan 分类 —
     * 纯计算，不调 Core。用 [splitGraphemeClusterRangesWithZwjMerge] 做 grapheme cluster 拆分，
     * 根据 affected ranges 的特征推导 [AnimationMode] 和 animation units。
     *
     * @param oldText 旧正文（UTF-16）。
     * @param newText 新正文（UTF-16）。
     * @param oldAffectedRanges 旧正文侧 affected ranges（UTF-16 [TextRange]）。
     * @param newAffectedRanges 新正文侧 affected ranges（UTF-16 [TextRange]）。
     * @param animationEnabled 是否启用动画。
     * @return [LocalVisualPlan]（animationMode + old/new animation units，UTF-16 [TextRange]）。
     */
    fun classifyLocalVisualPlan(
        oldText: String,
        newText: String,
        oldAffectedRanges: List<TextRange>,
        newAffectedRanges: List<TextRange>,
        animationEnabled: Boolean,
    ): LocalVisualPlan {
        if (!animationEnabled) {
            return LocalVisualPlan(
                animationMode = AnimationMode.SYSTEM_SUPPRESSED,
                oldAnimationUnits = emptyList(),
                newAnimationUnits = emptyList(),
            )
        }
        val oldUnits = oldAffectedRanges.flatMap { splitGraphemeClusterRangesWithZwjMerge(oldText.substring(it.start, it.end)) }
        val newUnits = newAffectedRanges.flatMap { splitGraphemeClusterRangesWithZwjMerge(newText.substring(it.start, it.end)) }
        val clusterCount = oldUnits.size + newUnits.size
        if (clusterCount == 0) {
            return LocalVisualPlan(
                animationMode = AnimationMode.SYSTEM_SUPPRESSED,
                oldAnimationUnits = emptyList(),
                newAnimationUnits = emptyList(),
            )
        }
        val containsNewline = oldAffectedRanges.any { range -> oldText.substring(range.start, range.end).contains('\n') } ||
            newAffectedRanges.any { range -> newText.substring(range.start, range.end).contains('\n') }
        val animationMode =
            if (containsNewline) {
                AnimationMode.LINE_REFLOW_ANIMATION
            } else if (clusterCount <= 8) {
                AnimationMode.GLYPH_ANIMATION
            } else {
                AnimationMode.RUN_ANIMATION
            }
        return LocalVisualPlan(
            animationMode = animationMode,
            oldAnimationUnits = oldAffectedRanges,
            newAnimationUnits = newAffectedRanges,
        )
    }

    /**
     * #694 评论 5692161955 回归修复：用 `java.text.BreakIterator` 拆分 grapheme cluster，
     * 再按 ZWJ（U+200D）合并被错误拆开的 emoji family。
     *
     * 此方案不依赖 ICU4J 是否可用，在所有环境（Robolectric、真实设备）下都能正确处理
     * ZWJ emoji family。
     *
     * @param text 要拆分的文本（UTF-16）。
     * @return cluster 在原文中的 UTF-16 [TextRange] 列表（按顺序，不重叠，覆盖所有字符）。
     */
    internal fun splitGraphemeClusterRangesWithZwjMerge(text: String): List<TextRange> {
        if (text.isEmpty()) return emptyList()
        val iterator = java.text.BreakIterator.getCharacterInstance()
        iterator.setText(text)
        val rawRanges = mutableListOf<TextRange>()
        var start = iterator.first()
        var end = iterator.next()
        while (end != java.text.BreakIterator.DONE) {
            rawRanges.add(TextRange(start, end))
            start = end
            end = iterator.next()
        }
        if (rawRanges.isEmpty()) return emptyList()
        val zwj = '\u200D'
        val mergedRanges = mutableListOf<TextRange>()
        var currentStart = rawRanges[0].start
        var currentEnd = rawRanges[0].end
        var lastClusterContainsZwj = text.substring(currentStart, currentEnd).contains(zwj)
        for (i in 1 until rawRanges.size) {
            val range = rawRanges[i]
            val segment = text.substring(range.start, range.end)
            val segmentContainsZwj = segment.contains(zwj)
            if (segmentContainsZwj || lastClusterContainsZwj) {
                currentEnd = range.end
            } else {
                mergedRanges.add(TextRange(currentStart, currentEnd))
                currentStart = range.start
                currentEnd = range.end
            }
            lastClusterContainsZwj = segmentContainsZwj
        }
        mergedRanges.add(TextRange(currentStart, currentEnd))
        return mergedRanges
    }

    /**
     * #694 评论第 4 步：retained reflow 只比较 oldLayout -> newLayout 的真实几何。
     * 复用 [ComposeVisualRebase.computeRetainedMovesFromComposedMap]。
     */
    fun computeRetainedMoves(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>,
    ): List<RetainedMove> = ComposeVisualRebase.computeRetainedMovesFromComposedMap(oldLayout, newLayout, offsetMap)

    /**
     * 安全获取 path bounds — 复用 [ComposeLayoutSnapshot.boundsForRawRange]。
     */
    fun safePathBounds(
        snapshot: ComposeLayoutSnapshot,
        range: TextRange,
    ): Rect? = snapshot.boundsForRawRange(range)

    /**
     * #694 评论 5693864609 问题1：把 plan 的细粒度 unit 按 local chain 的 stage range 重新排序。
     *
     * @param planUnits plan 返回的 animation units（UTF-16 [TextRange]，已按粒度切分）。
     * @param orderedStageRanges local chain 的时间顺序 stage ranges。
     * @return 按 stage range 顺序重排后的 plan units；空 plan 或空 stage ranges 时原样返回 planUnits。
     */
    fun orderPlanUnitsByStageRanges(
        planUnits: List<TextRange>,
        orderedStageRanges: List<TextRange>,
    ): List<TextRange> {
        if (planUnits.isEmpty()) return emptyList()
        if (orderedStageRanges.isEmpty()) return planUnits
        val result = mutableListOf<TextRange>()
        val used = mutableSetOf<Int>()
        for (stageRange in orderedStageRanges) {
            for ((i, planUnit) in planUnits.withIndex()) {
                if (i in used) continue
                if (planUnit.start < stageRange.end && planUnit.end > stageRange.start) {
                    result.add(planUnit)
                    used.add(i)
                }
            }
        }
        for ((i, planUnit) in planUnits.withIndex()) {
            if (i !in used) result.add(planUnit)
        }
        return result
    }
}

/**
 * Issue #735 评论 5771063665：Android 自己的本地视觉 plan —
 * 取代已删除的 Core `LocalVisualPlanDto`。纯 Android 数据结构，不跨 FFI。
 *
 * @param animationMode Android 推导的动画模式。
 * @param oldAnimationUnits 旧动画单元（UTF-16 [TextRange]）。
 * @param newAnimationUnits 新动画单元（UTF-16 [TextRange]）。
 */
data class LocalVisualPlan(
    val animationMode: AnimationMode,
    val oldAnimationUnits: List<TextRange>,
    val newAnimationUnits: List<TextRange>,
)
