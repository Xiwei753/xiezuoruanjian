package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #694 评论第 4 步：本地输入视觉 rebase —
 * 把"其实只需要 old/new ranges + old/new layout"的计算从 Core [EditorVisualIntent] 解耦出来。
 *
 * 本地键盘/退格的动画不再绕 [EditorVisualIntent] / Core 回声才能调用 [ComposeVisualRebase]。
 * 这里复用 [ComposeVisualRebase] 现有的 range 切片/映射函数，只是入口换成
 * [LocalInputChange] + old/new [ComposeLayoutSnapshot]。
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
     * 不同坐标系的 changes 不能直接摊平，必须通过 stage map 逐级合成：
     * - stage 0: T0→T1（首笔 edit.changes）
     * - stage 1: T1→T2
     * - ...
     * - stage n-1: Tn-1→Tn
     * - 合成: T0→Tn
     *
     * 算法参考 [ComposeVisualPatchBatch.composeBatchOffsetMap] / [composeTwoMaps]，
     * 这里实现一份同样的合成算法（[ComposeVisualPatchBatch.composeTwoMaps] 是 private，
     * 不便跨 object 复用；保持本 object 自洽）。
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
     * 与 [ComposeVisualPatchBatch.composeTwoMaps] / [ComposeVisualRebase.composeStage] 同算法。
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
     * 存活文本（不在任何 oldRange 内的部分）映射到 newText 中对应位置；
     * 被编辑/删除的区域没有 entry（补集即 changedRanges）。
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
            // 存活段 [oldCursor, oldStart) → [newCursor, newStart)
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
        // 尾部存活段 [oldCursor, oldLength) → [newCursor, newLength)
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
     * 每笔 edit 的 insertedUnits 从该笔 stage offset map 补集算（per-stage 净变化），
     * 然后用 [ComposeVisualRebase.composeNewUnitsToFinalStages] 沿后续 stage offset map
     * 映射到最终 Tn。这样 `"" -> "a" -> "ab" -> "abc"` 的 chain 会得到 3 个 unit
     * `[0,1), [1,2), [2,3)` 而非单个 `[0,3)`。
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
     * #694 评论第 4 步：retained reflow 只比较 oldLayout -> newLayout 的真实几何。
     * 复用 [ComposeVisualRebase.computeRetainedMovesFromComposedMap]。
     *
     * retainedMoves 只按第一份旧 layout 和最后一份新 layout 算一次，
     * 不把同一帧内中间态的幸存文字重新接管一遍。
     */
    fun computeRetainedMoves(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>,
    ): List<RetainedMove> =
        ComposeVisualRebase.computeRetainedMovesFromComposedMap(oldLayout, newLayout, offsetMap)

    /**
     * #694 评论第 4 步：cursor 从 oldSelection.end -> newSelection.end 构造路径。
     *
     * 多插入 unit 时按 unit 顺序生成路径点，endFraction 与 timeline 的 unit-wise 分段时序一致；
     * 单字符/删除/无 unit 时只保留最终目标点（endFraction = 1f）。
     * 不伪造中间位置，最终几何 target 只取最后 layout。
     *
     * @return 光标运动路径；取不到新光标 rect 时返回 null。
     */
    fun buildCursorPath(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        oldSelection: TextRange,
        newSelection: TextRange,
        insertedUnits: List<TextRange>,
        deletedUnits: List<TextRange>,
    ): CursorMotionPath? {
        val newCursorRect = safeCursorRectFromLayout(newLayout, newSelection.end) ?: return null
        // 无文字动画语义 → 单点路径 snap 到新光标位置。
        if (insertedUnits.isEmpty() && deletedUnits.isEmpty()) {
            return CursorMotionPath(listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)))
        }
        // 多插入 unit：按 unit.end 顺序生成路径点（光标依次经过每个字/cluster 出现后的位置）。
        val points = mutableListOf<CursorMotionPoint>()
        for (unit in insertedUnits) {
            val rect = safeCursorRectFromLayout(newLayout, unit.end) ?: continue
            points.add(CursorMotionPoint(rect = rect, endFraction = 0f))
        }
        if (points.isEmpty()) {
            // 只有删除 unit 或插入 unit 都取不到 rect → 最终目标点。
            points.add(CursorMotionPoint(rect = newCursorRect, endFraction = 1f))
        } else {
            // 归一化 endFraction = (i + 1f) / n，与 timeline unit-wise 分段时序一致。
            val n = points.size
            for (i in points.indices) {
                points[i] = points[i].copy(endFraction = (i + 1f) / n)
            }
        }
        return CursorMotionPath(points)
    }

    /**
     * 安全获取光标 rect — offset 越界或 layout 抛异常时返回 null。
     */
    private fun safeCursorRectFromLayout(
        layout: ComposeLayoutSnapshot,
        offset: Int,
    ): Rect? {
        val textLen = layout.result.layoutInput.text.length
        if (offset < 0 || offset > textLen) return null
        return try {
            layout.result.getCursorRect(offset)
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * 安全获取 path bounds — 复用 [ComposeVisualRebase.safePathBounds]。
     */
    fun safePathBounds(
        result: TextLayoutResult,
        range: TextRange,
    ): Rect? = ComposeVisualRebase.safePathBounds(result, range)
}
