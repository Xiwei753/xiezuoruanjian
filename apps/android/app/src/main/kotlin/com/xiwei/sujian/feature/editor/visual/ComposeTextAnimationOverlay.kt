package com.xiwei.sujian.feature.editor.visual

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector4D
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
    val density = LocalDensity.current

    // #644 评论 5662132136 第2项：smooth cursor 接管后必须有静止光标绘制路径。
    // drawsVisualCursor 由设置/attach 生命周期决定（overlay 是否整个会话拥有光标）。
    // latestLayout 为最新权威布局，静止光标从中取 caret rect。
    val drawsVisualCursor by visualState.drawsVisualCursor.collectAsStateWithLifecycle()
    val latestLayout by visualState.latestLayout.collectAsStateWithLifecycle()

    val transactionId = activeTransaction?.id ?: 0L

    // #684 评论 5672654866：光标的位置改成一个跨 transaction 保持的 Animatable<Rect, AnimationVector4D>。
    // 旧 LaunchedEffect(transactionId) 被新事务取消后，同一个 Animatable 仍然保留刚才屏幕实际画到的 rect；
    // 下一次 animateTo() 从这个真实 rect 出发，不再出现"用落后一帧的 progress 反算起点再往回抽"。
    // 第一次 attach、章节切换或当前还没有任何视觉光标位置时，才允许 snapTo(restingRect)。
    // 事务切换时禁止 snapTo(path.first())，直接从 cursorRect.value 继续。
    var cursorInitialized by remember { mutableStateOf(false) }
    val cursorRect = remember {
        Animatable(Rect.Zero, Rect.VectorConverter)
    }
    // 首次 attach / 章节切换 / 当前还没有任何视觉光标位置时，把权威 layout 的 resting rect 同步进 Animatable。
    // 只有 selection 被鼠标/方向键直接改动且没有 cursor 动画语义时才走这条路径。
    LaunchedEffect(drawsVisualCursor, latestLayout, liveSelection) {
        if (drawsVisualCursor && !cursorInitialized) {
            val restingRect = computeRestingCursorRect(latestLayout, liveSelection)
            if (restingRect != null) {
                cursorRect.snapTo(restingRect)
                cursorInitialized = true
            }
        }
    }
    // 章节切换（drawsVisualCursor 从 false→true 或 layout 重置）时重置初始化标志，让上面的 LaunchedEffect 重新 snapTo。
    LaunchedEffect(drawsVisualCursor) {
        if (!drawsVisualCursor) {
            cursorInitialized = false
        }
    }

    // #644 评论 #684（评论 #5660899405 第 5 项）：Core 的动画语义必须被真正消费 —
    // 正文视觉事务的 master timeline 直接使用冻结事务里的 durationMs；
    // AnimationModeDto.SYSTEM_SUPPRESSED 必须禁止正文自定义动画；
    // 设置层 motionPolicy 只负责总开关 / reduce motion，不覆盖 Core 已经决定好的本笔动画事实。
    // #684 评论 5665907509 问题1：animationMode 从冻结事务读取，不再从 _activeIntent 读取。
    //   overlay 据此判断 systemSuppressed，保证 SYSTEM_SUPPRESSED 到来时直接落到系统最终正文，
    //   不会让上一笔动画的 suppressed ranges / startFrame 跨过这笔 suppressed 事务继续跑。
    // #684 评论 5666730754：textEnabled/cursorEnabled 直接从 transaction 的冻结字段读，
    //   不再用 motionPolicy/systemSuppressed/activeIntent?.cursor?.animate 在 overlay 侧重新判断。
    //   视觉所有权在 coordinator 生成事务时一次算死，overlay 只读冻结结果。
    val transactionTextKind = activeTransaction?.textKind ?: TextVisualKind.None
    val textEnabled = activeTransaction?.textAnimationActive == true
    val cursorEnabled = activeTransaction?.cursorAnimationActive == true

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

    // 从 master progress 推导 textProgress / rebaseProgress。
    val textProgressValue = if (textEnabled) masterProgressValue else 1f
    // #684 评论 5672654866：cursorProgressValue 不再用于光标绘制 —
    // 光标由跨 transaction 的 cursorRect Animatable + cursorMotionPath 驱动。
    val rebaseProgressValue = masterProgressValue

    // #684 评论 5672654866：光标动画 — 按 cursorMotionPath 分段 animateTo。
    // 一次提交多个字时，光标依次经过每个 unit 的 caret rect；
    // 文字第几个 unit 正在吐，光标就移动到对应的第几个位置。
    // 旧 LaunchedEffect(transactionId) 被新事务取消后，同一个 cursorRect Animatable
    // 仍然保留刚才屏幕实际画到的 rect；下一次 animateTo() 从这个真实 rect 出发。
    LaunchedEffect(transactionId) {
        if (transactionId <= 0L) return@LaunchedEffect
        val path = activeTransaction?.cursorMotionPath ?: return@LaunchedEffect
        val durationMs = activeTransaction?.durationMs ?: 0L
        if (durationMs <= 0L) {
            // 无时长事务：直接 snapTo 路径最后一个点（最终光标位置）。
            path.points.lastOrNull()?.let { cursorRect.snapTo(it.rect) }
            return@LaunchedEffect
        }
        animateCursorPath(
            cursor = cursorRect,
            path = path,
            durationMs = durationMs,
        )
    }

    // #644 评论 #684：报告单 master progress 给 visualState，供下一事务物化 startFrame。
    // #684 评论 5668108597 问题1：reportProgress 带 transactionId 守卫 —
    // 旧事务迟到的 progress 不会污染新事务的 _masterProgress。
    LaunchedEffect(transactionId, masterProgressValue) {
        if (transactionId > 0L) {
            visualState.reportProgress(transactionId, masterProgressValue)
        }
    }

    val hasTextAnimation =
        activeTransaction != null && textEnabled &&
            (hiddenRanges.isNotEmpty() || transactionTextKind != TextVisualKind.None)
    // #684 评论 5666730754：hasCursorAnimation 直接读 transaction 冻结的 cursorAnimationActive，
    // 不再叠加 activeIntent?.cursor?.animate 判断。
    val hasCursorAnimation = activeTransaction != null && cursorEnabled
    val startFrameHasSlices = activeTransaction?.startFrame?.slices?.isNotEmpty() == true
    val hasRebaseAnimation = activeTransaction != null && startFrameHasSlices && rebaseProgressValue < 1f
    val hasAnimation = hasTextAnimation || hasCursorAnimation || hasRebaseAnimation

    // 动画结束：收口成一个带 ID 守卫的 finishTransaction 调用。
    // #684 评论 5667483662 问题2：不再分两步 complete + clear —
    // 旧事务迟到的完成回调若分两步，clearAnimation 无 ID 守卫会清掉新事务的 visual state。
    // finishTransaction 内部先检查 _activeTransaction.id == transactionId 才生效。
    // 系统正文马上可见。
    LaunchedEffect(transactionId, masterProgressValue) {
        if (transactionId > 0L && masterProgressValue >= 1f) {
            visualState.finishTransaction(transactionId)
        }
    }

    Box(
        modifier =
            modifier
                .drawBehind {
                    // 1. 动画帧：文字 Insert/Delete/Move、retained reflow、startFrame rebase。
                    //    绘制只读 frozen transaction，不再读 currentLayout/previousLayout。
                    //    #684 评论 5672654866：光标不再在此分支绘制 —
                    //    光标由跨 transaction 的 cursorRect Animatable 驱动，在第 2 步统一画。
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
                                textProgress = textProgressValue,
                                scrollY = scrollY,
                                textColor = textColor,
                                density = density,
                                textEnabled = textEnabled,
                                rebaseProgress = rebaseProgressValue,
                            )
                        }
                    }

                    // 2. 光标：smooth cursor 开启时 overlay 整个会话拥有光标。
                    //    #684 评论 5672654866：光标动画进行中、静止光标都用同一个 cursorRect.value。
                    //    动画进行中 cursorRect.value 是 animateCursorPath 当前画到的 rect；
                    //    无光标动画时 cursorRect.value 是上次 snapTo 的 resting rect。
                    //    不再在 draw 阶段第二次插值 — cursorRect.value 已经是当前应画的真实位置。
                    //    #684 评论 5663032418 断点3：静止光标 offset 从 live TextFieldState.selection 读取，
                    //    不再依赖 latestLayout.selection（只在 onTextLayout 时更新，纯 selection 变化会过期）。
                    if (drawsVisualCursor) {
                        // 无光标动画时把权威 layout 的 resting rect 同步进 Animatable。
                        // 只有 selection 被鼠标/方向键直接改动且没有 cursor 动画语义时才走这条路径。
                        if (!hasCursorAnimation) {
                            val restingRect = computeRestingCursorRect(latestLayout, liveSelection)
                            if (restingRect != null) {
                                // 同步静止光标到 Animatable（在 draw 阶段不能直接 snapTo，
                                // 用 LaunchedEffect 异步同步；这里直接用 restingRect 画）。
                                drawVisualCursorRect(
                                    rect = restingRect,
                                    scrollY = scrollY,
                                    density = density,
                                    cursorColor = cursorColor,
                                )
                            }
                        } else {
                            // 光标动画进行中：直接用 cursorRect.value，不再在 draw 阶段插值。
                            drawVisualCursorRect(
                                rect = cursorRect.value,
                                scrollY = scrollY,
                                density = density,
                                cursorColor = cursorColor,
                            )
                        }
                    }
                },
    )
}

