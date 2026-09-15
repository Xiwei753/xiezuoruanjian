package com.xiwei.sujian.feature.editor.visual

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector4D
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
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

/**
 * #641 评论1 第5节 / 问题3 + 评论 5457777142 问题2/问题4：动画 overlay —
 * 只"画"，绝不能再改变 viewport / selection / IME 几何。
 *
 * #689 评论 5674631257 步骤8：把视觉动画从"事务重启"改成"持续时间线"。
 *
 * 删除：
 * - 全局 `masterProgress = remember { Animatable(0f) }`
 * - `LaunchedEffect(transactionId) { masterProgress.snapTo(0f); masterProgress.animateTo(1f, ...) }`
 * - textProgressValue / rebaseProgressValue
 * - reportProgress() / finishTransaction()
 * - drawStartFrameLayer()
 * - drawUnitWiseAppear()/drawUnitWiseDisappear() 对全局 progress 的依赖
 * - drawRetainedMoves(... progress)
 *
 * 改成只在 timeline 有活动 unit 时用 Compose 的帧时钟推进：
 * ```kotlin
 * LaunchedEffect(patchVersion) {
 *     while (visualState.hasActiveVisuals()) {
 *         withFrameNanos { frameTimeNanos -> visualState.advanceVisualFrame(frameTimeNanos) }
 *     }
 * }
 * ```
 *
 * 绘制直接读 [ComposeVisualScene.units]。每个 unit 的 alpha、屏幕位置已经由 timeline
 * 按这个 frameTimeNanos 算好，draw 阶段不再二次插值。
 *
 * 光标保留跨 patch 的 `Animatable<Rect>` 思路（已做到新目标到来时从当前 value 继续）；
 * 触发条件从 `transactionId` 换成 patch/cursor target 的版本号。
 *
 * @param cursorColor 视觉光标颜色 — 从主题 role 注入。
 */
@Composable
@Suppress("LongParameterList", "LongMethod", "CyclomaticComplexity", "CognitiveComplexMethod")
fun ComposeTextAnimationOverlay(
    visualState: ComposeEditorVisualState,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    /**
     * #684 评论 5663032418 断点3：live selection — 直接从 [TextFieldState.selection] 读取。
     */
    liveSelection: TextRange?,
    modifier: Modifier = Modifier,
) {
    val hiddenRanges by visualState.hiddenRanges.collectAsStateWithLifecycle()
    val density = LocalDensity.current

    val drawsVisualCursor by visualState.drawsVisualCursor.collectAsStateWithLifecycle()
    val latestLayout by visualState.latestLayout.collectAsStateWithLifecycle()
    val latestPatch by visualState.latestPatch.collectAsStateWithLifecycle()
    val visualScene by visualState.visualScene.collectAsStateWithLifecycle()

    val patchVersion by visualState.patchVersion.collectAsStateWithLifecycle()
    val hasCursorMotionPath = latestPatch?.cursorMotionPath != null

    // #684 评论 5672654866：光标的位置改成一个跨 patch 保持的 Animatable<Rect, AnimationVector4D>。
    // 新 patch 取消旧 LaunchedEffect 后，同一个 Animatable 仍然保留刚才屏幕实际画到的 rect；
    // 下一次 animateTo() 从这个真实 rect 出发。
    // 第一次 attach、章节切换或当前还没有任何视觉光标位置时，才允许 snapTo(restingRect)。
    // patch 切换时禁止 snapTo(path.first())，直接从 cursorRect.value 继续。
    var cursorInitialized by remember { mutableStateOf(false) }
    val cursorRect =
        remember {
            Animatable(Rect.Zero, Rect.VectorConverter)
        }
    val restingRect = computeRestingCursorRect(latestLayout, liveSelection)
    val hasCursorAnimation = latestPatch != null && hasCursorMotionPath
    LaunchedEffect(drawsVisualCursor, hasCursorAnimation, restingRect) {
        if (drawsVisualCursor && !hasCursorAnimation && restingRect != null) {
            cursorRect.snapTo(restingRect)
            cursorInitialized = true
        }
    }
    LaunchedEffect(drawsVisualCursor, latestLayout) {
        if (!drawsVisualCursor || latestLayout == null) {
            cursorInitialized = false
        }
    }

    // #689 评论 5674631257 步骤8：只在 timeline 有活动 unit 时用 Compose 的帧时钟推进。
    // #689 评论 5676120929 问题1：用 patchVersion 唤醒帧循环，真正数据从队列 drain。
    // 这样即使 LaunchedEffect 因 key 变化重启，patch 数据仍在队列里不会丢。
    // #689 评论 5675270164 缺陷6：全过程只用 withFrameNanos 的 frameTimeNanos，
    // 不用 System.nanoTime()（Compose 官方明确 withFrameNanos 的 frameTimeNanos
    // time base 是 implementation-defined，不保证等于 System.nanoTime()）。
    LaunchedEffect(patchVersion) {
        if (patchVersion <= 0L) return@LaunchedEffect
        while (true) {
            val active =
                withFrameNanos { frameTimeNanos ->
                    visualState.drainPendingPatchesAtFrame(frameTimeNanos)
                    visualState.sampleVisualScene(frameTimeNanos)
                    visualState.hasPendingPatches() || visualState.hasActiveVisuals(frameTimeNanos)
                }
            if (!active) break
        }
    }

    // #684 评论 5672654866：光标动画 — 按 cursorMotionPath 分段 animateTo。
    // #689 评论 5676120929 问题1：用 patchVersion 与帧循环同步。
    LaunchedEffect(patchVersion) {
        if (patchVersion <= 0L) return@LaunchedEffect
        val path = latestPatch?.cursorMotionPath ?: return@LaunchedEffect
        val durationMs = latestPatch?.durationMs ?: 0L
        if (durationMs <= 0L) {
            path.points.lastOrNull()?.let { cursorRect.snapTo(it.rect) }
            return@LaunchedEffect
        }
        animateCursorPath(
            cursor = cursorRect,
            path = path,
            durationMs = durationMs,
        )
    }

    Box(
        modifier =
            modifier
                .drawBehind {
                    // 1. 动画帧：直接读 visualScene.units。
                    //    每个 unit 的 alpha、屏幕位置已经由 timeline 按当前帧时间算好，
                    //    draw 阶段不再二次插值。
                    val scene = visualScene
                    if (scene.units.isNotEmpty()) {
                        drawVisualScene(
                            scene = scene,
                            scrollY = scrollY,
                            textColor = textColor,
                        )
                    }

                    // 2. 光标：smooth cursor 开启时 overlay 整个会话拥有光标。
                    //    动画进行中 cursorRect.value 是 animateCursorPath 当前画到的 rect；
                    //    无光标动画时 cursorRect.value 是上次 snapTo 的 resting rect。
                    if (drawsVisualCursor) {
                        drawVisualCursorRect(
                            rect = cursorRect.value,
                            scrollY = scrollY,
                            density = density,
                            cursorColor = cursorColor,
                        )
                    }
                },
    )
}

