package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.style.ResolvedTextDirection
import kotlin.math.abs

/**
 * Issue #737：光标遍历路径 — 根据 old/new [TextLayoutResult] 生成光标运动 segments。
 *
 * caret traversal 是唯一主运动，吞字/吐字从同一份 traversal 的 progress 派生。
 * 跨自动换行时必须走分段 traversal，不能从上一行末尾直接直线插值到下一行开头，
 * 否则光标会斜穿过两行之间的空白区域。
 *
 * 所有坐标必须直接来自平台 layout（[TextLayoutResult.getCursorRect] /
 * [TextLayoutResult.getLineForOffset] 等），不由动画层自行猜位置。
 *
 * - [isValid] = false 时表示无运动（old/new caret rect 相同），
 *   对应的 [CoordinatedEditMotion] 也无效，直接显示最终静态正文。
 * - [segments] 按行有序，每个 segment 属于同一行；
 *   master progress [0,1] 跨 segment 时按 [Segment.startProgress]/[Segment.endProgress] 区间映射。
 *
 * Issue #737 评论 5781084709 修复点 6：暴露 [oldLine] / [newLine]，
 * 供 [CoordinatedEditMotion] 取真实行号，不再固定传 -1。
 *
 * @param segments 有序光标运动段列表（按行）。
 * @param isValid 是否存在有效运动。old/new caret rect 相同时为 false。
 * @param oldLine 编辑前 caret 所在行（-1 表示未确定/无运动）。
 * @param newLine 编辑后 caret 所在行（-1 表示未确定/无运动）。
 */