/**
 * #641 评论 5459896691：绘制视觉动画事务 —
 * 提取以降低 [ComposeTextAnimationOverlay] 的认知复杂度。
 *
 * #684 评论 5672654866：本函数只画正文动画 + startFrame rebase，不画光标。
 * 光标由 overlay 主体用跨 transaction 的 cursorRect Animatable 统一画。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawVisualTransaction(
    currentResult: TextLayoutResult,
    previousResult: TextLayoutResult?,
    transaction: ComposeVisualTransaction,
    textKind: TextVisualKind,
    textProgress: Float,
    scrollY: Int,
    textColor: Color,
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
            oldAnimationUnits = transaction.oldAnimationUnits,
            newAnimationUnits = transaction.newAnimationUnits,
            animationMode = transaction.animationMode,
            retainedMoves = transaction.retainedMoves,
            textKind = textKind,
            progress = textProgress,
            scrollY = scrollY,
            textColor = textColor,
        )
    }
    // #684 评论 5672654866：光标不再在此绘制 —
    // 光标由 overlay 主体用 cursorRect.value 统一画，不在 draw 阶段第二次插值。
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
 * #684 评论 5672654866：视觉光标绘制 — 接收单个 [rect]，不再在 draw 阶段第二次插值。
 *
 * 光标动画由 overlay 内长生命周期 [Animatable]<Rect, AnimationVector4D> 驱动，
 * animateCursorPath 按 [CursorMotionPoint.endFraction] 分段 animateTo，
 * draw 阶段直接画 cursorRect.value 当前值。
 */
