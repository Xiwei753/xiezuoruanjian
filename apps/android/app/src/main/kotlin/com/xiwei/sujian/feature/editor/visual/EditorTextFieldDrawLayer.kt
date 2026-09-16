package com.xiwei.sujian.feature.editor.visual

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
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
 * #698 评论 5698296237 / 5697612595 / 5699401353：编辑器绘制链根改 —
 * 统一 draw 层，断开"动画 hiddenRanges -> OutputTransformation 改正文显示 ->
 * BasicTextField 再 layout -> VisualState 再消费 layout"回路。
 *
 * #698 评论 5699401353 修复1：本 draw 层真正包住 [BasicTextField]（[content]），
 * 用 `Modifier.drawWithContent` 在绘制阶段对 [ComposeVisualScene.hiddenRanges] 做
 * `ClipOp.Difference` 裁切，使 `drawContent()`（BasicTextField 的完整绘制）只在
 * 非 hidden 区域可见（只在绘制阶段排除动画接管区域，不改 BasicTextField 输出表示），
 * 然后画动画字（[drawVisualScene]）和视觉光标（[drawVisualCursorRect]）。
 *
 * 不再用"拿主题背景色盖正文"（旧 `clipSystemTextForHiddenRanges` + `drawPath(backgroundColor)`）—
 * 那会把 selection/search highlight 一起盖掉，背景非纯 surface 时会画出错误底色。
 * 现在用 `ClipOp.Difference` 只裁切绘制区域，不引入任何颜色，selection/search highlight
 * 由 BasicTextField 自己画，裁切后自然只在非 hidden 区域可见。
 *
 * 本 draw 层统一三件事，全部使用同一个 [TextLayoutResult]（latestLayout）、
 * 同一个 scrollY 和同一个 frame clock（由 [LaunchedEffect] 的 [withFrameNanos] 提供）：
 *
 * 1. **正文裁切**：对 `scene.hiddenRanges` 合并成单个 [Path] 后用
 *    `clipPath(path, clipOp = ClipOp.Difference)` 包住 `drawContent()`，
 *    使 BasicTextField 的完整绘制只在非 hidden 区域可见。
 *    hiddenRanges 为空时直接 `drawContent()` 画完整原正文。
 * 2. **动画字重画**：[drawVisualScene] — 从原 [ComposeTextAnimationOverlay] 搬来，逻辑不变。
 * 3. **视觉光标**：drawsVisualCursor 时，cursorRect 从 scene.cursorRect
 *    ?: [computeRestingCursorRect] ?: restingCursorRect 读取，[drawVisualCursorRect] 绘制。
 *
 * BasicTextField 始终画完整真实正文，本 draw 层只在绘制阶段裁切动画接管区域，
 * onTextLayout 只因真实正文/几何变化触发，不再因 hiddenRanges 变化触发二次 layout，断开回路。
 *
 * @param visualState 编辑器视觉状态。
 * @param scrollY 当前滚动位置（px）— 与 BasicTextField 共享 scrollState.value。
 * @param textColor 文字颜色 — 从主题 role 注入。
 * @param cursorColor 视觉光标颜色 — 从主题 role 注入。
 * @param liveSelection 直接从 [TextFieldState.selection] 读取 — 纯 selection 变化时
 *   onTextLayout 不一定回调，restingCursorRect 可能停在旧位置，需要 live selection 实时算。
 * @param modifier Compose modifier。
 * @param content 被包住的正文 composable — 通常是 [BasicTextField]。
 *   本 draw 层用 `drawWithContent` 在绘制阶段裁切 hiddenRanges，使 content 只在非 hidden 区域可见。
 */
