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
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.drawText
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import uniffi.writer_core.AnimationModeDto

/**
 * #641 评论1 第5节 / 问题3 + 评论 5457777142 问题2/问题4：动画 overlay —
 * 只"画"，绝不能再改变 viewport / selection / IME 几何。
 *
 * #644 评论 #684：绘制只能读 frozen transaction。
 * 删除对 `visualState.currentLayout()` / `visualState.previousLayout()` 的调用。
 * 绘制只允许读：
 * - [ComposeVisualTransaction.oldLayout]
 * - [ComposeVisualTransaction.newLayout]
 * - [ComposeVisualTransaction.startFrame]
 * - [ComposeVisualTransaction.retainedMoves]
 * - [ComposeVisualTransaction.cursorStartRect]
 * - [ComposeVisualTransaction.cursorEndRect]
 *
 * 协调动画只保留一个 master progress：
 * 文字 Insert/Delete/Move、retained reflow、cursor move、startFrame rebase
 * 都从同一个 progress 推导。
 *
 * @param cursorColor 视觉光标颜色 — 从主题 role 注入。
 */
@Composable
@Suppress("LongParameterList", "LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
fun ComposeTextAnimationOverlay(
    visualState: ComposeEditorVisualState,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    /**
     * #684 评论 5663032418 断点3：live selection — 直接从 [TextFieldState.selection] 读取，
     * 不再依赖 [ComposeLayoutSnapshot.selection]（只在 onTextLayout 时更新，纯 selection 变化会过期）。
     *
     * [TextFieldState.selection] 本身就是 Compose 可观察状态，selection 变化会驱动这里更新。
     */
    liveSelection: TextRange?,
    modifier: Modifier = Modifier,
) {
    val hiddenRanges by visualState.hiddenRanges.collectAsStateWithLifecycle()
    val activeIntent by visualState.activeIntent.collectAsStateWithLifecycle()
    val activeTransaction by visualState.activeTransaction.collectAsStateWithLifecycle()
    val cursorSnapshotValue by visualState.visualCursorSnapshot.collectAsStateWithLifecycle()
    val density = LocalDensity.current
    val cursorSnapshot = remember(cursorSnapshotValue) { cursorSnapshotValue }

    // #644 评论 5662132136 第2项：smooth cursor 接管后必须有静止光标绘制路径。
    // drawsVisualCursor 由设置/attach 生命周期决定（overlay 是否整个会话拥有光标）。
    // latestLayout 为最新权威布局，静止光标从中取 caret rect。
    val drawsVisualCursor by visualState.drawsVisualCursor.collectAsStateWithLifecycle()
    val latestLayout by visualState.latestLayout.collectAsStateWithLifecycle()

    val transactionId = activeTransaction?.id ?: 0L
    val motionPolicy = activeTransaction?.motionPolicy ?: EditorMotionPolicy()

    // #644 评论 #684（评论 #5660899405 第 5 项）：Core 的动画语义必须被真正消费 —
    // 正文视觉事务的 master timeline 直接使用冻结事务里的 durationMs；
    // AnimationModeDto.SYSTEM_SUPPRESSED 必须禁止正文自定义动画；
    // 设置层 motionPolicy 只负责总开关 / reduce motion，不覆盖 Core 已经决定好的本笔动画事实。
    val animationMode = activeIntent?.animationMode
    val systemSuppressed = animationMode == AnimationModeDto.SYSTEM_SUPPRESSED
    // #684 评论 5664636035 Bug1：textKind 从 transaction 读取（屏幕事务按最终净变化决定），
    // 不再从 activeIntent 读取（最后一笔 intent 的 textKind 不代表整条 chain 的净变化）。
    val transactionTextKind = activeTransaction?.textKind ?: TextVisualKind.None
    val isCursorOnly =
        transactionTextKind == TextVisualKind.None && activeIntent?.cursor?.animate == true
    val textEnabled =
        motionPolicy.textEnabled && !systemSuppressed && transactionTextKind != TextVisualKind.None
    val cursorEnabled = motionPolicy.cursorEnabled

    // 直接使用冻结事务的 durationMs 作为 master timeline；Core 已决定本笔时长。
    val durationMs = activeTransaction?.durationMs ?: 0L

    // #644 评论 #684：单 master progress，不再有三套独立时间线。
    val masterProgress = remember { Animatable(0f) }

    LaunchedEffect(transactionId) {
        if (transactionId > 0L && durationMs > 0L) {
            masterProgress.snapTo(0f)
            masterProgress.animateTo(
                targetValue = 1f,
                animationSpec = tween(durationMillis = durationMs.toInt()),
            )
        } else if (transactionId > 0L) {
            masterProgress.snapTo(1f)
        }
    }

    val masterProgressValue = masterProgress.value

    // 从 master progress 推导 textProgress / cursorProgress / rebaseProgress。
    val textProgressValue = if (textEnabled) masterProgressValue else 1f
    val cursorProgressValue =
        if (cursorEnabled && activeIntent?.cursor?.animate == true) {
            masterProgressValue
        } else {
            1f
        }
    val rebaseProgressValue = masterProgressValue

    // #644 评论 #684：报告单 master progress 给 visualState，供下一事务物化 startFrame。
    LaunchedEffect(transactionId, masterProgressValue) {
        if (transactionId > 0L) {
            visualState.reportProgress(masterProgressValue)
        }
    }

    val hasTextAnimation =
        activeTransaction != null && textEnabled &&
            (hiddenRanges.isNotEmpty() || transactionTextKind != TextVisualKind.None)
    val hasCursorAnimation =
        activeTransaction != null && cursorEnabled &&
            activeIntent?.cursor?.animate == true
    val startFrameHasSlices = activeTransaction?.startFrame?.slices?.isNotEmpty() == true
    val hasRebaseAnimation = activeTransaction != null && startFrameHasSlices && rebaseProgressValue < 1f
    val hasAnimation = hasTextAnimation || hasCursorAnimation || hasRebaseAnimation

    // 动画结束：先通知 coordinator 清 active（避免下一笔拿已结束的旧事务当当前事务），
    // 再清本地 overlay 状态。系统正文马上可见。
    LaunchedEffect(transactionId, masterProgressValue) {
        if (transactionId > 0L && masterProgressValue >= 1f) {
            visualState.completeActiveTransaction(transactionId)
            visualState.clearAnimation()
        }
    }

    Box(
        modifier =
            modifier
                .drawBehind {
                    // 1. 动画帧：文字 Insert/Delete/Move、retained reflow、光标插值、startFrame rebase。
                    //    绘制只读 frozen transaction，不再读 currentLayout/previousLayout。
                    if (hasAnimation) {
                        val transaction = activeTransaction
                        val currentResult = transaction?.newLayout?.result
                        val previousResult = transaction?.oldLayout?.result
                        val intent = activeIntent
                        if (transaction != null && currentResult != null && intent != null) {
                            drawVisualTransaction(
                                currentResult = currentResult,
                                previousResult = previousResult,
                                transaction = transaction,
                                // #684 评论 5664636035 Bug1：绘制正文时读取 transaction.textKind，
                                // 不再读 activeIntent 的 textKind（屏幕事务的 textKind 按最终净变化决定）。
                                textKind = transaction.textKind,
                                // smooth cursor 关闭时系统光标负责绘制，overlay 不画光标动画。
                                cursorAnimate =
                                    drawsVisualCursor &&
                                        intent.cursor?.animate == true && cursorEnabled,
                                textProgress = textProgressValue,
                                cursorProgress = cursorProgressValue,
                                scrollY = scrollY,
                                textColor = textColor,
                                cursorColor = cursorColor,
                                cursorSnapshot = cursorSnapshot,
                                density = density,
                                textEnabled = textEnabled,
                                rebaseProgress = rebaseProgressValue,
                            )
                        }
                    }

                    // 2. 静止光标：smooth cursor 开启时 overlay 整个会话拥有光标。
                    //    动画进行中已在第 1 步按 old→new 插值画过，这里只在无光标动画时补 resting caret。
                    //    刚 attach、两次输入之间、纯等待、动画结束（clearAnimation 后）都画静止光标，
                    //    不会因为没有 active transaction 而丢失光标。
                    //    #684 评论 5663032418 断点3：光标 offset 从 live TextFieldState.selection 读取，
                    //    不再依赖 latestLayout.selection（只在 onTextLayout 时更新，纯 selection 变化会过期）。
                    if (drawsVisualCursor && !hasCursorAnimation) {
                        val restingRect = computeRestingCursorRect(latestLayout, liveSelection) ?: return@drawBehind
                        drawVisualCursor(
                            startRect = restingRect,
                            newRect = restingRect,
                            progress = 1f,
                            scrollY = scrollY,
                            density = density,
                            cursorColor = cursorColor,
                        )
                    }
                },
    )
}

/**
 * #641 评论 5459896691：绘制视觉动画事务 —
 * 提取以降低 [ComposeTextAnimationOverlay] 的认知复杂度。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawVisualTransaction(
    currentResult: TextLayoutResult,
    previousResult: TextLayoutResult?,
    transaction: ComposeVisualTransaction,
    textKind: TextVisualKind,
    cursorAnimate: Boolean,
    textProgress: Float,
    cursorProgress: Float,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    cursorSnapshot: VisualCursorSnapshot?,
    density: androidx.compose.ui.unit.Density,
    textEnabled: Boolean,
    rebaseProgress: Float,
) {
    // startFrame 起始画面层。
    val startFrame = transaction.startFrame
    if (startFrame != null && startFrame.slices.isNotEmpty()) {
        drawStartFrameLayer(
            startFrame = startFrame,
            rebaseProgress = rebaseProgress,
            scrollY = scrollY,
            textColor = textColor,
            currentResult = currentResult,
        )
    }

    if (textEnabled) {
        drawAnimatedRanges(
            currentResult = currentResult,
            previousResult = previousResult,
            oldRanges = transaction.oldRanges,
            newRanges = transaction.newRanges,
            retainedMoves = transaction.retainedMoves,
            textKind = textKind,
            progress = textProgress,
            scrollY = scrollY,
            textColor = textColor,
        )
    }

    // 视觉光标：从 cursorStartRect 插值到 cursorEndRect。
    // #644 评论 #684：从 frozen transaction 读取 cursorStartRect/cursorEndRect。
    if (cursorAnimate) {
        val cursorStartRect =
            transaction.cursorStartRect
                ?: cursorSnapshot?.oldCursorRect
                ?: return
        val cursorEndRect =
            transaction.cursorEndRect
                ?: cursorSnapshot?.newCursorRect
                ?: return
        drawVisualCursor(
            startRect = cursorStartRect,
            newRect = cursorEndRect,
            progress = cursorProgress,
            scrollY = scrollY,
            density = density,
            cursorColor = cursorColor,
        )
    }
}

/**
 * 绘制 startFrame 起始画面层 — 所有 slice 都是 [RebasedTextSlice]。
 */
private fun DrawScope.drawStartFrameLayer(
    startFrame: ComposeVisualFrame,
    rebaseProgress: Float,
    scrollY: Int,
    textColor: Color,
    currentResult: TextLayoutResult?,
) {
    for (slice in startFrame.slices) {
        val sourceResult = slice.sourceLayout.result
        if (slice.sourceRange.end > sourceResult.layoutInput.text.length) continue
        val targetRange = slice.targetRange
        if (targetRange != null) {
            val targetResult = currentResult ?: continue
            if (targetRange.end > targetResult.layoutInput.text.length) continue
            val sourceBounds = safePathBounds(sourceResult, slice.sourceRange) ?: continue
            val targetBounds = safePathBounds(targetResult, targetRange) ?: continue
            val interpolatedAlpha = lerp(slice.sourceAlpha, 1f, rebaseProgress)
            if (interpolatedAlpha <= 0f) continue
            val currentX =
                lerp(sourceBounds.left + slice.sourceTranslate.x, targetBounds.left, rebaseProgress)
            val currentY =
                lerp(sourceBounds.top + slice.sourceTranslate.y, targetBounds.top, rebaseProgress)
            val translate =
                Offset(
                    currentX - targetBounds.left,
                    currentY - targetBounds.top,
                )
            drawTranslatedRangeText(
                result = targetResult,
                range = targetRange,
                translate = translate,
                alpha = interpolatedAlpha,
                scrollY = scrollY,
                textColor = textColor,
            )
        } else {
            val interpolatedAlpha = lerp(slice.sourceAlpha, 0f, rebaseProgress)
            if (interpolatedAlpha <= 0f) continue
            drawTranslatedRangeText(
                result = sourceResult,
                range = slice.sourceRange,
                translate = slice.sourceTranslate,
                alpha = interpolatedAlpha,
                scrollY = scrollY,
                textColor = textColor,
            )
        }
    }
}

/** 视觉光标宽度（dp）。 */
private val VisualCursorWidthDp: Dp = 2.dp

/**
 * 视觉光标插值绘制 — 从 [startRect] 按 progress 插值到 [newRect]。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawVisualCursor(
    startRect: Rect,
    newRect: Rect,
    progress: Float,
    scrollY: Int,
    density: androidx.compose.ui.unit.Density,
    cursorColor: Color,
) {
    val interpolatedLeft = lerp(startRect.left, newRect.left, progress)
    val interpolatedTop = lerp(startRect.top, newRect.top, progress)
    val interpolatedBottom = lerp(startRect.bottom, newRect.bottom, progress)
    val interpolatedWidth = lerp(startRect.width, newRect.width, progress)

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
 * #644 评论 5662132136 第2项 + #684 评论 5663032418 断点3：静止光标 rect —
 * 从最新权威布局的几何 + **live** selection offset 取 caret rect。
 *
 * smooth cursor 开启、当前没有光标动画时（attach、两次输入之间、纯等待、动画结束后），
 * overlay 直接画这个 rect 作为静止光标。layout 缺失或 offset 越界时返回 null。
 *
 * #684 评论 5663032418 断点3：布局几何继续用 [layout.result]，但光标 offset 不再从
 * [ComposeLayoutSnapshot.selection]（只在 onTextLayout 时快照）读取，而是由调用方传入
 * live [TextFieldState.selection] 的 end。这样纯 selection 变化（方向键、点击移动光标）
 * 不触发新 layout 时，静止光标也能立即更新。
 *
 * [liveSelection] 为 null 时回退到 [layout.selection]（保持向后兼容，例如测试场景）。
 */
private fun computeRestingCursorRect(
    layout: ComposeLayoutSnapshot?,
    liveSelection: TextRange?,
): Rect? {
    if (layout == null) return null
    return try {
        val selectionEnd =
            (liveSelection?.end ?: layout.selection.end)
                .coerceIn(0, layout.result.layoutInput.text.length)
        layout.result.getCursorRect(selectionEnd)
    } catch (_: Throwable) {
        null
    }
}

/**
 * 对同一份 [TextLayoutResult] 做 `clipPath + drawText(result)`。
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
    translate(left = 0f, top = -scrollY.toFloat()) {
        clipPath(path) {
            drawText(
                textLayoutResult = result,
                color = textColor,
                topLeft = Offset.Zero,
                alpha = alpha,
            )
        }
    }
}

/**
 * 绘制受影响 range 的动画过程。
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
            val alpha = progress
            if (previousResult != null) {
                for (range in oldRanges) {
                    drawRangeText(previousResult, range, alpha = 1f - alpha, scrollY = scrollY, textColor = textColor)
                }
            }
            for (range in newRanges) {
                drawRangeText(currentResult, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
            }
        }
        TextVisualKind.None -> {
            // 没有文字动画（如 CURSOR_ONLY 事务）。
        }
    }
    // retained move：Insert/Delete/Move 都画。
    if (textKind != TextVisualKind.None) {
        drawRetainedMoves(
            previousResult = previousResult,
            currentResult = currentResult,
            retainedMoves = retainedMoves,
            progress = progress,
            scrollY = scrollY,
            textColor = textColor,
        )
    }
}

/**
 * 绘制 retained moves — 被挤到下一行的"保留文字"。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawRetainedMoves(
    previousResult: TextLayoutResult?,
    currentResult: TextLayoutResult,
    retainedMoves: List<RetainedMove>,
    progress: Float,
    scrollY: Int,
    textColor: Color,
) {
    for (move in retainedMoves) {
        val oldBounds = safePathBounds(previousResult, move.oldRange) ?: continue
        val newBounds = safePathBounds(currentResult, move.newRange) ?: continue
        val dx = lerp(oldBounds.left, newBounds.left, progress) - newBounds.left
        val dy = lerp(oldBounds.top, newBounds.top, progress) - newBounds.top
        drawTranslatedRangeText(
            result = currentResult,
            range = move.newRange,
            translate = Offset(dx, dy),
            alpha = 1f,
            scrollY = scrollY,
            textColor = textColor,
        )
    }
}

/** 安全获取 path bounds — result 为 null 或 range 无效时返回 null。 */
private fun safePathBounds(
    result: TextLayoutResult?,
    range: TextRange,
): Rect? {
    if (result == null) return null
    if (range.start >= range.end) return null
    if (range.end > result.layoutInput.text.length) return null
    return try {
        result.getPathForRange(range.start, range.end).getBounds()
    } catch (_: Throwable) {
        null
    }
}

/**
 * 按 translate 偏移绘制一段 range 文字。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawTranslatedRangeText(
    result: TextLayoutResult,
    range: TextRange,
    translate: Offset,
    alpha: Float,
    scrollY: Int,
    textColor: Color,
) {
    if (range.start >= range.end) return
    if (range.end > result.layoutInput.text.length) return
    if (alpha <= 0f) return
    val path = result.getPathForRange(range.start, range.end)
    withTransform({
        translate(
            left = translate.x,
            top = translate.y - scrollY.toFloat(),
        )
    }) {
        clipPath(path) {
            drawText(
                textLayoutResult = result,
                color = textColor,
                topLeft = Offset.Zero,
                alpha = alpha,
            )
        }
    }
}
