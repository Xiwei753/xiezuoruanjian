package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.rawLineEndForRawOffset

/**
 * #644 评论 5467821839 第5节剩余子项：visual rebase 纯计算 —
 * 从 [ComposeEditorVisualState] 抽出的无副作用几何/区间函数。
 *
 * #689 评论 5674631257 步骤6：删除整套旧事务物化代码。
 * 保留纯计算：
 * - [computeRetainedMoves] / [computeRetainedMovesFromComposedMap]
 * - 动画 unit 的 stage 合成（转 [ComposeVisualOffsetMapStage]）
 * - range 切分与变更区间（转 [ComposeVisualSliceMap]）
 * - 几何与光标快照（转 [ComposeVisualGeometry]）
 * - retained move 合并状态机（转 [ComposeVisualRetainedMoveMerge]）
 *
 * 删除：
 * - MaterializeStartFrameParams / materializeStartFrame
 * - collectCurrentSlicesAsRebased / collectInsertSlicesAsRebased / collectDeleteSlicesAsRebased
 * - collectRetainedMoveSlicesAsRebased
 * - 所有只服务 ComposeVisualFrame/RebasedTextSlice 的 split/materialize 方法
 * - unitLocalProgress（旧 overlay 已删除，无调用）
 */
internal object ComposeVisualRebase {
    /**
     * #641 评论 问题3 + 评论 5457777142 问题2 + 评论 5458283021 问题2b：retained move 计算 —
     * 自动折行/手动换行的 retained move 用 old/new [TextLayoutResult]
     * 比较同一逻辑文本范围的位置变化生成。
     *
     * Issue #735 评论 5771063665：不再接收 [EditorVisualIntent]，改为接收纯数据。
     */
    fun computeRetainedMoves(
        textKind: TextVisualKind,
        replaceBounds: VisualReplaceBounds?,
        oldRanges: List<TextRange>,
        newRanges: List<TextRange>,
        previousSnapshot: ComposeLayoutSnapshot?,
        currentSnapshot: ComposeLayoutSnapshot?,
    ): List<RetainedMove> {
        if (textKind == TextVisualKind.None) return emptyList()
        val prev = previousSnapshot ?: return emptyList()
        val curr = currentSnapshot ?: return emptyList()

        val oldSuffixStart =
            replaceBounds?.oldEnd ?: (oldRanges.maxOfOrNull { it.end } ?: 0)
        val newSuffixStart =
            replaceBounds?.newEnd ?: (newRanges.maxOfOrNull { it.end } ?: 0)

        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        // 类型从 AnnotatedString 变 String，String 也是 CharSequence。
        val oldText = prev.result.layoutInput.text.text
        val newText = curr.result.layoutInput.text.text
        val oldTextLen = oldText.length
        val newTextLen = newText.length

        if (oldSuffixStart >= oldTextLen || newSuffixStart >= newTextLen) return emptyList()

        val ctx =
            ComposeVisualRetainedMoveMerge.RetainedMovesContext(
                prev = prev,
                curr = curr,
                oldText = oldText,
                newText = newText,
                oldTextLen = oldTextLen,
                newTextLen = newTextLen,
                oldSuffixStart = oldSuffixStart,
                newSuffixStart = newSuffixStart,
            )
        return ComposeVisualRetainedMoveMerge.computeRetainedMovesLoop(ctx)
    }

    /**
     * #644 评论 #684：按 offset map chain 合并整条事务链的 retained moves。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun computeRetainedMoves(
        oldLayout: ComposeLayoutSnapshot?,
        newLayout: ComposeLayoutSnapshot?,
        chain: List<EditorEditFact>,
    ): List<RetainedMove> {
        val prev = oldLayout ?: return emptyList()
        val curr = newLayout ?: return emptyList()
        if (chain.isEmpty()) return emptyList()

        val composed = ComposeVisualOffsetMapStage.composeOffsetMapChain(chain)
        if (composed != null) {
            return computeRetainedMovesFromComposedMap(prev, curr, composed)
        }

        return computeRetainedMovesLegacy(prev, curr, chain)
    }

    /**
     * #644 评论 #684 + 评论 5662132136 第1项 + 评论 5663032418 断点2：
     * 用合成后的 offset map 计算 retained moves。
     *
     * #694 评论第 4 步：internal 可见性 — 供 [ComposeLocalVisualRebase] 复用，
     * 本地输入不再绕 [EditorVisualIntent] 才能调用。
     */
    internal fun computeRetainedMovesFromComposedMap(
        prev: ComposeLayoutSnapshot,
        curr: ComposeLayoutSnapshot,
        composed: List<VisualOffsetMapEntry>,
    ): List<RetainedMove> {
        val moves = mutableListOf<RetainedMove>()
        for (entry in composed) {
            val oldStart = entry.oldStart
            val newStart = entry.newStart
            val length = entry.length
            if (length <= 0) continue
            // Issue #717 评论 5742904417 修复1：边界检查用 rawText 长度。
            if (oldStart + length > prev.result.layoutInput.text.text.length) continue
            if (newStart + length > curr.result.layoutInput.text.text.length) continue

            val chunks = splitEntryByVisualLines(prev, curr, oldStart, newStart, length)
            mergeChunksIntoMoves(chunks, moves)
        }
        return moves
    }

