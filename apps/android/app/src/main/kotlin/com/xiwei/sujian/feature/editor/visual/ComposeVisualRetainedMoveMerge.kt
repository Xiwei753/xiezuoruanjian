package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.rawLineEndForRawOffset

/**
 * retained move 的相邻段合并状态机。
 *
 * [ComposeVisualRebase.computeRetainedMoves] 按视觉行切出 move 段后，由这里决定
 * 相邻段是合并成一个 move 还是各自成段。原先这段状态机与切片切分、offset map 合成
 * 混在同一个 1372 行文件里，拆出来后本文件只依赖 [ComposeVisualGeometry] 与纯数据。
 */
internal object ComposeVisualRetainedMoveMerge {
    /**
     * Retained moves 计算上下文 — 封装循环中不变的参数。
     *
     * Issue #717 评论 5742904417 修复1：oldText/newText 类型从 AnnotatedString 改成 CharSequence，
     * 因为现在传的是 String（rawText），但 ctx.oldText[segEnd-1].isHighSurrogate() 需要 CharSequence 索引，
     * String 和 AnnotatedString 都支持。
     */
    data class RetainedMovesContext(
        val prev: ComposeLayoutSnapshot,
        val curr: ComposeLayoutSnapshot,
        val oldText: CharSequence,
        val newText: CharSequence,
        val oldTextLen: Int,
        val newTextLen: Int,
        val oldSuffixStart: Int,
        val newSuffixStart: Int,
    )

    fun computeRetainedMovesLoop(ctx: RetainedMovesContext): List<RetainedMove> {
        val result = mutableListOf<RetainedMove>()
        var oldPos = ctx.oldSuffixStart
        val mergeState = MergeState()

        while (oldPos < ctx.oldTextLen) {
            val moveResult = processRetainedMoveSegment(ctx, oldPos)
            val newPos = oldPos - ctx.oldSuffixStart + ctx.newSuffixStart
            oldPos = updateMoveResult(result, moveResult, mergeState, oldPos, newPos)
        }

        flushPendingMove(result, mergeState, oldPos, ctx)
        return result
    }

    class MergeState {
        var mergedOldStart: Int = -1
        var mergedNewStart: Int = -1
        var mergedDx: Float = 0f
        var mergedDy: Float = 0f
        var merging: Boolean = false
    }

    data class RetainedMoveSegmentResult(
        val segEnd: Int,
        val oldBounds: Rect?,
        val newBounds: Rect?,
    )

    fun processRetainedMoveSegment(
        ctx: RetainedMovesContext,
        oldPos: Int,
    ): RetainedMoveSegmentResult {
        // Issue #717 评论 5742904417 修复2：lineEnd 转回 raw 坐标。
        // getLineEnd 返回 display offset（含 U+200B），retained move 切片需要 raw offset。
        // ctx.oldTextLen 现在是 rawText.length，oldLineEnd 也是 raw offset，正确。
        val oldLineEnd = ctx.prev.rawLineEndForRawOffset(oldPos)
        var segEnd = minOf(oldLineEnd, ctx.oldTextLen)
        if (segEnd in 1 until ctx.oldTextLen &&
            ctx.oldText[segEnd - 1].isHighSurrogate() &&
            ctx.oldText[segEnd].isLowSurrogate()
        ) {
            segEnd -= 1
        }
        if (segEnd <= oldPos) segEnd = oldPos + 1

        val newPos = oldPos - ctx.oldSuffixStart + ctx.newSuffixStart
        val newSegEnd = segEnd - ctx.oldSuffixStart + ctx.newSuffixStart

        return if (newSegEnd > ctx.newTextLen) {
            RetainedMoveSegmentResult(segEnd, null, null)
        } else {
            val oldRange = TextRange(oldPos, segEnd)
            val newRange = TextRange(newPos, newSegEnd)
            val oldBounds = ComposeVisualGeometry.safePathBounds(ctx.prev, oldRange)
            val newBounds = ComposeVisualGeometry.safePathBounds(ctx.curr, newRange)
            RetainedMoveSegmentResult(segEnd, oldBounds, newBounds)
        }
    }