/**
 * #689 评论 5674631257 步骤8：绘制持续视觉场景 —
 * 直接读 [ComposeVisualScene.units]，每个 unit 的 alpha、屏幕位置已经由 timeline 算好。
 */
private fun DrawScope.drawVisualScene(
    scene: ComposeVisualScene,
    scrollY: Int,
    textColor: Color,
) {
    for (unit in scene.units) {
        val range = unit.range
        if (range.start >= range.end) continue
        val result = unit.layout.result
        if (range.end > result.layoutInput.text.length) continue
        // alpha 已由 timeline 算好，直接读 unit.alpha.from（sample 后 from == 当前值）
        val alpha = unit.alpha.from.coerceIn(0f, 1f)
        if (alpha <= 0f) continue
        // position 已由 timeline 算好，直接读 unit.position.from（sample 后 from == 当前值）
        val currentPosition = unit.position.from
        val targetRange = unit.targetRange
        if (targetRange != null) {
            // 存活 unit：在新 layout 的真实位置 + timeline 算好的偏移
            val targetBounds = safePathBounds(result, targetRange) ?: continue
            val translate =
                Offset(
                    currentPosition.x - targetBounds.left,
                    currentPosition.y - targetBounds.top,
                )
            drawTranslatedRangeText(
                result = result,
                range = targetRange,
                translate = translate,
                alpha = alpha,
                scrollY = scrollY,
                textColor = textColor,
            )
        } else {
            // ghost unit：在旧 layout 的真实位置淡出
            val sourceBounds = safePathBounds(result, range) ?: continue
            val translate =
                Offset(
                    currentPosition.x - sourceBounds.left,
                    currentPosition.y - sourceBounds.top,
                )
            drawTranslatedRangeText(
                result = result,
                range = range,
                translate = translate,
                alpha = alpha,
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
 * #684 评论 5672654866：按 [CursorMotionPath] 分段 animateTo。
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
            animationSpec = tween(durationMillis = totalMs, easing = LinearEasing),
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
                animationSpec = tween(durationMillis = segmentMs, easing = LinearEasing),
            )
        } else {
            cursor.snapTo(point.rect)
        }
        prevFraction = point.endFraction
    }
}

/**
 * #644 评论 5662132136 第2项 + #684 评论 5663032418 断点3：静止光标 rect。
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
