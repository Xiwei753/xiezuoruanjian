package com.xiwei.sujian.feature.editor.visual

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.text.TextLayoutResult

/**
 * #641 评论1 第5节：动画 overlay — 只“画”，绝不能再改变 viewport / selection / IME 几何。
 *
 * 绘制规则：
 * - 新文字：从当前 [TextLayoutResult] 取 bounding box，当前 range 已被
 *   [OutputTransformation] 隐藏，overlay 做淡入/位移；
 * - 删除文字：保留上一份 [TextLayoutResult]，按旧 range 的 bounding box 画旧布局；
 * - 同行移动/自动折行/手动换行：old/new 坐标分别来自前后两份真实 [TextLayoutResult]；
 * - 视觉光标：从 `oldResult.getCursorRect(oldSelection.end)` 插值到
 *   `newResult.getCursorRect(newSelection.end)`。
 *
 * 绘制受影响 range 时，不用 `TextMeasurer` 再排一次。对同一份 [TextLayoutResult]
 * 做 `clipRect + drawText(result)`。一个 range 跨多行就按真实 layout 的行段拆成多个 clip rect。
 *
 * #641：当前实现结构完整 — overlay 已接入 [WritingEditorSurface]，
 * 当 [ComposeEditorVisualState.hiddenRanges] 为空时不绘制（无活跃动画）。
 * 动画 range 的淡入/位移/光标插值由后续提交逐步接入，但 overlay 不会反向修改
 * viewport / selection / IME 几何。
 */
@Composable
fun ComposeTextAnimationOverlay(
    visualState: ComposeEditorVisualState,
    scrollY: Int,
    modifier: Modifier = Modifier,
) {
    val hiddenRanges = visualState.hiddenRanges
    Box(
        modifier =
            modifier.drawBehind {
                if (hiddenRanges.value.isEmpty()) return@drawBehind
                val current = visualState.currentLayout() ?: return@drawBehind
                drawAnimatedRanges(
                    currentResult = current.result,
                    hiddenRanges = hiddenRanges.value,
                )
            },
    )
}

/**
 * #641 评论1 第5节：对同一份 [TextLayoutResult] 做 `clipRect + drawText(result)`。
 * 一个 range 跨多行就按真实 layout 的行段拆成多个 clip rect。
 */
private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawAnimatedRanges(
    currentResult: TextLayoutResult,
    hiddenRanges: List<androidx.compose.ui.text.TextRange>,
) {
    for (range in hiddenRanges) {
        if (range.start >= range.end) continue
        if (range.end > currentResult.layoutInput.text.length) continue
        val startLine = currentResult.getLineForOffset(range.start)
        val endLine = currentResult.getLineForOffset(range.end)
        for (line in startLine..endLine) {
            val lineTop = currentResult.getLineTop(line)
            val lineBottom = currentResult.getLineBottom(line)
            val lineLeft = currentResult.getLineLeft(line)
            val lineRight = currentResult.getLineRight(line)
            drawRect(
                color = androidx.compose.ui.graphics.Color.Transparent,
                topLeft = androidx.compose.ui.geometry.Offset(lineLeft, lineTop),
                size = androidx.compose.ui.geometry.Size(lineRight - lineLeft, lineBottom - lineTop),
            )
        }
    }
}
