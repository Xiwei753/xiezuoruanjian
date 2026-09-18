package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #708 评论 5725146968：overlay ownership 差集 helper —
 * 供首帧 handoff（`publishLocalHandoffScene`）和 timeline（`applyReflowMoves`）共用。
 *
 * **背景**：旧 `subtractOverlayOwnedRanges()` 对"部分重叠"直接把整个 reflow 丢掉
 * （返回 `emptyList`），而不是做真正的差集。同时首帧 handoff 的去重逻辑和 timeline
 * 的去重逻辑不一致，导致首帧可能把同一段文字画两遍。
 *
 * **本 helper 做真正的差集**：
 * 1. 从 `move.newRange` 中减去所有 `ownedRanges`，得到剩余的一个或多个 newRange slice；
 * 2. 每个 new slice 按相对偏移算对应 oldRange；
 * 3. old/new bounds 都重新从真实 layout 取（不按原整块 Rect 比例切，文字宽度不是等比例的）；
 * 4. bounds 取不到（返回 null）或位置没变化的 slice 跳过；
 * 5. 返回只包含没有被 owned 且位置真变化的 slice 列表。
 */
internal object ComposeOverlayOwnership {
    /**
     * 从 [move] 中减去已被其他 active unit 接管的范围，返回剩余的 reflow moves。
     *
     * @param move 原始 reflow move（包含 oldRange/newRange/oldBounds/newBounds）。
     * @param ownedRanges 已被其他 active unit 接管的 targetRange 列表。
     * @param oldLayout 旧布局快照（取 oldBounds）。
     * @param newLayout 新布局快照（取 newBounds）。
     * @return 只包含没有被 owned 且位置真变化的 [ComposeReflowMove] slice 列表。
     */
    fun subtractOwnedRanges(
        move: ComposeReflowMove,
        ownedRanges: List<TextRange>,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
    ): List<ComposeReflowMove> {
        val newRange = move.newRange
        if (newRange.start >= newRange.end) return emptyList()

        // 步骤1：从 newRange 中真正减去所有 ownedRanges，得到剩余的 newRange slices
        val newSlices = ComposeVisualRebase.subtractRanges(listOf(newRange), ownedRanges)
        if (newSlices.isEmpty()) return emptyList()

        val result = mutableListOf<ComposeReflowMove>()
        for (newSlice in newSlices) {
            if (newSlice.start >= newSlice.end) continue

            // 步骤2：按相对偏移算对应 oldRange
            val deltaStart = newSlice.start - move.newRange.start
            val deltaEnd = newSlice.end - move.newRange.start
            val oldSlice =
                TextRange(
                    move.oldRange.start + deltaStart,
                    move.oldRange.start + deltaEnd,
                )

            // 步骤3：old/new bounds 都重新从真实 layout 取
            val oldBounds = ComposeVisualRebase.safePathBounds(oldLayout.result, oldSlice)
            val newBounds = ComposeVisualRebase.safePathBounds(newLayout.result, newSlice)

            // 步骤4：bounds 取不到则跳过该 slice
            if (oldBounds == null || newBounds == null) continue

            // 步骤5：位置没有变化则跳过该 slice
            if (oldBounds.left == newBounds.left && oldBounds.top == newBounds.top) continue

            result.add(
                ComposeReflowMove(
                    oldRange = oldSlice,
                    newRange = newSlice,
                    oldBounds = oldBounds,
                    newBounds = newBounds,
                ),
            )
        }
        return result
    }
}
