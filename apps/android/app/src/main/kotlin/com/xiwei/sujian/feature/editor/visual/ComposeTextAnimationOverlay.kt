package com.xiwei.sujian.feature.editor.visual

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.drawText
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/**
 * #641 评论1 第5节：动画 overlay — 只"画"，绝不能再改变 viewport / selection / IME 几何。
 *
 * 绘制规则：
 * - 新文字：从当前 [TextLayoutResult] 取 bounding box，当前 range 已被
 *   [OutputTransformation] 隐藏，overlay 做淡入/位移；
 * - 删除文字：保留上一份 [TextLayoutResult]，按旧 range 的 bounding box 画旧布局；
 * - 同行移动/自动折行/手动换行：old/new 坐标分别来自前后两份真实 [TextLayoutResult]；
 * - 视觉光标：从 `oldResult.getCursorRect(oldSelection.end)` 插值到
 *   `newResult.getCursorRect(newSelection.end)`，按 progress 插值 x/y/width/height。
 *
 * 绘制受影响 range 时，不用 `TextMeasurer` 再排一次。对同一份 [TextLayoutResult]
 * 做 `clipRect + drawText(result)`。一个 range 跨多行就按真实 layout 的行段拆成多个 clip rect。
 *
 * 动画通过 Compose animation progress 只改变 alpha/translate/绘制，
 * 不 scrollTo、不改 selection/IME/height/viewport。动画结束清 hiddenRanges，
 * 系统正文马上可见。
 */
@Composable
fun ComposeTextAnimationOverlay(
    visualState: ComposeEditorVisualState,
    scrollY: Int,
    modifier: Modifier = Modifier,
) {
    val hiddenRanges by visualState.hiddenRanges.collectAsStateWithLifecycle()
    val activeIntent by visualState.activeIntent.collectAsStateWithLifecycle()
    val cursorSnapshotValue by visualState.visualCursorSnapshot.collectAsStateWithLifecycle()
    val density = LocalDensity.current
    val cursorSnapshot = remember(cursorSnapshotValue) { cursorSnapshotValue }

    val hasAnimation = hiddenRanges.isNotEmpty() || activeIntent != null

    val progress by animateFloatAsState(
        targetValue = if (hasAnimation) 1f else 0f,
        animationSpec = tween(durationMillis = ANIMATION_DURATION_MS),
        label = "editorVisualAnimation",
    )

    // 动画结束清 hiddenRanges，系统正文马上可见。
    LaunchedEffect(hasAnimation, progress) {
        if (hasAnimation && progress >= 1f) {
            visualState.clearAnimation()
        }
    }

    Box(
        modifier =
            modifier.drawBehind {
                if (!hasAnimation) return@drawBehind
                val current = visualState.currentLayout() ?: return@drawBehind
                val previous = visualState.previousLayout()
                val intent = activeIntent ?: return@drawBehind

                drawAnimatedRanges(
                    currentResult = current.result,
                    previousResult = previous?.result,
                    hiddenRanges = hiddenRanges,
                    intent = intent,
                    progress = progress,
                    scrollY = scrollY,
                )

                // 视觉光标：按 progress 从 old cursor rect 插值到 new cursor rect。
                if (intent.kind == EditorVisualIntent.Kind.Cursor && cursorSnapshot != null) {
                    drawVisualCursor(
                        snapshot = cursorSnapshot,
                        progress = progress,
                        scrollY = scrollY,
                        density = density,
                    )
                }
            },
    )
}

private const val ANIMATION_DURATION_MS = 200

/** 视觉光标颜色 — 与当前设计系统一致（可随 text style 注入）。 */
private val VisualCursorColor = Color(0xFF1A73E8)

/** 视觉光标宽度（dp）。 */
private val VisualCursorWidthDp: Dp = 2.dp

/**
 * #641 评论1 第5节：视觉光标插值绘制 — 从 [oldCursorRect] 按 progress 插值到
 * [newCursorRect]。光标宽度/高度随 rect 插值，颜色固定为设计系统色。
 * BasicTextField 的 cursorBrush 在动画期间透明，动画结束 clearAnimation 后恢复。
 */