class CaretTraversal(
    val segments: List<Segment>,
    val isValid: Boolean,
    val oldLine: Int,
    val newLine: Int,
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
                return CaretTraversal(
                    segments = emptyList(),
                    isValid = false,
                    oldLine = -1,
                    newLine = -1,
                )
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
                    oldLine = oldLine,
                    newLine = newLine,
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
                oldLine = oldLine,
                newLine = newLine,
            )
        }

        /**
         * Issue #737 评论 5781084709 修复点 1：跨行分段 traversal —
         * segments 永远按"old caret → new caret"的实际时间顺序构造，
         * 不按 minLine/maxLine 先排序再决定起终点。
         *
         * - 向下移动（oldLine < newLine）：
         *   - 段 0 (oldLine): oldCaretRect → oldLine 行尾 caretRect
         *   - 中间行 (oldLine+1 .. newLine-1): 行首 caretRect → 行尾 caretRect
         *   - 最后段 (newLine): newLine 行首 caretRect → newCaretRect
         * - 向上移动（oldLine > newLine）：
         *   - 段 0 (oldLine): oldCaretRect → oldLine 行首 caretRect（向左到行首）
         *   - 中间行 (oldLine-1 .. newLine+1, 递减): 行尾 caretRect → 行首 caretRect（从右到左）
         *   - 最后段 (newLine): newLine 行尾 caretRect → newCaretRect
         *
         * progress 区间均分：每段占 1f / totalLines。
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
            val totalLines = abs(oldLine - newLine) + 1
            val progressPerLine = 1f / totalLines.toFloat()

            if (oldLine < newLine) {
                // 向下移动：oldLine → newLine
                // 段 0 (oldLine): oldCaretRect → oldLine 行尾
                segments.add(
                    Segment(
                        startRect = oldCaretRect,
                        endRect = lineEndRect(oldLayout, oldLine),
                        lineIndex = oldLine,
                        startProgress = 0f,
                        endProgress = progressPerLine,
                    ),
                )
                // 中间行 (oldLine+1 .. newLine-1): 行首 → 行尾
                for (i in 1 until totalLines - 1) {
                    val line = oldLine + i
                    segments.add(
                        Segment(
                            startRect = lineStartRect(newLayout, line),
                            endRect = lineEndRect(newLayout, line),
                            lineIndex = line,
                            startProgress = i * progressPerLine,
                            endProgress = (i + 1) * progressPerLine,
                        ),
                    )
                }
                // 最后段 (newLine): newLine 行首 → newCaretRect
                segments.add(
                    Segment(
                        startRect = lineStartRect(newLayout, newLine),
                        endRect = newCaretRect,
                        lineIndex = newLine,
                        startProgress = (totalLines - 1) * progressPerLine,
                        endProgress = 1f,
                    ),
                )
            } else {
                // 向上移动：oldLine → newLine (oldLine > newLine)
                // 段 0 (oldLine): oldCaretRect → oldLine 行首（向左到行首）
                segments.add(
                    Segment(
                        startRect = oldCaretRect,
                        endRect = lineStartRect(oldLayout, oldLine),
                        lineIndex = oldLine,
                        startProgress = 0f,
                        endProgress = progressPerLine,
                    ),
                )
                // 中间行 (oldLine-1 .. newLine+1, 递减): 行尾 → 行首（从右到左）
                for (i in 1 until totalLines - 1) {
                    val line = oldLine - i
                    segments.add(
                        Segment(
                            startRect = lineEndRect(oldLayout, line),
                            endRect = lineStartRect(oldLayout, line),
                            lineIndex = line,
                            startProgress = i * progressPerLine,
                            endProgress = (i + 1) * progressPerLine,
                        ),
                    )
                }
                // 最后段 (newLine): newLine 行尾 → newCaretRect
                segments.add(
                    Segment(
                        startRect = lineEndRect(newLayout, newLine),
                        endRect = newCaretRect,
                        lineIndex = newLine,
                        startProgress = (totalLines - 1) * progressPerLine,
                        endProgress = 1f,
                    ),
                )
            }
            return segments
        }

        /**
         * Issue #737 评论 5781084709 修复点 2：用平台 API [TextLayoutResult.getLineForOffset]
         * 找出 offset 所在行号，不要自己用 offset <= getLineEnd(i) 扫描
         * （软换行边界 offset 容易判到上一行）。offset 越界时返回 -1。
         */
        private fun lineForOffset(
            layout: TextLayoutResult,
            offset: Int,
        ): Int {
            val textLength = layout.layoutInput.text.text.length
            if (offset < 0 || offset > textLength) return -1
            return layout.getLineForOffset(offset)
        }

        /**
         * Issue #737 评论 5781084709 修复点 2：行尾 caret rect —
         * 取该行实际 end offset 的 [TextLayoutResult.getCursorRect]，
         * 不再用 getLineRight + 行高×0.6 估算 caret 几何。
         *
         * Issue #737 评论 5781634285 修复点 1：行边界几何改用 line 身份 —
         * 不再把有歧义的 shared offset 丢回 [TextLayoutResult.getCursorRect]。
         * [TextLayoutResult.getLineEnd] 返回该行最后一个字符**之后**的 exclusive offset；
         * 软换行时它等于下一行的 [TextLayoutResult.getLineStart]，[TextLayoutResult.getCursorRect]
         * 对"正好等于下一行 start 的 offset"会归到下一行，导致"上一行行尾 rect"实际拿到
         * "下一行行首 rect"，跨软换行没有真正闭环。
         *
         * 新实现直接用 line + 平台几何决定行边界：
         * - top/bottom：[TextLayoutResult.getLineTop] / [TextLayoutResult.getLineBottom]
         * - LTR 行尾 x：[TextLayoutResult.getLineRight]（平台该行的 right 边界）
         * - RTL 行尾 x：[TextLayoutResult.getLineLeft]（平台该行的 left 边界）
         * - rect 保持零宽（left == right == x），不再估 caret 宽度
         */
        private fun lineEndRect(
            layout: TextLayoutResult,
            line: Int,
        ): Rect {
            val top = layout.getLineTop(line)
            val bottom = layout.getLineBottom(line)
            val isRtl = isLineRtl(layout, line)
            val x = if (isRtl) layout.getLineLeft(line) else layout.getLineRight(line)
            return Rect(left = x, top = top, right = x, bottom = bottom)
        }

        /**
         * Issue #737 评论 5781084709 修复点 2：行首 caret rect —
         * 取该行实际 start offset 的 [TextLayoutResult.getCursorRect]，
         * 不再用 getLineLeft + 行高×0.6 估算 caret 几何。
         *
         * Issue #737 评论 5781634285 修复点 1：行边界几何改用 line 身份 —
         * 与 [lineEndRect] 同理，行首也用 line + 平台几何决定，不再把 shared offset 丢回
         * [TextLayoutResult.getCursorRect]。LTR 行首 x = [TextLayoutResult.getLineLeft]，
         * RTL 行首 x = [TextLayoutResult.getLineRight]。
         */
        private fun lineStartRect(
            layout: TextLayoutResult,
            line: Int,
        ): Rect {
            val top = layout.getLineTop(line)
            val bottom = layout.getLineBottom(line)
            val isRtl = isLineRtl(layout, line)
            val x = if (isRtl) layout.getLineRight(line) else layout.getLineLeft(line)
            return Rect(left = x, top = top, right = x, bottom = bottom)
        }

        /**
         * Issue #737 评论 5781634285 修复点 1：判断行方向 —
         * 用 [TextLayoutResult.getParagraphDirection] 取该行起始 offset 的段落方向，
         * [ResolvedTextDirection.Rtl] 表示 RTL 行。
         */
        private fun isLineRtl(
            layout: TextLayoutResult,
            line: Int,
        ): Boolean {
            val startOffset = layout.getLineStart(line)
            return layout.getParagraphDirection(startOffset) == ResolvedTextDirection.Rtl
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