    /**
     * #684 评论 5663032418 断点2：把一个合成 entry 按 old/new 两边真实视觉行边界切片。
     *
     * Issue #717 评论 5742273757 修复3：改为接收 [ComposeLayoutSnapshot]，
     * 内部通过 projection 做 raw→display 映射再调 TextLayoutResult。
     */
    private fun splitEntryByVisualLines(
        prevSnapshot: ComposeLayoutSnapshot,
        currSnapshot: ComposeLayoutSnapshot,
        oldStart: Int,
        newStart: Int,
        length: Int,
    ): List<RetainedMoveChunk> {
        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        // avoidSurrogateCut 接收 CharSequence，String 兼容。
        val oldText = prevSnapshot.result.layoutInput.text.text
        val newText = currSnapshot.result.layoutInput.text.text
        val cutOffsets = sortedSetOf(0, length)
        var scan = 0
        while (scan < length) {
            val oldOffset = oldStart + scan
            if (oldOffset >= oldText.length) break
            // Issue #717 评论 5742904417 修复2：lineEnd 转回 raw 坐标。
            // getLineEnd 返回 display offset（含 U+200B），retained move 切片需要 raw offset。
            // rawLineEndForRawOffset 内部会调 lineForRawOffset 查行。
            val oldLineEnd = prevSnapshot.rawLineEndForRawOffset(oldOffset)
            val nextCut = oldLineEnd - oldStart
            if (nextCut in (scan + 1)..length) {
                cutOffsets.add(avoidSurrogateCut(oldText, oldStart, nextCut, length))
            }
            scan = oldLineEnd - oldStart
            if (scan <= 0) scan = 1
        }
        scan = 0
        while (scan < length) {
            val newOffset = newStart + scan
            if (newOffset >= newText.length) break
            // Issue #717 评论 5742904417 修复2：lineEnd 转回 raw 坐标。
            val newLineEnd = currSnapshot.rawLineEndForRawOffset(newOffset)
            val nextCut = newLineEnd - newStart
            if (nextCut in (scan + 1)..length) {
                cutOffsets.add(avoidSurrogateCut(newText, newStart, nextCut, length))
            }
            scan = newLineEnd - newStart
            if (scan <= 0) scan = 1
        }

        val chunks = mutableListOf<RetainedMoveChunk>()
        val sortedCuts = cutOffsets.toList()
        for (i in 0 until sortedCuts.size - 1) {
            val chunkStart = sortedCuts[i]
            val chunkEnd = sortedCuts[i + 1]
            if (chunkEnd <= chunkStart) continue
            val oldRange = TextRange(oldStart + chunkStart, oldStart + chunkEnd)
            val newRange = TextRange(newStart + chunkStart, newStart + chunkEnd)
            val oldBounds = ComposeVisualGeometry.safePathBounds(prevSnapshot, oldRange)
            val newBounds = ComposeVisualGeometry.safePathBounds(currSnapshot, newRange)
            if (oldBounds == null || newBounds == null) continue
            val dx = newBounds.left - oldBounds.left
            val dy = newBounds.top - oldBounds.top
            chunks.add(
                RetainedMoveChunk(
                    oldRange = oldRange,
                    newRange = newRange,
                    dx = dx,
                    dy = dy,
                ),
            )
        }
        return chunks
    }

    /**
     * Issue #717 评论 5742904417 修复1：签名改成接收 [CharSequence]，
     * 这样 String（rawText）和 AnnotatedString 都能传入。
     */
    private fun avoidSurrogateCut(
        text: CharSequence,
        base: Int,
        cutOffset: Int,
        maxOffset: Int,
    ): Int {
        if (cutOffset <= 0 || cutOffset >= maxOffset) return cutOffset.coerceIn(0, maxOffset)
        val absCut = base + cutOffset
        if (absCut in 1 until text.length &&
            text[absCut - 1].isHighSurrogate() &&
            text[absCut].isLowSurrogate()
        ) {
            return (cutOffset - 1).coerceIn(0, maxOffset)
        }
        return cutOffset
    }

