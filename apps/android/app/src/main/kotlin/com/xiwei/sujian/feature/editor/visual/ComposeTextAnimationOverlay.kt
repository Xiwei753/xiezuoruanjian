package com.xiwei.sujian.feature.editor.visual

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
 * #691：把光标 position 也并入同一持续视觉时间线。
 *
 * 删除：
 * - 独立的 `Animatable<Rect, AnimationVector4D>` 光标位置
 * - `LaunchedEffect(patchVersion)` 驱动的独立 cursor path 动画
 * - `animateCursorPath()` 作为独立位置时间线
 * - `computeRestingCursorRect()` — 静止光标由 visualState.restingCursorRect 提供
 *
 * 改成：
 * - 光标位置从 [ComposeVisualScene.cursorRect] 读取 — 与文字 units 共享同一个 frame clock
 * - 无光标动画时从 visualState.restingCursorRect 读取最终真实位置
 * - 光标闪烁（alpha）在 draw 阶段独立计算，不改变几何位置
 *
 * 绘制直接读 [ComposeVisualScene.units]。每个 unit 的 alpha、屏幕位置已经由 timeline
 * 按这个 frameTimeNanos 算好，draw 阶段不再二次插值。
 *
 * @param cursorColor 视觉光标颜色 — 从主题 role 注入。
 */
@Composable
@Suppress("LongParameterList")
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
    val visualScene by visualState.visualScene.collectAsStateWithLifecycle()
    val restingCursorRect by visualState.restingCursorRect.collectAsStateWithLifecycle()
    // #691 评论 5679242735 修改1：静止光标需要 live selection + latestLayout 实时计算。
    // BasicTextField.onTextLayout 只在"新的 text layout 被计算时"才回调，
    // 纯 selection 变化（鼠标点选、方向键移动）不保证重新计算文字布局，
    // 此时 restingCursorRect（只在 onAuthoritativeLayout 里更新）会停在旧位置。
    val latestLayout by visualState.latestLayout.collectAsStateWithLifecycle()

    val patchVersion by visualState.patchVersion.collectAsStateWithLifecycle()

    // #689 评论 5674631257 步骤8：只在 timeline 有活动 unit 时用 Compose 的帧时钟推进。
    // #689 评论 5676120929 问题1：用 patchVersion 唤醒帧循环，真正数据从队列 drain。
    // #689 评论 5675270164 缺陷6：全过程只用 withFrameNanos 的 frameTimeNanos。
    // #691：cursor 动画也并入 timeline，不再需要第二个 LaunchedEffect。
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
                    //    #691：光标位置从 scene.cursorRect（timeline 统一采样）读取，
                    //    或从 restingCursorRect（无动画时的最终真实位置）读取。
                    //    不再使用独立的 Animatable<Rect>。
                    //    #691 评论 5679242735 修改1：纯 selection 变化时 onTextLayout 不一定回调，
                    //    restingCursorRect 可能停在旧位置。此时用 latestLayout + liveSelection
                    //    实时计算静止光标；只有拿不到 live selection/layout 时才回退 restingCursorRect。
                    if (drawsVisualCursor) {
                        val cursorRectValue =
                            scene.cursorRect
                                ?: computeRestingCursorRect(latestLayout, liveSelection)
                                ?: restingCursorRect
                        if (cursorRectValue != null) {
                            drawVisualCursorRect(
                                rect = cursorRectValue,
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
 * #684 评论 5672654866 + #691：视觉光标绘制 —
 * 接收单个 [rect]，不再在 draw 阶段第二次插值。
 * 光标位置由 timeline 统一采样，不再使用独立的 Animatable。
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
 * #691 评论 5679242735 修改1：纯函数 — 从 layout + liveSelection 实时计算静止光标 rect。
 *
 * BasicTextField.onTextLayout 只在"新的 text layout 被计算时"才回调，
 * 纯 selection 变化（鼠标点选、方向键移动）不保证重新计算文字布局，
 * 此时 [ComposeEditorVisualState.restingCursorRect]（只在 onAuthoritativeLayout 里更新）
 * 会停在旧位置。overlay 用本函数 + [ComposeEditorVisualState.latestLayout] + liveSelection
 * 实时算出当前 selection 对应的光标几何。
 *
 * 不重新引入 `Animatable` — 这只是静态几何查询。
 *
 * @param layout 最新 layout 快照；null 时返回 null。
 * @param liveSelection 当前 live selection；null 时返回 null。
 * @return 当前 selection.end 对应的光标 rect；越界或异常时返回 null。
 */
internal fun computeRestingCursorRect(
    layout: ComposeLayoutSnapshot?,
    liveSelection: TextRange?,
): Rect? {
    if (layout == null || liveSelection == null) return null
    return try {
        val end = liveSelection.end.coerceIn(0, layout.result.layoutInput.text.length)
        layout.result.getCursorRect(end)
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
