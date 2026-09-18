package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #708 评论 5723410606 第四节：独立 reflow 通道 —
 * 自动换行/手动换行时幸存文字从旧位置平移到新位置，**始终全亮**，
 * 不再拿 insert/delete 动画硬扛，也不再凭"整行/整段 path.getBounds()"当一个块移动。
 *
 * 与 [RetainedMove] 的区别：
 * - [RetainedMove] 只描述 oldRange/newRange，bounds 由 timeline 从 layout 现算；
 * - [ComposeReflowMove] 同时携带 oldBounds/newBounds，避免 timeline 重新查 layout，
 *   也让 planner 在切分时能直接判断"位置是否真的变化"。
 *
 * @param oldRange 旧正文 UTF-16 range（T0 坐标）。
 * @param newRange 新正文 UTF-16 range（Tn 坐标）。
 * @param oldBounds 旧 layout 中此 range 的 path bounds（屏幕坐标）。
 * @param newBounds 新 layout 中此 range 的 path bounds（屏幕坐标）。
 */
data class ComposeReflowMove(
    val oldRange: TextRange,
    val newRange: TextRange,
    val oldBounds: Rect,
    val newBounds: Rect,
)

/**
 * #708 评论 5723410606 第四节：reflow 规划器 —
 * 从 oldLayout / newLayout / offsetMap 算出幸存文字的真实屏幕位移。
 *
 * 算法（评论 5723410606 第四节）：
 * 1. 从 offsetMap 只拿幸存文本（IDENTITY/SHIFTED entry）；
 * 2. 同时按 old layout 行边界 和 new layout 行边界 切段；
 * 3. 每个 slice 必须在 old/new 两边都只落在单一视觉行；
 * 4. 分别取 old/new path bounds；
 * 5. 只有位置真的变化才生成 [ComposeReflowMove]。
 *
 * 不再把"整行/整段 path.getBounds()"当一个块移动 — 那会把跨行幸存文字压成一个巨大矩形，
 * 位置变化时整段字一起平移，视觉上像"整行抽动"。
 */
internal object ComposeReflowPlanner {
    /**
     * 规划幸存文字的 reflow moves。
     *
     * @param oldLayout T0 时的 layout 快照。
     * @param newLayout Tn 时的 layout 快照。
     * @param offsetMap T0→Tn 合成后的 offset map（只取 IDENTITY/SHIFTED entry）。
     * @return 位置真变化的 [ComposeReflowMove] 列表；无变化时为空。
     */
    fun plan(
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>?,
    ): List<ComposeReflowMove> {
        if (offsetMap.isNullOrEmpty()) return emptyList()
        val oldResult = oldLayout.result
        val newResult = newLayout.result
        val moves = mutableListOf<ComposeReflowMove>()
        for (entry in offsetMap) {
            collectMovesForEntry(entry, oldResult, newResult, moves)
        }
        return moves
    }

    /**
     * 单个 offset map entry 的 reflow move 收集 —
     * 从 [plan] 抽出以降低圈复杂度。
     */
    private fun collectMovesForEntry(
        entry: VisualOffsetMapEntry,
        oldResult: TextLayoutResult,
        newResult: TextLayoutResult,
        moves: MutableList<ComposeReflowMove>,
    ) {
        if (entry.length <= 0) return
        val oldStart = entry.oldStart
        val newStart = entry.newStart
        val length = entry.length
        if (oldStart + length > oldResult.layoutInput.text.length) return
        if (newStart + length > newResult.layoutInput.text.length) return
        // 按 old/new 行边界切段，每段在两边都只落在单一视觉行
        val slices = splitEntryByBothLineBounds(oldResult, newResult, oldStart, newStart, length)
        for (slice in slices) {
            val oldBounds = safePathBounds(oldResult, slice.oldRange) ?: continue
            val newBounds = safePathBounds(newResult, slice.newRange) ?: continue
            // 只有位置真的变化才生成 move
            if (oldBounds.left == newBounds.left && oldBounds.top == newBounds.top) continue
            moves.add(
                ComposeReflowMove(
                    oldRange = slice.oldRange,
                    newRange = slice.newRange,
                    oldBounds = oldBounds,
                    newBounds = newBounds,
                ),
            )
        }
    }

    private data class ReflowSlice(
        val oldRange: TextRange,
        val newRange: TextRange,
    )

    /**
     * 把一个 entry 按 old/new 两边真实视觉行边界切片 —
     * 每个 slice 在 old/new 两边都只落在单一视觉行。
     */
    private fun splitEntryByBothLineBounds(
        oldResult: TextLayoutResult,
        newResult: TextLayoutResult,
        oldStart: Int,
        newStart: Int,
        length: Int,
    ): List<ReflowSlice> {
        val cutOffsets = sortedSetOf(0, length)
        // old 行边界 + new 行边界
        collectLineCutOffsets(oldResult, oldStart, length, cutOffsets)
        collectLineCutOffsets(newResult, newStart, length, cutOffsets)
        return buildSlicesFromCuts(cutOffsets, oldStart, newStart)
    }

    /**
     * 扫描单边（old 或 new）的视觉行边界，把行末相对偏移加进 [cutOffsets] —
     * 从 [splitEntryByBothLineBounds] 抽出以降低认知复杂度。
     */
    private fun collectLineCutOffsets(
        result: TextLayoutResult,
        start: Int,
        length: Int,
        cutOffsets: MutableSet<Int>,
    ) {
        var scan = 0
        while (scan < length) {
            val offset = start + scan
            if (offset >= result.layoutInput.text.length) break
            val line = result.getLineForOffset(offset)
            val lineEnd = result.getLineEnd(line)
            val nextCut = lineEnd - start
            if (nextCut in (scan + 1)..length) cutOffsets.add(nextCut)
            scan = lineEnd - start
            if (scan <= 0) scan = 1
        }
    }

    /**
     * 从已排序切点构建 [ReflowSlice] 列表 —
     * 从 [splitEntryByBothLineBounds] 抽出以降低认知复杂度。
     */
    private fun buildSlicesFromCuts(
        cutOffsets: Collection<Int>,
        oldStart: Int,
        newStart: Int,
    ): List<ReflowSlice> {
        val slices = mutableListOf<ReflowSlice>()
        val sortedCuts = cutOffsets.sorted()
        for (i in 0 until sortedCuts.size - 1) {
            val chunkStart = sortedCuts[i]
            val chunkEnd = sortedCuts[i + 1]
            if (chunkEnd <= chunkStart) continue
            slices.add(
                ReflowSlice(
                    oldRange = TextRange(oldStart + chunkStart, oldStart + chunkEnd),
                    newRange = TextRange(newStart + chunkStart, newStart + chunkEnd),
                ),
            )
        }
        return slices
    }

    private fun safePathBounds(
        result: TextLayoutResult,
        range: TextRange,
    ): Rect? {
        if (range.start >= range.end) return null
        if (range.end > result.layoutInput.text.length) return null
        return try {
            result.getPathForRange(range.start, range.end).getBounds()
        } catch (_: Throwable) {
            null
        }
    }
}
