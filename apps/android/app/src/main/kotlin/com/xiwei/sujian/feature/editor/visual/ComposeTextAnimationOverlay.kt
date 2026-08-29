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
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy

/**
 * #641 评论1 第5节 / 问题3 + 评论 5457777142 问题2/问题4：动画 overlay —
 * 只"画"，绝不能再改变 viewport / selection / IME 几何。
 *
 * 绘制规则：
 * - 新文字：从当前 [TextLayoutResult] 取 bounding box，当前 range 已被
 *   [OutputTransformation] 隐藏，overlay 做淡入/位移；
 * - 删除文字：保留上一份 [TextLayoutResult]，按旧 range 的 bounding box 画旧布局；
 * - 同行移动/自动折行/手动换行：old/new 坐标分别来自前后两份真实 [TextLayoutResult]；
 * - 视觉光标：从 `oldResult.getCursorRect(oldSelection.end)` 插值到
 *   `newResult.getCursorRect(newSelection.end)`，按 progress 插值 x/y/width/height。
 *
 * #641 评论 问题3 + 评论 5457777142 问题2：
 * - duration 来自 [EditorMotionPolicy]（不再写死 200ms）；
 * - 用 [Animatable] 手动控制动画，transaction id 变化时重新建立正确起点；
 * - 新事务到来时若 [ComposeVisualTransaction.startFrame] 非空，
 *   从 startFrame 对应的 progress 开始动画，而不是 `snapTo(0f)`；
 * - retained move 用 old/new `getPathForRange()` 的 bounds 算 dx/dy，
 *   按 progress 插值 translate，而不是 crossfade 冒充 move；
 * - 用 `getPathForRange` 替代整行 clip，同一行没参与动画的文字不会被 overlay 再画一遍；
 * - cursor 颜色吃 [cursorColor]，同一帧只画一次 cursor（不再闪烁叠加）。
 *
 * #641 评论 5457777142 问题4：双 timeline。
 * - [EditorMotionPolicy.coordinated] = true：一个 timeline（textDurationMillis），
 *   cursor 共用主 timeline；
 * - [EditorMotionPolicy.coordinated] = false：textProgress + cursorProgress 两个 timeline；
 * - CURSOR_ONLY（textKind = None 且 cursor.animate = true）单独用 cursorDurationMillis；
 * - reduceMotion / textEnabled / cursorEnabled 在这一层一次性落实：
 *   textEnabled=false 时不画文字动画（直接显示最终态）；
 *   cursorEnabled=false 时不画视觉光标（用系统光标）；
 *   reduceMotion=true 时全静态。
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
@Suppress("LongParameterList", "LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
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
    val motionPolicy = activeTransaction?.motionPolicy ?: EditorMotionPolicy()

    // #641 评论 5457777142 问题4 + 评论 5458283021 问题3a：根据 motionPolicy 决定 timeline 数量。
    // coordinated=true 且非 CURSOR_ONLY：一个 textProgress，cursor 共用；
    // coordinated=false：textProgress + cursorProgress 两个 timeline；
    // CURSOR_ONLY（textKind=None 且 cursor.animate=true）单独用 cursorDurationMillis，
    //   text 不动画，cursor 独立跑自己的 timeline。
    val isCursorOnly =
        activeIntent?.textKind == TextVisualKind.None && activeIntent?.cursor?.animate == true
    // #641 评论 5458283021 问题3a：useSingleTimeline 不再包含 isCursorOnly —
    // CURSOR_ONLY 时 cursor 单独跑自己的 timeline，不共用 textProgress。
    val useSingleTimeline = motionPolicy.coordinated && !isCursorOnly
    val textEnabled = motionPolicy.textEnabled && !isCursorOnly
    val cursorEnabled = motionPolicy.cursorEnabled

    val textDurationMillis = motionPolicy.textDurationMillis
    val cursorDurationMillis = motionPolicy.cursorDurationMillis

    // #641 评论 问题3 + 5457777142 问题2：用 Animatable 手动控制动画 —
    // transaction id 变化时若 startFrame 非空，从 startFrame 对应的 progress 开始；
    // 否则 snapTo(0f)。不再用 animateFloatAsState(target=1f)。
    // #641 评论 5458880786 问题1f：不再用 estimateStartProgress 把 frame 降维成平均 alpha —
    // textProgress / cursorProgress 始终从 0f 开始。startFrame 层在 drawVisualTransaction 里
    // 用 alpha = 1 - textProgress 淡出（已有逻辑），新事务层按 textProgress 画（已有 drawAnimatedRanges）。
    // 这样 t=0 画 100% frozen startFrame，t=1 画 100% 新事务最终态。
    // cursor 同理：cursorProgress 从 0f 开始，startFrame.cursorRect 在 drawStartFrameLayer 里
    // 按 startLayerAlpha = 1 - textProgress 淡出，新事务 cursor 在 drawVisualCursor 里按 cursorProgress 画。
    val textProgress = remember { Animatable(0f) }
    val cursorProgress = remember { Animatable(0f) }

    LaunchedEffect(transactionId) {
        if (transactionId > 0L && textDurationMillis > 0L && textEnabled) {
            textProgress.snapTo(0f)
            textProgress.animateTo(
                targetValue = 1f,
                animationSpec = tween(durationMillis = textDurationMillis.toInt()),
            )
        } else if (transactionId > 0L) {
            // textEnabled=false 或 CURSOR_ONLY：直接显示最终态，不画文字动画。
            textProgress.snapTo(1f)
        }
    }
    LaunchedEffect(transactionId) {
        // #641 评论 5458283021 问题3a：CURSOR_ONLY 单独启动 cursor timeline；
        // coordinated=true 的普通编辑才让 cursor 复用 text timeline；
        // coordinated=false 才分别跑两条。
        val shouldAnimateCursor =
            transactionId > 0L && cursorDurationMillis > 0L && cursorEnabled &&
                activeIntent?.cursor?.animate == true && (isCursorOnly || !useSingleTimeline)
        if (shouldAnimateCursor) {
            cursorProgress.snapTo(0f)
            cursorProgress.animateTo(
                targetValue = 1f,
                animationSpec = tween(durationMillis = cursorDurationMillis.toInt()),
            )
        } else if (useSingleTimeline && transactionId > 0L) {
            // coordinated=true 且非 CURSOR_ONLY：cursor 共用 textProgress。
            cursorProgress.snapTo(textProgress.value)
        } else if (transactionId > 0L) {
            // 无 cursor 动画：直接最终态。
            cursorProgress.snapTo(1f)
        }
    }
    val textProgressValue = textProgress.value
    val cursorProgressValue =
        if (useSingleTimeline) textProgressValue else cursorProgress.value

    // #641 评论 5458283021 问题1c：分别报告 textProgress / cursorProgress 给 visualState，
    // 供下一事务物化 startFrame。coordinated=false 时 cursor 用 cursorProgress 不再错算。
    LaunchedEffect(transactionId, textProgressValue, cursorProgressValue) {
        if (transactionId > 0L) {
            visualState.reportProgress(
                textProgress = textProgressValue,
                cursorProgress = cursorProgressValue,
            )
        }
    }

    val hasTextAnimation =
        activeTransaction != null && textEnabled &&
            (hiddenRanges.isNotEmpty() || activeIntent?.textKind != TextVisualKind.None)
    val hasCursorAnimation =
        activeTransaction != null && cursorEnabled &&
            activeIntent?.cursor?.animate == true
    val hasAnimation = hasTextAnimation || hasCursorAnimation

    // #641 评论 5458283021 问题3b：动画结束清 hiddenRanges，系统正文马上可见。
    // 完成条件只等待真正存在的 timeline —
    // 无 text 动画时 textDone=true，无 cursor 动画时 cursorDone=true，
    // 不再硬要求 cursorProgress>=1 导致 hiddenRanges 永远不清。
    LaunchedEffect(transactionId, textProgressValue, cursorProgressValue) {
        if (transactionId > 0L) {
            val textDone = !hasTextAnimation || textProgressValue >= 1f
            val cursorDone = !hasCursorAnimation || cursorProgressValue >= 1f
            if (textDone && cursorDone) {
                visualState.clearAnimation()
            }
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
                        cursorAnimate = intent.cursor?.animate == true && cursorEnabled,
                        textProgress = textProgressValue,
                        cursorProgress = cursorProgressValue,
                        scrollY = scrollY,
                        textColor = textColor,
                        cursorColor = cursorColor,
                        cursorSnapshot = cursorSnapshot,
                        density = density,
                        textEnabled = textEnabled,
                    )
                },
    )
}

/**
 * #641 评论 问题3 + 5457777142 问题2/问题4 + 评论 5458283021 问题1b：绘制视觉动画事务 —
 * 提取以降低 [ComposeTextAnimationOverlay] 的认知复杂度。
 *
 * #641 评论 5458283021 问题1b：如果 [ComposeVisualTransaction.startFrame] 非空，
 * 先按 startFrame.slices 的真实数据（layoutSource/range/translate/alpha）画起始画面层，
 * alpha 随 textProgress 淡出。不再把 frame 降维成一个平均 progress —
 * 上一帧的具体文字位置、retained translate、cursor rect 都被使用。
 *
 * #641 评论 5458880786 问题1f：删除 estimateStartProgress — textProgress/cursorProgress 从 0f 开始，
 * startFrame 层用 alpha = 1 - textProgress 淡出，新事务层按 textProgress 画。
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
) {
    // #641 评论 5458283021 问题1b：startFrame 起始画面层 —
    // 用 startFrame.slices 的真实数据画，alpha 随 textProgress 淡出。
    // 新事务的第一帧（textProgress≈0）先按这些数据画，再插值到新 transaction 的目标几何。
    // #641 评论 5458880786 问题1e：drawStartFrameLayer 只读 startFrame.sourceOldLayout/sourceNewLayout，
    // 不用当前 transaction 的 oldLayout/newLayout（新事务 onAuthoritativeLayout 已替换它们）。
    // 同时画 startFrame.cursorRect，让 cursor 真正参与第一帧。
    val startFrame = transaction.startFrame
    if (startFrame != null && textEnabled) {
        val startLayerAlpha = (1f - textProgress).coerceIn(0f, 1f)
        if (startLayerAlpha > 0f) {
            drawStartFrameLayer(
                startFrame = startFrame,
                alpha = startLayerAlpha,
                scrollY = scrollY,
                textColor = textColor,
                cursorColor = cursorColor,
                density = density,
            )
        }
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

    // 视觉光标：按 cursorProgress 从 old cursor rect 插值到 new cursor rect。
    // #641 评论 问题2：只要 cursor?.animate == true 就画（不管 textKind）。
    // #641 评论 问题3：同一帧只画一次 cursor（不再闪烁叠加）。
    // #641 评论 5457777142 问题4：cursor 用 cursorProgress（coordinated=false 时独立 timeline）。
    if (cursorAnimate && cursorSnapshot != null) {
        drawVisualCursor(
            snapshot = cursorSnapshot,
            progress = cursorProgress,
            scrollY = scrollY,
            density = density,
            cursorColor = cursorColor,
        )
    }
}

/**
 * #641 评论 5458283021 问题1b：绘制 startFrame 起始画面层 —
 * 用 startFrame.slices 的真实数据（layoutSource/range/translate/alpha）画。
 * 每个 slice 用 [VisualFrameSlice.layoutSource] 从 sourceOldLayout/sourceNewLayout 取对应 layout。
 * 不再把 frame 降维成一个平均 progress。
 *
 * #641 评论 5458880786 问题1e：layout 只读 [ComposeVisualFrame.sourceOldLayout] / [ComposeVisualFrame.sourceNewLayout]
 * （物化时冻结的上一事务 layout），不用当前 transaction 的 oldLayout/newLayout —
 * 新事务的 onAuthoritativeLayout 会替换 active transaction 的 oldLayout/newLayout，
 * 若仍指向 transaction.oldLayout/newLayout，slice 会从当前（新）事务 layout 取字，画面错乱。
 * 同时画 startFrame.cursorRect（按 alpha 淡出），让 cursor 真正参与第一帧。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawStartFrameLayer(
    startFrame: ComposeVisualFrame,
    alpha: Float,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    density: androidx.compose.ui.unit.Density,
) {
    val oldResult = startFrame.sourceOldLayout?.result
    val newResult = startFrame.sourceNewLayout?.result
    for (slice in startFrame.slices) {
        val result =
            when (slice.layoutSource) {
                FrameSliceSource.OldLayout -> oldResult ?: continue
                FrameSliceSource.NewLayout -> newResult ?: continue
            }
        if (slice.range.end > result.layoutInput.text.length) continue
        drawTranslatedRangeText(
            result = result,
            range = slice.range,
            translate = slice.translate,
            alpha = slice.alpha * alpha,
            scrollY = scrollY,
            textColor = textColor,
        )
    }
    // #641 评论 5458880786 问题1e：画 startFrame 的 cursor，让 cursor 真正参与第一帧。
    // 抽成 [drawStartFrameCursor] 降低 drawStartFrameLayer 圈复杂度。
    val cursorRect = startFrame.cursorRect
    if (cursorRect != null && startFrame.cursorAlpha > 0f) {
        drawStartFrameCursor(
            cursorRect = cursorRect,
            cursorAlpha = startFrame.cursorAlpha,
            alpha = alpha,
            scrollY = scrollY,
            cursorColor = cursorColor,
            density = density,
        )
    }
}

/**
 * #641 评论 5458880786 问题1e：绘制 startFrame 的 cursor（按 alpha 淡出）—
 * 抽取以降低 [drawStartFrameLayer] 圈复杂度。边界条件拆成 verticalInBounds / horizontalInBounds
 * 降低 ComplexCondition（每个 if 条件数 ≤ 3）。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawStartFrameCursor(
    cursorRect: Rect,
    cursorAlpha: Float,
    alpha: Float,
    scrollY: Int,
    cursorColor: Color,
    density: androidx.compose.ui.unit.Density,
) {
    val cursorWidth = density.run { VisualCursorWidthDp.toPx() }
    val top = cursorRect.top - scrollY.toFloat()
    val bottom = cursorRect.bottom - scrollY.toFloat()
    val verticalInBounds = bottom > 0f && top < size.height
    val horizontalInBounds = cursorRect.right > 0f && cursorRect.left < size.width
    if (verticalInBounds && horizontalInBounds) {
        drawRect(
            color = cursorColor,
            topLeft = Offset(cursorRect.left - cursorWidth / 2f, top),
            size = Size(maxOf(cursorWidth, cursorRect.width), bottom - top),
            alpha = cursorAlpha * alpha,
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
 *
 * #641 评论 5458283021 问题2d：统一改成"变换整个坐标系，再 clip + draw" —
 * 用 `translate(0f, -scrollY)` 同时变换后续绘制坐标系，
 * 保证 path 和 text 永远一起移动，滚动后 clip 和文字在同一坐标系。
 * Android 官方 DrawScope.translate 的语义是同时变换后续绘制坐标系，不要只移动文字。
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
    // #641 评论 5458283021 问题2d：先 translate 整个坐标系，再 clip + draw。
    // path 和 text 在同一变换后坐标系，滚动后不会分离。
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
 * #641 评论1 第5节 / 问题3 + 5457777142 问题2：绘制受影响 range 的动画过程。
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
            // #641 评论 5458880786 问题2f：drawRetainedMoves 移到 when 块之后，
            // Insert/Delete/Move 都画（自动折行最常见是 Insert 挤到下一行 / Delete 拉回上一行）。
        }
        TextVisualKind.None -> {
            // 没有文字动画（如 CURSOR_ONLY 事务）。
        }
    }
    // #641 评论 5458880786 问题2f：retained move 不只 Move 才画 —
    // 自动折行最常见是 Insert（挤到下一行）/Delete（拉回上一行），Insert/Delete/Move 都需要画 retained move，
    // 否则被挤到新位置的保留文字会重影（hiddenRanges 隐藏了新位置，但 overlay 没画 old→new 位移过渡）。
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
 * #641 评论 问题3 + 5457777142 问题2：绘制 retained moves —
 * 被挤到下一行的"保留文字"。
 *
 * 真实现：用 old/new `getPathForRange()` 的 bounds 算 dx/dy，
 * 按 progress 插值 translate，而不是 crossfade 冒充 move。
 * 提取以降低 [drawAnimatedRanges] 的认知复杂度。
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
        // old bounds（previous layout）和 new bounds（current layout）。
        val oldBounds = safePathBounds(previousResult, move.oldRange) ?: continue
        val newBounds = safePathBounds(currentResult, move.newRange) ?: continue
        val dx = lerp(oldBounds.left, newBounds.left, progress) - newBounds.left
        val dy = lerp(oldBounds.top, newBounds.top, progress) - newBounds.top
        // 按 translate 画 current layout 的 newRange，alpha=1（retained 文字全程可见）。
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
 * #641 评论 5457777142 问题2 + 评论 5458283021 问题2c：按 translate 偏移绘制一段 range 文字。
 *
 * #641 评论 5458283021 问题2c：统一改成"变换整个坐标系，再 clip + draw" —
 * 用 `withTransform { translate(dx, dy - scrollY) }` 同时变换后续绘制坐标系，
 * 然后 `clipPath(path) { drawText(topLeft = Offset.Zero) }`。
 * path 和 text 在同一变换后坐标系，progress=0 时文字画在 old position，
 * clip 也跟着移到 old position，文字不会被自己 new position 的 clip 掉。
 * Android 官方 DrawScope.withTransform/translate 的语义是同时变换后续绘制坐标系，
 * 不要只移动文字。
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
    // #641 评论 5458283021 问题2c：先 translate 整个坐标系（含 scrollY），
    // 再 clip + draw。path 和 text 在同一变换后坐标系，
    // 文字不会被自己 new position 的 clip 掉，滚动后也不会分离。
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