@Composable
@Suppress("LongParameterList")
fun EditorTextFieldDrawLayer(
    visualState: ComposeEditorVisualState,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    /**
     * #684 评论 5663032418 断点3：live selection — 直接从 [TextFieldState.selection] 读取。
     */
    liveSelection: TextRange?,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
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
                .drawWithContent {
                    val scene = visualScene

                    // 1. 正文裁切：对 hiddenRanges 做 ClipOp.Difference 裁切，
                    //    使 drawContent()（BasicTextField 的完整绘制）只在非 hidden 区域可见。
                    //    #698 评论 5699401353 修复1：不再用 drawPath(backgroundColor) 盖背景色，
                    //    而是用 clipPath + ClipOp.Difference 只裁切绘制区域 —
                    //    selection/search highlight 由 BasicTextField 自己画，
                    //    裁切后自然只在非 hidden 区域可见，不引入任何颜色。
                    // #698 评论 5700812160：hiddenPath 必须与动画字、视觉光标在同一视口坐标系。
                    // drawTranslatedRangeText 用 translate.y - scrollY、drawVisualCursorRect 用
                    // rect.top/bottom - scrollY，唯独裁切 path 没减 scrollY 会导致滚动后裁切位置
                    // 与动画字错开，出现重影/缺字/局部空白。这里把 scrollY 传进 buildHiddenPath，
                    // 在合并 path 时带视口偏移把正文坐标换算到当前视口坐标，不改 BasicTextField
                    // 自己的滚动（不在外层整体 translate(-scrollY) 后再 drawContent()）。
                    val hiddenPath =
                        buildHiddenPath(
                            hiddenRanges = scene.hiddenRanges,
                            layout = latestLayout,
                            scrollY = scrollY,
                        )
                    if (hiddenPath != null) {
                        clipPath(
                            path = hiddenPath,
                            clipOp = ClipOp.Difference,
                        ) {
                            this@drawWithContent.drawContent()
                        }
                    } else {
                        // hiddenRanges 为空：直接画完整原正文。
                        drawContent()
                    }

                    // 2. 动画帧：直接读 visualScene.units。
                    //    每个 unit 的 alpha、屏幕位置已经由 timeline 按当前帧时间算好，
                    //    draw 阶段不再二次插值。
                    if (scene.units.isNotEmpty()) {
                        drawVisualScene(
                            scene = scene,
                            scrollY = scrollY,
                            textColor = textColor,
                        )
                    }

                    // 3. 光标：smooth cursor 开启时本 draw 层整个会话拥有光标。
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
    ) {
        content()
    }
}

/**
 * #698 评论 5699401353 修复1：把 [hiddenRanges] 合并成单个 [Path] —
 * 对每个 hiddenRange 用 [TextLayoutResult.getPathForRange] 取 path，
 * 用 [Path.addPath] 拼接成合并 path，供 `clipPath(clipOp = ClipOp.Difference)` 一次裁切。
 *
 * 越界检查（range.end <= result.layoutInput.text.length）和 try/catch 防御异常。
 * layout 为 null 时返回 null（首帧或章节切换中）。hiddenRanges 为空或全部无效时返回 null。
 *
 * #698 评论 5700812160：[scrollY] 把 [TextLayoutResult.getPathForRange] 得到的正文坐标 path
 * 换算到当前编辑器视口坐标。同一 draw 层里 [drawTranslatedRangeText] 用 `translate.y - scrollY`、
 * [drawVisualCursorRect] 用 `rect.top/bottom - scrollY`，唯独裁掉 BasicTextField 原字的
 * hidden path 之前没减 `scrollY`，编辑器向下滚过一段距离后裁切位置（layoutY）与动画字位置
 * （layoutY - scrollY）错开，会出现重影、缺字或局部空白。这里给 [Path.addPath] 传视口偏移
 * `Offset(0f, -scrollY)` 统一三者坐标系，不改 BasicTextField 自己的滚动。
 *
 * @param hiddenRanges 需要裁切的正文 range 列表。
 * @param layout 当前正文 layout 快照；null 时返回 null。
 * @param scrollY 当前滚动位置（px）— 与 BasicTextField 共享 scrollState.value，
 *   用于把正文坐标 path 换算到视口坐标。
 */
private fun DrawScope.buildHiddenPath(
    hiddenRanges: List<TextRange>,
    layout: ComposeLayoutSnapshot?,
    scrollY: Int,
): Path? {
    if (hiddenRanges.isEmpty() || layout == null) return null
    val result = layout.result
    val textLength = result.layoutInput.text.length
    var combined: Path? = null
    for (range in hiddenRanges) {
        if (range.start >= range.end) continue
        if (range.end > textLength) continue
        try {
            val path: Path = result.getPathForRange(range.start, range.end)
            // #698 评论 5700812160：给 addPath 传视口偏移，把正文坐标 path 换算到当前视口坐标，
            // 与 drawTranslatedRangeText（translate.y - scrollY）、drawVisualCursorRect
            // （rect.top/bottom - scrollY）统一坐标系。
            val viewportOffset = Offset(0f, -scrollY.toFloat())
            val target = combined ?: Path()
            target.addPath(path, viewportOffset)
            combined = target
        } catch (_: Throwable) {
            // 越界或几何异常：跳过此 range，不阻断其他绘制。
        }
    }
    return combined
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
 * 会停在旧位置。draw 层用本函数 + [ComposeEditorVisualState.latestLayout] + liveSelection
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
