package com.xiwei.sujian.feature.editor.visual

import androidx.compose.animation.core.Animatable
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
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.drawText
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/**
 * #641 评论1 第5节 / 问题3：动画 overlay — 只"画"，绝不能再改变 viewport / selection / IME 几何。
 *
 * 绘制规则：
 * - 新文字：从当前 [TextLayoutResult] 取 bounding box，当前 range 已被
 *   [OutputTransformation] 隐藏，overlay 做淡入/位移；
 * - 删除文字：保留上一份 [TextLayoutResult]，按旧 range 的 bounding box 画旧布局；
 * - 同行移动/自动折行/手动换行：old/new 坐标分别来自前后两份真实 [TextLayoutResult]；
 * - 视觉光标：从 `oldResult.getCursorRect(oldSelection.end)` 插值到
 *   `newResult.getCursorRect(newSelection.end)`，按 progress 插值 x/y/width/height。
 *
 * #641 评论 问题3：
 * - duration 来自 [com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy]（不再写死 200ms）；
 * - 用 [Animatable] 手动控制动画，transaction id 变化时重新建立正确起点；
 * - 新事务到来时先物化当前视觉帧作为下一事务起点，再 rebase；
 * - retained move 处理自动折行时被挤到下一行的"保留文字"；
 * - 用 `getPathForRange` 替代整行 clip，同一行没参与动画的文字不会被 overlay 再画一遍；
 * - cursor 颜色吃 [cursorColor]，同一帧只画一次 cursor（不再闪烁叠加）。
 *
 * 动画通过 Compose animation progress 只改变 alpha/translate/绘制，
 * 不 scrollTo、不改 selection/IME/height/viewport。动画结束清 hiddenRanges，
 * 系统正文马上可见。
 *
 * #641 评论1 第3节：overlay 文字颜色不能硬编码成与正文不一致的黑色，
 * 必须从 [WritingEditorSurface] 传入当前 textColor/字体 style。
 *
 * @param cursorColor 视觉光标颜色 — 从主题 role 注入，不再硬编码蓝色。
 */
@Composable
@Suppress("LongParameterList")
fun ComposeTextAnimationOverlay(
    visualState: ComposeEditorVisualState,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    modifier: Modifier = Modifier,
) {
    val hiddenRanges by visualState.hiddenRanges.collectAsStateWithLifecycle()
    val activeIntent by visualState.activeIntent.collectAsStateWithLifecycle()
    val activeTransaction by visualState.activeTransaction.collectAsStateWithLifecycle()
    val cursorSnapshotValue by visualState.visualCursorSnapshot.collectAsStateWithLifecycle()
    val density = LocalDensity.current
    val cursorSnapshot = remember(cursorSnapshotValue) { cursorSnapshotValue }

    val transactionId = activeTransaction?.id ?: 0L
    val durationMillis = activeTransaction?.durationMillis ?: 0L

    // #641 评论 问题3：用 Animatable 手动控制动画 —
    // transaction id 变化时 snapTo(0f) 重新建立正确起点，再 animateTo(1f)。
    // 不再用 animateFloatAsState(target=1f)，连续输入时不会为新事务重新建立正确起点。
    val progress = remember { Animatable(0f) }
    LaunchedEffect(transactionId) {
        if (transactionId > 0L && durationMillis > 0L) {
            progress.snapTo(0f)
            progress.animateTo(
                targetValue = 1f,
                animationSpec = tween(durationMillis = durationMillis.toInt()),
            )
        }
    }
    val progressValue = progress.value

    val hasAnimation =
        activeTransaction != null &&
            (hiddenRanges.isNotEmpty() || activeIntent?.cursor?.animate == true)

    // 动画结束清 hiddenRanges，系统正文马上可见。
    LaunchedEffect(transactionId, progressValue) {
        if (transactionId > 0L && progressValue >= 1f) {
            visualState.clearAnimation()
        }
    }

    Box(
        modifier =
            modifier
                .drawBehind {
                    if (!hasAnimation) return@drawBehind
                    val current = visualState.currentLayout() ?: return@drawBehind
                    val previous = visualState.previousLayout()
                    val intent = activeIntent ?: return@drawBehind
                    val transaction = activeTransaction ?: return@drawBehind

                    drawVisualTransaction(
                        currentResult = current.result,
                        previousResult = previous?.result,
                        transaction = transaction,
                        textKind = intent.textKind,
                        cursorAnimate = intent.cursor?.animate == true,
                        progress = progressValue,
                        scrollY = scrollY,
                        textColor = textColor,
                        cursorColor = cursorColor,
                        cursorSnapshot = cursorSnapshot,
                        density = density,
                    )
                },
    )
}

/**
 * #641 评论 问题3：绘制视觉动画事务 — 提取以降低 [ComposeTextAnimationOverlay] 的认知复杂度。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawVisualTransaction(
    currentResult: TextLayoutResult,
    previousResult: TextLayoutResult?,
    transaction: ComposeVisualTransaction,
    textKind: TextVisualKind,
    cursorAnimate: Boolean,
    progress: Float,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    cursorSnapshot: VisualCursorSnapshot?,
    density: androidx.compose.ui.unit.Density,
) {
    drawAnimatedRanges(
        currentResult = currentResult,
        previousResult = previousResult,
        oldRanges = transaction.oldRanges,
        newRanges = transaction.newRanges,
        retainedMoves = transaction.retainedMoves,
        textKind = textKind,
        progress = progress,
        scrollY = scrollY,
        textColor = textColor,
    )

    // 视觉光标：按 progress 从 old cursor rect 插值到 new cursor rect。
    // #641 评论 问题2：只要 cursor?.animate == true 就画（不管 textKind）。
    // #641 评论 问题3：同一帧只画一次 cursor（不再闪烁叠加）。
    if (cursorAnimate && cursorSnapshot != null) {
        drawVisualCursor(
            snapshot = cursorSnapshot,
            progress = progress,
            scrollY = scrollY,
            density = density,
            cursorColor = cursorColor,
        )
    }
}

/** 视觉光标宽度（dp）。 */
private val VisualCursorWidthDp: Dp = 2.dp

