package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult

/**
 * Issue #737：光标遍历路径 — 根据 old/new [TextLayoutResult] 生成光标运动 segments。
 *
 * caret traversal 是唯一主运动，吞字/吐字从同一份 traversal 的 progress 派生。
 * 跨自动换行时必须走分段 traversal，不能从上一行末尾直接直线插值到下一行开头，
 * 否则光标会斜穿过两行之间的空白区域。
 *
 * 所有坐标必须直接来自平台 layout（[TextLayoutResult.getCursorRect] /
 * [TextLayoutResult.getLineLeft] / [TextLayoutResult.getLineRight] 等），
 * 不由动画层自行猜位置。
 *
 * - [isValid] = false 时表示无运动（old/new caret rect 相同），
 *   对应的 [CoordinatedEditMotion] 也无效，直接显示最终静态正文。
 * - [segments] 按行有序，每个 segment 属于同一行；
 *   master progress [0,1] 跨 segment 时按 [Segment.startProgress]/[Segment.endProgress] 区间映射。
 *
 * @param segments 有序光标运动段列表（按行）。
 * @param isValid 是否存在有效运动。old/new caret rect 相同时为 false。
 */
class CaretTraversal(
    val segments: List<Segment>,
    val isValid: Boolean,
) {
    /**
     * 一个光标运动段：起点 rect → 终点 rect，属于同一行。
     *
     * @param startRect 起点 caret rect（来自平台 layout）。
     * @param endRect 终点 caret rect（来自平台 layout）。
     * @param lineIndex 行号（在所属 layout 中的行索引）。
     * @param startProgress 在 master progress [0,1] 中的起点。
     * @param endProgress 在 master progress [0,1] 中的终点。
     */
    data class Segment(
        val startRect: Rect,
        val endRect: Rect,
        val lineIndex: Int,
        val startProgress: Float,
        val endProgress: Float,
    )

    companion object {
        /**
         * 根据 old/new layout + old/new caret offset 和 rect 生成 traversal。
         *
         * - old/new caret rect 相同：[isValid] = false（无运动），[segments] 为空。
         * - 同一行：一个 segment（oldCaretRect → newCaretRect）。
         * - 跨软换行：上一行 oldCaretRect → 上一行行尾 rect；下一行行首 rect → newCaretRect。
         * - 多行编辑：按经过的实际行生成有序 segments，中间行各一个 segment（行首→行尾）。
         * - 所有坐标直接来自平台 layout（[TextLayoutResult.getCursorRect] /
         *   [TextLayoutResult.getLineRight] 等）。
         *
         * @param oldLayout 编辑前平台 layout。
         * @param newLayout 编辑后平台 layout。
         * @param oldCaretOffset 编辑前 caret offset（UTF-16）。
         * @param newCaretOffset 编辑后 caret offset（UTF-16）。
         * @param oldCaretRect 编辑前 caret rect（来自 oldLayout）。
         * @param newCaretRect 编辑后 caret rect（来自 newLayout）。
         */
        fun fromLayouts(
            oldLayout: TextLayoutResult,
            newLayout: TextLayoutResult,
            oldCaretOffset: Int,
            newCaretOffset: Int,
            oldCaretRect: Rect,
            newCaretRect: Rect,
        ): CaretTraversal {
            // 无运动：old/new caret rect 相同
            if (oldCaretRect == newCaretRect) {
                return CaretTraversal(segments = emptyList(), isValid = false)
            }
            val oldLine = lineForOffset(oldLayout, oldCaretOffset)
            val newLine = lineForOffset(newLayout, newCaretOffset)
            // 同行或无法确定行：单 segment 直线插值
            if (oldLine == newLine || oldLine < 0 || newLine < 0) {
                return CaretTraversal(
                    segments =
                        listOf(
                            Segment(
                                startRect = oldCaretRect,
                                endRect = newCaretRect,
                                lineIndex = maxOf(oldLine, newLine, 0),
                                startProgress = 0f,
                                endProgress = 1f,
                            ),
                        ),
                    isValid = true,
                )
            }
            // 跨行：分段 traversal
            return CaretTraversal(
                segments =
                    buildCrossLineSegments(
                        oldLayout,
                        newLayout,
                        oldLine,
                        newLine,
                        oldCaretRect,
                        newCaretRect,
                    ),
                isValid = true,
            )
        }

        /**
         * 跨行分段 traversal — 上一行 oldCaretRect → 行尾；下一行行首 → newCaretRect；
         * 跨多行时中间行各一个 segment（行首→行尾）。
         */
        private fun buildCrossLineSegments(
            oldLayout: TextLayoutResult,
            newLayout: TextLayoutResult,
            oldLine: Int,
            newLine: Int,
            oldCaretRect: Rect,
            newCaretRect: Rect,
        ): List<Segment> {
            val segments = mutableListOf<Segment>()
            val minLine = minOf(oldLine, newLine)
            val maxLine = maxOf(oldLine, newLine)
            val totalLines = maxLine - minLine + 1
            val progressPerLine = 1f / totalLines.toFloat()
            val oldIsFirst = oldLine < newLine
            // 第一段：从 oldCaretRect 到行尾
            addFirstSegment(
                segments, oldIsFirst, oldLayout, newLayout,
                oldLine, newLine, oldCaretRect, newCaretRect, progressPerLine,
            )
            // 中间行
            addMiddleSegments(segments, minLine, oldLine, oldLayout, newLayout, totalLines, progressPerLine)
            // 最后一段：从行首到 newCaretRect
            addLastSegment(
                segments, oldIsFirst, oldLayout, newLayout,
                oldLine, newLine, oldCaretRect, newCaretRect, totalLines, progressPerLine,
            )
            return segments
        }

        @Suppress("LongParameterList")
        private fun addFirstSegment(
            segments: MutableList<Segment>,
            oldIsFirst: Boolean,
            oldLayout: TextLayoutResult,
            newLayout: TextLayoutResult,
            oldLine: Int,
            newLine: Int,
            oldCaretRect: Rect,
            newCaretRect: Rect,
            progressPerLine: Float,
        ) {
            val firstLayout = if (oldIsFirst) oldLayout else newLayout
            val firstLine = if (oldIsFirst) oldLine else newLine
            val firstStartRect = if (oldIsFirst) oldCaretRect else newCaretRect
            segments.add(
                Segment(
                    startRect = firstStartRect,
                    endRect = lineEndRect(firstLayout, firstLine),
                    lineIndex = firstLine,
                    startProgress = 0f,
                    endProgress = progressPerLine,
                ),
            )
        }

        @Suppress("LongParameterList")
        private fun addMiddleSegments(
            segments: MutableList<Segment>,
            minLine: Int,
            oldLine: Int,
            oldLayout: TextLayoutResult,
            newLayout: TextLayoutResult,
            totalLines: Int,
            progressPerLine: Float,
        ) {
            for (i in 1 until totalLines - 1) {
                val line = minLine + i
                val layout = if (line <= oldLine) oldLayout else newLayout
                segments.add(
                    Segment(
                        startRect = lineStartRect(layout, line),
                        endRect = lineEndRect(layout, line),
                        lineIndex = line,
                        startProgress = i * progressPerLine,
                        endProgress = (i + 1) * progressPerLine,
                    ),
                )
            }
        }

        @Suppress("LongParameterList")
        private fun addLastSegment(
            segments: MutableList<Segment>,
            oldIsFirst: Boolean,
            oldLayout: TextLayoutResult,
            newLayout: TextLayoutResult,
            oldLine: Int,
            newLine: Int,
            oldCaretRect: Rect,
            newCaretRect: Rect,
            totalLines: Int,
            progressPerLine: Float,
        ) {
            val lastLayout = if (oldIsFirst) newLayout else oldLayout
            val lastLine = if (oldIsFirst) newLine else oldLine
            segments.add(
                Segment(
                    startRect = lineStartRect(lastLayout, lastLine),
                    endRect = if (oldIsFirst) newCaretRect else oldCaretRect,
                    lineIndex = lastLine,
                    startProgress = (totalLines - 1) * progressPerLine,
                    endProgress = 1f,
                ),
            )
        }

        /**
         * 找出 offset 所在行号；offset 越界时返回 -1。
         */
        private fun lineForOffset(
            layout: TextLayoutResult,
            offset: Int,
        ): Int {
            if (offset < 0 || offset > layout.layoutInput.text.text.length) return -1
            for (i in 0 until layout.lineCount) {
                if (offset >= layout.getLineStart(i) && offset <= layout.getLineEnd(i)) return i
            }
            return layout.lineCount - 1
        }

        /**
         * 行尾 caret rect — 用行右边界构造一个 caret 大小的 rect。
         */
        private fun lineEndRect(
            layout: TextLayoutResult,
            line: Int,
        ): Rect {
            val top = layout.getLineTop(line)
            val bottom = layout.getLineBottom(line)
            val right = layout.getLineRight(line)
            val left = layout.getLineLeft(line)
            // 行尾 caret rect：用行高 * 0.6 作为 caret 宽度估计
            return Rect(left = right, top = top, right = right + (bottom - top) * 0.6f, bottom = bottom)
        }

        /**
         * 行首 caret rect — 用行左边界构造一个 caret 大小的 rect。
         */
        private fun lineStartRect(
            layout: TextLayoutResult,
            line: Int,
        ): Rect {
            val top = layout.getLineTop(line)
            val bottom = layout.getLineBottom(line)
            val left = layout.getLineLeft(line)
            return Rect(left = left, top = top, right = left + (bottom - top) * 0.6f, bottom = bottom)
        }
    }

    /**
     * 根据 master progress [0,1] 算当前 caret rect。
     *
     * 找到 progress 落在哪个 segment 的 [Segment.startProgress, Segment.endProgress] 区间，
     * 在该 segment 内线性插值。progress <= 0 返回首段起点；progress >= 1 返回末段终点。
     *
     * @param progress master progress [0,1]。
     * @return 当前帧的 caret rect；segments 为空时返回 [Rect.Zero]。
     */
    fun sampleCaret(progress: Float): Rect {
        if (segments.isEmpty()) return Rect.Zero
        if (progress <= 0f) return segments.first().startRect
        if (progress >= 1f) return segments.last().endRect
        for (seg in segments) {
            if (progress >= seg.startProgress && progress <= seg.endProgress) {
                val span = seg.endProgress - seg.startProgress
                if (span <= 0f) return seg.endRect
                val t = (progress - seg.startProgress) / span
                return lerpRect(seg.startRect, seg.endRect, t)
            }
        }
        return segments.last().endRect
    }

    /**
     * 线性插值两个 [Rect]。
     */
    private fun lerpRect(
        from: Rect,
        to: Rect,
        t: Float,
    ): Rect {
        if (t <= 0f) return from
        if (t >= 1f) return to
        return Rect(
            left = from.left + (to.left - from.left) * t,
            top = from.top + (to.top - from.top) * t,
            right = from.right + (to.right - from.right) * t,
            bottom = from.bottom + (to.bottom - from.bottom) * t,
        )
    }
}