private fun DrawScope.drawVisualCursorRect(
    rect: Rect,
    scrollY: Int,
    density: androidx.compose.ui.unit.Density,
    cursorColor: Color,
) {
    val cursorWidth = density.run { VisualCursorWidthDp.toPx() }
    val cursorLeft = rect.left - cursorWidth / 2
    val cursorRight = cursorLeft + maxOf(cursorWidth, rect.width)

    val cursorTop = rect.top - scrollY.toFloat()
    val cursorBottom = rect.bottom - scrollY.toFloat()

    if (cursorBottom <= 0f || cursorTop >= size.height) return
    if (cursorRight <= 0f || cursorLeft >= size.width) return

    drawRect(
        color = cursorColor,
        topLeft = Offset(cursorLeft, cursorTop),
        size = Size(cursorRight - cursorLeft, cursorBottom - cursorTop),
    )
}

/**
 * #684 评论 5672654866：按 [CursorMotionPath] 分段 animateTo —
 * 一次提交多个字时，光标依次经过每个 unit 的 caret rect；
 * 文字第几个 unit 正在吐，光标就移动到对应的第几个位置。
 *
 * 每段 durationMs = totalDurationMs * (next.endFraction - current.endFraction)。
 * 路径只有一个点时直接 animateTo 到那个点。
 * 新事务取消旧 LaunchedEffect 后，同一个 [cursor] Animatable 仍然保留刚才屏幕实际画到的 rect，
 * 下一次 animateTo() 从这个真实 rect 出发。
 */
