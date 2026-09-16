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