private fun DrawScope.drawVisualCursor(
    snapshot: VisualCursorSnapshot,
    progress: Float,
    scrollY: Int,
    density: androidx.compose.ui.unit.Density,
) {
    val oldRect = snapshot.oldCursorRect
    val newRect = snapshot.newCursorRect

    val interpolatedLeft = lerp(oldRect.left, newRect.left, progress)
    val interpolatedTop = lerp(oldRect.top, newRect.top, progress)
    val interpolatedBottom = lerp(oldRect.bottom, newRect.bottom, progress)
    val interpolatedWidth = lerp(oldRect.width, newRect.width, progress)

    val cursorWidth = density.run { VisualCursorWidthDp.toPx() }
    val cursorLeft = interpolatedLeft - cursorWidth / 2
    val cursorRight = cursorLeft + maxOf(cursorWidth, interpolatedWidth)

    val cursorTop = interpolatedTop - scrollY.toFloat()
    val cursorBottom = interpolatedBottom - scrollY.toFloat()

    if (cursorBottom <= 0f || cursorTop >= size.height) return
    if (cursorRight <= 0f || cursorLeft >= size.width) return

    drawRect(
        color = VisualCursorColor,
        topLeft = Offset(cursorLeft, cursorTop),
        size = Size(cursorRight - cursorLeft, cursorBottom - cursorTop),
    )

    // 闪烁：progress 接近 1 时淡出，提示动画即将结束。
    if (progress >= 0.85f) {
        val fadeAlpha = 1f - (progress - 0.85f) / 0.15f
        drawRect(
            color = VisualCursorColor.copy(alpha = fadeAlpha),
            topLeft = Offset(cursorLeft, cursorTop),
            size = Size(cursorRight - cursorLeft, cursorBottom - cursorTop),
        )
    }
}

/** 线性插值 helper。 */
private fun lerp(
    a: Float,
    b: Float,
    t: Float,
): Float = a + (b - a) * t.coerceIn(0f, 1f)

/**
 * #641 评论1 第5节：对同一份 [TextLayoutResult] 做 `clipRect + drawText(result)`。
 * 一个 range 跨多行就按真实 layout 的行段拆成多个 clip rect。
 */
private fun DrawScope.drawAnimatedRanges(
    currentResult: TextLayoutResult,
    previousResult: TextLayoutResult?,
    hiddenRanges: List<TextRange>,
    intent: EditorVisualIntent,
    progress: Float,
    scrollY: Int,
) {
    when (intent.kind) {
        EditorVisualIntent.Kind.Insert -> {
            val alpha = progress
            for (range in hiddenRanges) {
                drawRangeText(currentResult, range, alpha = alpha, scrollY = scrollY)
            }
        }
        EditorVisualIntent.Kind.Delete -> {
            val alpha = 1f - progress
            val result = previousResult ?: currentResult
            for (range in hiddenRanges) {
                drawRangeText(result, range, alpha = alpha, scrollY = scrollY)
            }
        }
        EditorVisualIntent.Kind.Move -> {
            // Move：从 previous layout 的 old range 位置淡出，
            // 从 current layout 的 new range 位置淡入。
            // 若 previous layout 缺失，则只在 current 位置淡入。
            val alpha = progress
            if (previousResult != null) {
                for (range in hiddenRanges) {
                    drawRangeText(previousResult, range, alpha = 1f - alpha, scrollY = scrollY)
                }
            }
            for (range in hiddenRanges) {
                drawRangeText(currentResult, range, alpha = alpha, scrollY = scrollY)
            }
        }
        EditorVisualIntent.Kind.Cursor -> {
            // 视觉光标由 drawVisualCursor 单独绘制（基于 cursorSnapshot 插值）。
        }
    }
}

/**
 * #641：按真实 layout 的行段拆成多个 clip rect，对每个行段做
 * `clipRect + drawText(layoutResult)`。drawText 使用同一份 [TextLayoutResult]，
 * 不用 TextMeasurer 再排一次。
 */
private fun DrawScope.drawRangeText(
    result: TextLayoutResult,
    range: TextRange,
    alpha: Float,
    scrollY: Int,
) {
    if (range.start >= range.end) return
    if (range.end > result.layoutInput.text.length) return
    val startLine = result.getLineForOffset(range.start)
    val endLine = result.getLineForOffset(range.end)
    for (line in startLine..endLine) {
        val lineTop = result.getLineTop(line) - scrollY
        val lineBottom = result.getLineBottom(line) - scrollY
        val lineLeft = result.getLineLeft(line)
        val lineRight = result.getLineRight(line)
        if (lineBottom <= 0f || lineTop >= size.height) continue
        clipRect(
            left = lineLeft,
            top = lineTop,
            right = lineRight,
            bottom = lineBottom,
        ) {
            drawText(
                textLayoutResult = result,
                color = Color.Black,
                topLeft = Offset(0f, -scrollY.toFloat()),
                alpha = alpha,
            )
        }
    }
}