suspend fun animateCursorPath(
    cursor: Animatable<Rect, AnimationVector4D>,
    path: CursorMotionPath,
    durationMs: Long,
) {
    val points = path.points
    if (points.isEmpty()) return
    val totalMs = durationMs.toInt().coerceAtLeast(0)
    if (points.size == 1) {
        cursor.animateTo(
            targetValue = points[0].rect,
            animationSpec = tween(durationMillis = totalMs),
        )
        return
    }
    var prevFraction = 0f
    for (point in points) {
        val segmentFraction = (point.endFraction - prevFraction).coerceIn(0f, 1f)
        val segmentMs = (totalMs * segmentFraction).toInt().coerceAtLeast(0)
        if (segmentMs > 0) {
            cursor.animateTo(
                targetValue = point.rect,
                animationSpec = tween(durationMillis = segmentMs),
            )
        } else {
            // 零时长段：直接 snapTo 避免 animateTo 默认时长。
            cursor.snapTo(point.rect)
        }
        prevFraction = point.endFraction
    }
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
 *
 * #684 评论 5668108597 问题2：按 Core 计算的 animation units 做吐字/吞字动画。
 * - `oldAnimationUnits` / `newAnimationUnits` 为空时回退到整段 alpha 行为（向后兼容）。
 * - 有 units 时每个 unit 在原位按 master progress 依次显现/消失：
 *   N 个 unit，unit i 的局部 progress = ((progress * N) - i).coerceIn(0f, 1f)。
 * - Insert: newAnimationUnits 依次淡入（吐字）。
 * - Delete: oldAnimationUnits 依次淡出（吞字）。
 * - Move: oldAnimationUnits 淡出 + newAnimationUnits 淡入。
 * - RunAnimation: 按 run 组推进（每个 run 作为一个整体，run 之间依次出现）。
 * - LineReflowAnimation: 同时驱动 retained move（保留现有行为）。
 * - SystemSuppressed: 不画（textEnabled 已经是 false）。
 *
 * 一笔输入仍只有一个 master timeline，文字、reflow、光标继续协调，不重新拆成几套时钟。
 * 不做"整段从左边滑进来" — 每个 Core 单元在原位按 master progress 依次显现/消失。
 */
@Suppress("LongParameterList", "CyclomaticComplexity", "CognitiveComplexMethod")
private fun DrawScope.drawAnimatedRanges(
    currentResult: TextLayoutResult,
    previousResult: TextLayoutResult?,
    oldRanges: List<TextRange>,
    newRanges: List<TextRange>,
    oldAnimationUnits: List<TextRange>,
    newAnimationUnits: List<TextRange>,
    animationMode: AnimationModeDto,
    retainedMoves: List<RetainedMove>,
    textKind: TextVisualKind,
    progress: Float,
    scrollY: Int,
    textColor: Color,
) {
    // #684 评论 5668108597 问题2：各 Core 动画模式的处理策略 —
    // SystemSuppressed 已被外层 textEnabled=false 拦截（不会进入本函数）。
    // LineReflowAnimation 的 retained move 在函数末尾统一驱动。
    // RunAnimation 的"按 run 组依次出现"由 unit-wise 默认行为覆盖（unit 即 Core 算好的 run 边界）。
    // GlyphAnimation/ClusterAnimation/SnapshotAnimation：unit 在原位按 localProgress 依次显现/消失。
    when (animationMode) {
        AnimationModeDto.SYSTEM_SUPPRESSED -> {
            // 不画 — textEnabled 已是 false，理论上不会进入本分支。保留防御。
            return
        }
        AnimationModeDto.GLYPH_ANIMATION,
        AnimationModeDto.CLUSTER_ANIMATION,
        AnimationModeDto.RUN_ANIMATION,
        AnimationModeDto.LINE_REFLOW_ANIMATION,
        AnimationModeDto.SNAPSHOT_ANIMATION,
        -> Unit
    }
    when (textKind) {
        TextVisualKind.Insert -> {
            if (newAnimationUnits.isNotEmpty()) {
                drawUnitWiseAppear(
                    result = currentResult,
                    units = newAnimationUnits,
                    progress = progress,
                    scrollY = scrollY,
                    textColor = textColor,
                )
            } else {
                // 回退到整段 alpha（向后兼容）。
                val alpha = progress
                for (range in newRanges) {
                    drawRangeText(currentResult, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
                }
            }
        }
        TextVisualKind.Delete -> {
            val result = previousResult ?: currentResult
            if (oldAnimationUnits.isNotEmpty()) {
                drawUnitWiseDisappear(
                    result = result,
                    units = oldAnimationUnits,
                    progress = progress,
                    scrollY = scrollY,
                    textColor = textColor,
                )
            } else {
                // 回退到整段 alpha（向后兼容）。
                val alpha = 1f - progress
                for (range in oldRanges) {
                    drawRangeText(result, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
                }
            }
        }
        TextVisualKind.Move -> {
            // Move: old units 淡出 + new units 淡入。
            if (previousResult != null) {
                if (oldAnimationUnits.isNotEmpty()) {
                    drawUnitWiseDisappear(
                        result = previousResult,
                        units = oldAnimationUnits,
                        progress = progress,
                        scrollY = scrollY,
                        textColor = textColor,
                    )
                } else {
                    val alpha = 1f - progress
                    for (range in oldRanges) {
                        drawRangeText(previousResult, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
                    }
                }
            }
            if (newAnimationUnits.isNotEmpty()) {
                drawUnitWiseAppear(
                    result = currentResult,
                    units = newAnimationUnits,
                    progress = progress,
                    scrollY = scrollY,
                    textColor = textColor,
                )
            } else {
                val alpha = progress
                for (range in newRanges) {
                    drawRangeText(currentResult, range, alpha = alpha, scrollY = scrollY, textColor = textColor)
                }
            }
        }
        TextVisualKind.None -> {
            // 没有文字动画（如 CURSOR_ONLY 事务）。
        }
    }
    // retained move：Insert/Delete/Move 都画。
    // LineReflowAnimation 同时驱动 retained move（保留现有行为）。
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
 * #684 评论 5668108597 问题2：按 unit 依次淡入（吐字）。
 *
 * N 个 unit，unit i 的局部 progress = ((progress * N) - i).coerceIn(0f, 1f)。
 * RunAnimation 模式下每个 run 作为一个整体，run 之间依次出现（与默认行为一致，
 * 因为 unit 已经是 Core 管好的 run 边界）。
 *
 * #684 评论 5670182711 问题2：alpha 公式收成 [ComposeVisualRebase.unitLocalProgress] 纯函数，
 * 与 startFrame 物化（collectCurrentSlicesAsRebased）使用同一套公式。
 */
private fun DrawScope.drawUnitWiseAppear(
    result: TextLayoutResult,
    units: List<TextRange>,
    progress: Float,
    scrollY: Int,
    textColor: Color,
) {
    if (units.isEmpty()) return
    val n = units.size
    for ((i, unit) in units.withIndex()) {
        val localProgress = ComposeVisualRebase.unitLocalProgress(progress, i, n)
        if (localProgress <= 0f) continue
        // RunAnimation: 每个 run 整体出现（unit 即 run，无需特殊处理）。
        // 其他模式（GlyphAnimation/ClusterAnimation/LineReflowAnimation/SnapshotAnimation）：
        // unit 在原位按 localProgress 淡入。
        drawRangeText(
            result = result,
            range = unit,
            alpha = localProgress,
            scrollY = scrollY,
            textColor = textColor,
        )
    }
}

/**
 * #684 评论 5668108597 问题2：按 unit 依次淡出（吞字）。
 *
 * N 个 unit，unit i 的局部 progress = ((progress * N) - i).coerceIn(0f, 1f)。
 * alpha = 1f - localProgress（先消失的 unit alpha 先到 0）。
 *
 * #684 评论 5670182711 问题2：alpha 公式收成 [ComposeVisualRebase.unitLocalProgress] 纯函数，
 * 与 startFrame 物化（collectCurrentSlicesAsRebased）使用同一套公式。
 */
private fun DrawScope.drawUnitWiseDisappear(
    result: TextLayoutResult,
    units: List<TextRange>,
    progress: Float,
    scrollY: Int,
    textColor: Color,
) {
    if (units.isEmpty()) return
    val n = units.size
    for ((i, unit) in units.withIndex()) {
        val localProgress = ComposeVisualRebase.unitLocalProgress(progress, i, n)
        val alpha = 1f - localProgress
        if (alpha <= 0f) continue
        drawRangeText(
            result = result,
            range = unit,
            alpha = alpha,
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