    fun updateMoveResult(
        result: MutableList<RetainedMove>,
        segmentResult: RetainedMoveSegmentResult,
        mergeState: MergeState,
        oldPos: Int,
        newPos: Int,
    ): Int {
        val segEnd = segmentResult.segEnd
        val oldBounds = segmentResult.oldBounds
        val newBounds = segmentResult.newBounds

        if (oldBounds != null && newBounds != null) {
            handleBoundsChanged(result, oldPos, oldBounds, newBounds, mergeState, newPos)
        } else {
            handleBoundsNull(result, oldPos, mergeState, newPos)
        }
        return segEnd
    }

    private fun handleBoundsChanged(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        oldBounds: Rect,
        newBounds: Rect,
        mergeState: MergeState,
        newPos: Int,
    ) {
        val dx = newBounds.left - oldBounds.left
        val dy = newBounds.top - oldBounds.top
        val topChanged = kotlin.math.abs(dy) > 1f
        val leftChanged = kotlin.math.abs(dx) > 1f
        if (topChanged || leftChanged) {
            handlePositionChanged(result, oldPos, dx, dy, mergeState, newPos)
        } else {
            handlePositionUnchanged(result, oldPos, mergeState, newPos)
        }
    }

    private fun handlePositionChanged(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        dx: Float,
        dy: Float,
        mergeState: MergeState,
        newPos: Int,
    ) {
        if (mergeState.merging &&
            kotlin.math.abs(dx - mergeState.mergedDx) <= 1f &&
            kotlin.math.abs(dy - mergeState.mergedDy) <= 1f
        ) {
            // 位移向量一致，继续合并。
        } else if (mergeState.merging) {
            finishCurrentMerge(result, oldPos, mergeState, newPos)
            startNewMerge(oldPos, dx, dy, mergeState, newPos)
        } else {
            startNewMerge(oldPos, dx, dy, mergeState, newPos)
        }
    }

    private fun handlePositionUnchanged(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        mergeState: MergeState,
        newPos: Int,
    ) {
        if (mergeState.merging) {
            finishCurrentMerge(result, oldPos, mergeState, newPos)
            mergeState.merging = false
        }
    }

    private fun handleBoundsNull(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        mergeState: MergeState,
        newPos: Int,
    ) {
        if (mergeState.merging) {
            finishCurrentMerge(result, oldPos, mergeState, newPos)
            mergeState.merging = false
        }
    }

    private fun finishCurrentMerge(
        result: MutableList<RetainedMove>,
        oldPos: Int,
        mergeState: MergeState,
        newPos: Int,
    ) {
        result.add(
            RetainedMove(
                oldRange = TextRange(mergeState.mergedOldStart, oldPos),
                newRange = TextRange(mergeState.mergedNewStart, newPos),
            ),
        )
    }

    private fun startNewMerge(
        oldPos: Int,
        dx: Float,
        dy: Float,
        mergeState: MergeState,
        newPos: Int,
    ) {
        mergeState.mergedOldStart = oldPos
        mergeState.mergedNewStart = newPos
        mergeState.mergedDx = dx
        mergeState.mergedDy = dy
        mergeState.merging = true
    }

    fun flushPendingMove(
        result: MutableList<RetainedMove>,
        mergeState: MergeState,
        oldPos: Int,
        ctx: RetainedMovesContext,
    ) {
        if (mergeState.merging) {
            val newPos = oldPos - ctx.oldSuffixStart + ctx.newSuffixStart
            if (newPos <= ctx.newTextLen) {
                result.add(
                    RetainedMove(
                        oldRange = TextRange(mergeState.mergedOldStart, oldPos),
                        newRange = TextRange(mergeState.mergedNewStart, newPos),
                    ),
                )
            }
        }
    }
}