/**
 * #641 评论1 第5节 / 问题3：视觉光标插值绘制 — 从 [oldCursorRect] 按 progress 插值到
 * [newCursorRect]。光标宽度/高度随 rect 插值，颜色从 [cursorColor] 注入。
 * BasicTextField 的 cursorBrush 在动画期间透明，动画结束 clearAnimation 后恢复。
 *
 * #641 评论 问题3：同一帧只画一次 cursor — 删除 progress >= 0.85f 时的闪烁叠加。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawVisualCursor(
    snapshot: VisualCursorSnapshot,
    progress: Float,
    scrollY: Int,
    density: androidx.compose.ui.unit.Density,
    cursorColor: Color,
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
        color = cursorColor,
        topLeft = Offset(cursorLeft, cursorTop),
        size = Size(cursorRight - cursorLeft, cursorBottom - cursorTop),
    )
}

/** 线性插值 helper。 */
private fun lerp(
    a: Float,
    b: Float,
    t: Float,
): Float = a + (b - a) * t.coerceIn(0f, 1f)

/**
 * #641 评论1 第5节 / 问题3：对同一份 [TextLayoutResult] 做 `clipPath + drawText(result)`。
 *
 * #641 评论 问题3：用 `getPathForRange` 替代整行 clip —
 * 同一行没参与动画的文字不会被 overlay 再画一遍。
 * 官方 API：`TextLayoutResult.getPathForRange(start, end)` 返回 Path，用 `clipPath` 裁剪。
 *
 * #641 评论1 第3节：overlay 文字颜色不能硬编码成与正文不一致的黑色，
 * 必须从 [WritingEditorSurface] 传入当前 textColor。
 */
private fun DrawScope.drawRangeText(
    result: TextLayoutResult,
    range: TextRange,
    alpha: Float,
    scrollY: Int,
    textColor: Color,
) {
    if (range.start >= range.end) return
    if (range.end > result.layoutInput.text.length) return
    val path = result.getPathForRange(range.start, range.end)
    clipPath(path) {
        drawText(
            textLayoutResult = result,
            color = textColor,
            topLeft = Offset(0f, -scrollY.toFloat()),
            alpha = alpha,
        )
    }
}

/**
 * #641 评论1 第5节 / 问题3：绘制受影响 range 的动画过程。
 *
 * #641 评论 问题3：
 * - Insert：用 newRanges，从 current layout 淡入。
 * - Delete：用 oldRanges，从 previous layout 淡出。
 * - Move：用 oldRanges 从 previous layout 淡出 + newRanges 从 current layout 淡入；
 *   retained moves 处理自动折行时被挤到下一行的"保留文字"。
 * - None：不画文字动画。
 *
 * #641 评论1 第3节：overlay 文字颜色不能硬编码成与正文不一致的黑色，
 * 必须从 [WritingEditorSurface] 传入当前 textColor。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawAnimatedRanges(
    currentResult: TextLayoutResult,
    previousResult: TextLayoutResult?,
    oldRanges: List<TextRange>,
    newRanges: List<TextRange>,
    retainedMoves: List<RetainedMove>,
    textKind: TextVisualKind,
    progress: Float,
    scrollY: Int,
    textColor: Color,
) {
    when (textKind) {
        TextVisualKind.Insert -> {
            val alpha = progress
            for (range in newRanges) {
                drawRangeText(currentResult, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
            }
        }
        TextVisualKind.Delete -> {
            val alpha = 1f - progress
            val result = previousResult ?: currentResult
            for (range in oldRanges) {
                drawRangeText(result, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
            }
        }
        TextVisualKind.Move -> {
            // Move：从 previous layout 的 old range 位置淡出，
            // 从 current layout 的 new range 位置淡入。
            // 若 previous layout 缺失，则只在 current 位置淡入。
            val alpha = progress
            if (previousResult != null) {
                for (range in oldRanges) {
                    drawRangeText(previousResult, range, alpha = 1f - alpha, scrollY = scrollY, textColor = textColor)
                }
            }
            for (range in newRanges) {
                drawRangeText(currentResult, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
            }
            // #641 评论 问题3：retained moves — 被挤到下一行的"保留文字"。
            drawRetainedMoves(
                previousResult = previousResult,
                currentResult = currentResult,
                retainedMoves = retainedMoves,
                alpha = alpha,
                scrollY = scrollY,
                textColor = textColor,
            )
        }
        TextVisualKind.None -> {
            // 没有文字动画（如 CURSOR_ONLY 事务）。
        }
    }
}

/**
 * #641 评论 问题3：绘制 retained moves — 被挤到下一行的"保留文字"。
 * 提取以降低 [drawAnimatedRanges] 的认知复杂度。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawRetainedMoves(
    previousResult: TextLayoutResult?,
    currentResult: TextLayoutResult,
    retainedMoves: List<RetainedMove>,
    alpha: Float,
    scrollY: Int,
    textColor: Color,
) {
    for (move in retainedMoves) {
        if (previousResult != null) {
            drawRangeText(
                previousResult,
                move.oldRange,
                alpha = 1f - alpha,
                scrollY = scrollY,
                textColor = textColor,
            )
        }
        drawRangeText(
            currentResult,
            move.newRange,
            alpha = alpha,
            scrollY = scrollY,
            textColor = textColor,
        )
    }
}