    private data class RetainedMoveChunk(
        val oldRange: TextRange,
        val newRange: TextRange,
        val dx: Float,
        val dy: Float,
    )

    private fun mergeChunksIntoMoves(
        chunks: List<RetainedMoveChunk>,
        out: MutableList<RetainedMove>,
    ) {
        if (chunks.isEmpty()) return
        var mergeStartIdx = 0
        for (i in 1..chunks.size) {
            val prevChunk = chunks[i - 1]
            val canContinue =
                i < chunks.size &&
                    kotlin.math.abs(chunks[i].dx - prevChunk.dx) <= 1f &&
                    kotlin.math.abs(chunks[i].dy - prevChunk.dy) <= 1f &&
                    chunks[i].oldRange.start == prevChunk.oldRange.end &&
                    chunks[i].newRange.start == prevChunk.newRange.end
            if (!canContinue) {
                val first = chunks[mergeStartIdx]
                val last = chunks[i - 1]
                val dx = first.dx
                val dy = first.dy
                if (kotlin.math.abs(dx) > 1f || kotlin.math.abs(dy) > 1f) {
                    out.add(
                        RetainedMove(
                            oldRange = TextRange(first.oldRange.start, last.oldRange.end),
                            newRange = TextRange(first.newRange.start, last.newRange.end),
                        ),
                    )
                }
                mergeStartIdx = i
            }
        }
    }

    /**
     * #644 评论 #684：回退路径 — 取最后一个 replaceBounds / 所有 ranges 摊平做线性平移。
     *
     * Issue #735 评论 5771063665：chain 类型从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    private fun computeRetainedMovesLegacy(
        prev: ComposeLayoutSnapshot,
        curr: ComposeLayoutSnapshot,
        chain: List<EditorEditFact>,
    ): List<RetainedMove> {
        val lastWithBounds = chain.lastOrNull { it.replaceBounds != null }
        val replaceBounds = lastWithBounds?.replaceBounds

        val effectiveOldRanges = chain.flatMap { it.oldRanges }.filter { it.start < it.end }
        val effectiveNewRanges = chain.flatMap { it.newRanges }.filter { it.start < it.end }

        val oldSuffixStart =
            replaceBounds?.oldEnd
                ?: (effectiveOldRanges.maxOfOrNull { it.end } ?: 0)
        val newSuffixStart =
            replaceBounds?.newEnd
                ?: (effectiveNewRanges.maxOfOrNull { it.end } ?: 0)

        // Issue #717 评论 5742904417 修复1：文本身份用 rawText（不含 U+200B）。
        val oldText = prev.result.layoutInput.text.text
        val newText = curr.result.layoutInput.text.text
        val oldTextLen = oldText.length
        val newTextLen = newText.length

        if (oldSuffixStart >= oldTextLen || newSuffixStart >= newTextLen) return emptyList()

        val ctx =
            ComposeVisualRetainedMoveMerge.RetainedMovesContext(
                prev = prev,
                curr = curr,
                oldText = oldText,
                newText = newText,
                oldTextLen = oldTextLen,
                newTextLen = newTextLen,
                oldSuffixStart = oldSuffixStart,
                newSuffixStart = newSuffixStart,
            )
        return ComposeVisualRetainedMoveMerge.computeRetainedMovesLoop(ctx)
    }

    /**
     * #684 评论 5669048233 Bug2 修复 + #694 评论 5691696678 问题3：通用 stage-map 版本 —
     * 把每个 stage 的 newUnits 沿后续 stage offset map 映射到最终 Tn 坐标。
     *
     * 不依赖 [EditorVisualIntent]，接收纯 [List]<[TextRange]> 和 [List]<[VisualOffsetMapEntry]?>，
     * 供 [ComposeVisualPatchBatch.compose] 合成本地输入 patch 的 insertedUnits（保留吐字顺序）。
     *
     * 算法和 [ComposeVisualOffsetMapStage.composeNewAnimationUnitsToFinal] 一致：每笔的 units 沿后续 stage offset map
     * 映射到最终 Tn（用 [ComposeVisualOffsetMapStage.mapRangesForwardThroughOffsetMap]），null offset map 跳过该 stage 映射
     * （和 `entries == null -> continue` 同语义），空 entries 清空 units。最后 [ComposeVisualOffsetMapStage.deduplicateRanges]。
     *
     * @param perStageNewUnits 每个 stage 的新动画 units（T_i 坐标）。
     * @param perStageOffsetMaps 每个 stage 的 offset map（T_i→T_{i+1}）；null 表示该 stage 无 offset map。
     * @return 合成到最终 Tn 坐标的 units 列表（去重保序）。
     */
}
