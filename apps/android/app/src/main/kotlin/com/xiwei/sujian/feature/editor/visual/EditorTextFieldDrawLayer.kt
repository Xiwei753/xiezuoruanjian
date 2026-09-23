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
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.drawText
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange
import com.xiwei.sujian.feature.editor.layout.pathForRawRange

/**
 * Issue #737：编辑器绘制链根 — 重写为只接收一份 motion sample。
 *
 * 删除旧架构的两条分支：
 * - 旧 `drawCurrentEditorFrame` 接收 `scene + layout + caretRect`
 * - 旧 `drawVisualScene` 消费 [ComposeVisualScene]（已删除）
 *
 * 新架构：
 * - [drawCurrentEditorFrame] 接收 `motionSample + layout + restingCaretRect`。
 * - motionSample != null 且 [CoordinatedEditMotion.Sample.isValid]：
 *   画 BasicTextField 内容（裁掉 hiddenRanges）+ glyph overlays + animated caret。
 * - 否则：画 BasicTextField 内容 + resting caret。
 *
 * 一笔编辑只有一个 motion — 不再分别维护"文字动画是否 active"和"光标动画是否 active"。
 *
 * BasicTextField 始终画完整真实正文，本 draw 层只在绘制阶段裁切动画接管区域，
 * onTextLayout 只因真实正文/几何变化触发，不再因 hiddenRanges 变化触发二次 layout。
 *
 * @param visualState 编辑器视觉状态。
 * @param scrollY 当前滚动位置（px）— 与 BasicTextField 共享 scrollState.value。
 * @param textColor 文字颜色 — 从主题 role 注入。
 * @param cursorColor 光标颜色 — 从主题 role 注入。
 *   Issue #728 评论 5754045689：系统 caret 已透明（cursorBrush = Color.Transparent），
 *   draw 层用 motion sample 的 caretRect 画 caret。
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
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val density = LocalDensity.current

    // #708 评论 5723410606 第一节：每帧状态不在 Composable 主体读取 —
    // motionSample / latestLayout 改成只在 drawWithContent 内取 drawSnapshot()。
    // frameRequestVersion 继续作为启动帧循环的低频信号。
    // Issue #732 评论 5763493968 第3节：policy 改变、selection-only caret target、patch 入队
    // 都会唤醒同一个帧循环 — 所有引用 frameRequestVersion 的地方同步改名。
    val frameRequestVersion by visualState.frameRequestVersion.collectAsStateWithLifecycle()

    // #689 评论 5674631257 步骤8：只在有活动 motion 时用 Compose 的帧时钟推进。
    // #689 评论 5676120929 问题1：用 frameRequestVersion 唤醒帧循环，真正数据从队列 drain。
    // #689 评论 5675270164 缺陷6：全过程只用 withFrameNanos 的 frameTimeNanos。
    // Issue #732 评论 5763493968 第3节：单一 withFrameNanos 循环 —
    // 每帧顺序固定为：应用 pending policy → 合并/消费 patch → 创建同一笔
    // CoordinatedEditMotion → sample motion → draw。
    // Issue #737：sampleVisualScene 返回 motion sample（不再返回 ComposeVisualScene），
    // 但方法名保留以减少调用点改动。
    LaunchedEffect(frameRequestVersion) {
        if (frameRequestVersion <= 0L) return@LaunchedEffect
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
                    // #708 评论 5723410606 第一节：真正的画面状态只在 drawWithContent 内取 —
                    // 动画每帧只重跑 Draw，不重新执行 BasicTextField 的 Composition/Layout。
                    val snapshot = visualState.drawSnapshot()
                    drawCurrentEditorFrame(
                        motionSample = snapshot.motionSample,
                        layout = snapshot.layout,
                        restingCaretRect = snapshot.restingCaretRect,
                        scrollY = scrollY,
                        textColor = textColor,
                        cursorColor = cursorColor,
                        density = density,
                        drawContent = { this@drawWithContent.drawContent() },
                    )
                },
    ) {
        content()
    }
}

/**
 * Issue #728 评论 5754045689：把 [hiddenRanges] 合并成单个 [Path] —
 * 对每个 hiddenRange 用 [TextLayoutResult.getPathForRange] 取 path，
 * 用 [Path.addPath] 拼接成合并 path，供 `clipPath(clipOp = ClipOp.Difference)` 一次裁切。
 *
 * 越界检查（range.end <= result.layoutInput.text.length）和 try/catch 防御异常。
 * layout 为 null 时返回 null（首帧或章节切换中）。hiddenRanges 为空或全部无效时返回 null。
 *
 * #698 评论 5700812160：[scrollY] 把 [TextLayoutResult.getPathForRange] 得到的正文坐标 path
 * 换算到当前编辑器视口坐标。
 *
 * Issue #737 评论 5781084709 修复点 4：[hiddenRanges] 只含 Inserted 角色的 current-layout ranges
 * （属于 newLayout，裁掉 BasicTextField 里的对应正文是正确的）。
 * Deleted ghost 不进入此列表 — deleted 通过 [drawGlyphOverlay] 用自己的 oldLayout 绘制。
 *
 * Issue #739 评论 5787769674：[hiddenRanges] 语义扩展为"当前 motion 接管的 current-layout ranges" —
 * 包含 inserted range + retained move 的 destination newRange。
 * retained move 的 destination newRange 在 motion 未结束时由动画层接管（裁掉 BasicTextField 里
 * 对应的最终位置正文），完成后释放交还 BasicTextField。实际 hiddenRanges 内容由
 * [CoordinatedEditMotion.sample] 产出，draw 层只消费 [CoordinatedEditMotion.Sample.hiddenRanges]，
 * 所以 buildHiddenPath 的裁切逻辑不用改，只更新注释说明语义扩展。
 *
 * @param hiddenRanges 需要裁切的正文 range 列表。
 *   **语义（#711 评论 5738906634 + #737 评论 5781084709 修复点 4 + #739 评论 5787769674）**：
 *   表示"这一帧由动画层接管的 current-layout ranges"（inserted range + retained destination newRange），
 *   不表示"被删除的旧字 ghost"（deleted 用 overlay 自带 oldLayout 画）。
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
    // Issue #717 评论 5742904417 修复1：hiddenRanges 是 raw 坐标，textLength 用 rawText 长度。
    val textLength = layout.result.layoutInput.text.text.length
    var combined: Path? = null
    for (range in hiddenRanges) {
        if (range.start >= range.end) continue
        // Issue #717 评论 5742273757 修复3：hiddenRanges 是 raw 坐标，
        // 通过 snapshot.pathForRawRange 做 raw→display 映射再取 path。
        try {
            val path: Path = layout.pathForRawRange(range)
            // #698 评论 5700812160：给 addPath 传视口偏移，把正文坐标 path 换算到当前视口坐标，
            // 与 drawTranslatedRangeText（translate.y - scrollY）统一坐标系。
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
 * Issue #737：绘制单个 glyph overlay —
 * 从 [CoordinatedEditMotion.GlyphOverlay] 读取 range / layout / role / clipFraction，
 * 在所属 layout 的真实位置画一段 range 文字，按 clipFraction 裁切可见区域。
 *
 * Issue #737 评论 5781084709 修复点 4：用 overlay 自带的 [CoordinatedEditMotion.GlyphOverlay.layout]
 * 画 ghost — inserted overlay 用 newLayout，deleted overlay 用 oldLayout。
 * 不用当前 BasicTextField 的 layout，避免 deleted range 拿旧 offset 裁新正文。
 *
 * - [GlyphRole.Inserted]（吐字）：clipRect = [left, left + width * fraction]
 * - [GlyphRole.Deleted]（吞字）：clipRect = [left, left + width * fraction]
 *   （统一边界模型，fraction 从 1→0 表示完全可见→完全被吞掉）
 *
 * @param overlay glyph overlay。
 * @param scrollY 当前滚动位置（px）。
 * @param textColor 文字颜色。
 */
private fun DrawScope.drawGlyphOverlay(
    overlay: CoordinatedEditMotion.GlyphOverlay,
    scrollY: Int,
    textColor: Color,
) {
    val range = overlay.range
    if (range.start >= range.end) return
    val snapshot = overlay.layout
    // Issue #717 评论 5742904417 修复1：range 是 raw 坐标，边界检查用 rawText 长度。
    if (range.end > snapshot.result.layoutInput.text.text.length) return
    // Issue=717 评论 5742273757 修复3：range 是 raw 坐标，通过 snapshot 做 raw→display。
    val bounds = snapshot.boundsForRawRange(range) ?: return
    val clipFraction = overlay.clipFraction.coerceIn(0f, 1f)
    if (clipFraction <= 0f) return
    // glyph overlay 在 layout 真实位置画（translate = Zero），只按 clipFraction 裁切可见区域
    val translate = Offset.Zero
    val clipRect =
        if (clipFraction < 1f) {
            Rect(
                left = bounds.left,
                top = bounds.top,
                right = bounds.left + bounds.width * clipFraction,
                bottom = bounds.bottom,
            )
        } else {
            null
        }
    drawTranslatedRangeText(
        snapshot = snapshot,
        range = range,
        translate = translate,
        alpha = 1f,
        scrollY = scrollY,
        textColor = textColor,
        clipRect = clipRect,
    )
}

/**
 * Issue #739 评论 5787769674：绘制单个 retained reflow overlay —
 * 自动换行时被挤到下一行的"保留文字"的一帧绘制。
 *
 * 用 [CoordinatedEditMotion.RetainedOverlay.oldLayout] + [oldRange] 画原文字，
 * 按 [CoordinatedEditMotion.RetainedOverlay.translate]（从 Zero 插值到 newTopLeft - oldTopLeft）平移。
 * 不做 reveal clip（clipRect = null），不改变 alpha（alpha = 1f）— 保留文字全程可见，只做位置平移。
 *
 * @param overlay retained reflow overlay。
 * @param scrollY 当前滚动位置（px）。
 * @param textColor 文字颜色。
 */
private fun DrawScope.drawRetainedOverlay(
    overlay: CoordinatedEditMotion.RetainedOverlay,
    scrollY: Int,
    textColor: Color,
) {
    drawTranslatedRangeText(
        snapshot = overlay.oldLayout,
        range = overlay.oldRange,
        translate = overlay.translate,
        alpha = 1f,
        scrollY = scrollY,
        textColor = textColor,
        clipRect = null,
    )
}

/**
 * 按 translate 偏移绘制一段 range 文字。
 *
 * #703 评论 B：[clipRect] 用于空间进度驱动吞吐字 —
 * 非 null 时用 clipPath(clipRect) 裁切 glyph 可见区域，
 * 使光标经过哪里文字才出现/消失到哪里。
 *
 * Issue #717 评论 5742273757 修复3：改为接收 [ComposeLayoutSnapshot]，
 * range 是 raw 坐标，通过 [pathForRawRange] 做 raw→display 映射再取 path。
 */
@Suppress("LongParameterList")
private fun DrawScope.drawTranslatedRangeText(
    snapshot: ComposeLayoutSnapshot,
    range: TextRange,
    translate: Offset,
    alpha: Float,
    scrollY: Int,
    textColor: Color,
    clipRect: Rect? = null,
) {
    val result = snapshot.result
    if (range.start >= range.end) return
    if (alpha <= 0f) return
    val path = snapshot.pathForRawRange(range)
    withTransform({
        translate(
            left = translate.x,
            top = translate.y - scrollY.toFloat(),
        )
    }) {
        // #703 评论 B：空间进度驱动吞吐字 — 用 clipRect 裁切 glyph 可见区域
        if (clipRect != null) {
            val clipPath = Path().apply { addRect(clipRect) }
            clipPath(clipPath) {
                clipPath(path) {
                    drawText(
                        textLayoutResult = result,
                        color = textColor,
                        topLeft = Offset.Zero,
                        alpha = alpha,
                    )
                }
            }
        } else {
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
}

/**
 * Issue #737：绘制完整编辑器当前帧 — 只接收一份 motion sample。
 *
 * 删除旧架构的 `drawVisualScene`（不再消费 [ComposeVisualScene]）。
 *
 * - motionSample != null 且 [CoordinatedEditMotion.Sample.isValid]：
 *   1. 画 BasicTextField 内容（裁掉 hiddenRanges）
 *   2. 画 retained move overlays（Issue #739 评论 5787769674：自动换行保留文字的位置平移）
 *   3. 画 glyph overlays（插入/删除的文字）
 *   4. 画 animated caret
 * - 否则：画 BasicTextField 内容 + resting caret
 *
 * Issue #737 评论 5781084709 修复点 4：[buildHiddenPath] 只用
 * [CoordinatedEditMotion.Sample.hiddenRanges]（含 inserted current-layout ranges
 * + retained destination newRange）裁 BasicTextField。
 * Deleted ghost 通过 [drawGlyphOverlay] 用 overlay 自带的 oldLayout 绘制，不裁 BasicTextField。
 *
 * Issue #739 评论 5787769674：retained overlay 画在 BasicTextField 之后、glyph overlay 之前。
 * 保留文字用 oldLayout + oldRange 画原文字，按 translate 平移，不做 reveal clip 不改 alpha。
 * hiddenRanges 语义现在是"当前 motion 接管的 current-layout ranges"
 * （inserted + retained destination newRange）。
 *
 * Issue #737 评论 5782769758：layout 和 motionSample 来自同一 presentation generation
 * （由 [ComposeEditorVisualState] 原子切换），draw 层只消费这一份原子 snapshot，
 * 不自己判断代际，不会出现 old sample + new layout 混搭配。
 *
 * @param motionSample 当前帧的 motion 采样结果 — 由 [CoordinatedEditMotion.sample] 产生。
 *   null 或无效时画平台最终正文 + [restingCaretRect]。
 * @param layout 当前 layout 快照。
 * @param restingCaretRect 静止 caret rect — 无 active motion 时画这个 caret。
 * @param scrollY 当前滚动位置。
 * @param textColor 文字颜色。
 * @param cursorColor 光标颜色。
 * @param density 密度信息。
 * @param drawContent 绘制 BasicTextField 内容的回调。
 */
@Suppress("LongParameterList")
internal fun DrawScope.drawCurrentEditorFrame(
    motionSample: CoordinatedEditMotion.Sample?,
    layout: ComposeLayoutSnapshot?,
    restingCaretRect: Rect?,
    scrollY: Int,
    textColor: Color,
    cursorColor: Color,
    density: androidx.compose.ui.unit.Density,
    drawContent: () -> Unit,
) {
    if (motionSample != null && motionSample.isValid) {
        // sample 有效：同时画 animated caret + glyph overlay
        // 1. 先画 BasicTextField 内容，但裁掉 hiddenRanges
        val hiddenPath =
            buildHiddenPath(
                hiddenRanges = motionSample.hiddenRanges,
                layout = layout,
                scrollY = scrollY,
            )
        if (hiddenPath != null) {
            clipPath(
                path = hiddenPath,
                clipOp = ClipOp.Difference,
            ) {
                drawContent()
            }
        } else {
            drawContent()
        }
        // 2. 画 retained move overlays（Issue #739 评论 5787769674：自动换行保留文字的位置平移）
        for (overlay in motionSample.retainedOverlays) {
            drawRetainedOverlay(
                overlay = overlay,
                scrollY = scrollY,
                textColor = textColor,
            )
        }
        // 3. 画 glyph overlays（插入/删除的文字）
        for (overlay in motionSample.glyphOverlays) {
            drawGlyphOverlay(
                overlay = overlay,
                scrollY = scrollY,
                textColor = textColor,
            )
        }
        // 4. 画 animated caret
        if (cursorColor != Color.Transparent) {
            drawVisualCaretRect(
                caretRect = motionSample.caretRect,
                scrollY = scrollY,
                cursorColor = cursorColor,
                density = density,
            )
        }
    } else {
        // sample 不存在或无效：画平台最终正文 + resting caret
        drawContent()
        if (restingCaretRect != null && cursorColor != Color.Transparent) {
            drawVisualCaretRect(
                caretRect = restingCaretRect,
                scrollY = scrollY,
                cursorColor = cursorColor,
                density = density,
            )
        }
    }
}

/**
 * Issue #728 评论 5754045689：画统一 motion caret —
 * 用 [CoordinatedEditMotion.Sample.caretRect] 给的 rect 画一条竖线。
 *
 * rect.width > 0 时直接用 rect 的宽度（来自 TextLayoutResult 的 cursor rect）；
 * rect.width == 0 时用 2dp 默认宽度（系统 cursor 的标准宽度）。
 * rect 已是正文坐标系，需要减 scrollY 换算到视口坐标。
 *
 * @param caretRect caret 的 rect（正文坐标系）。
 * @param scrollY 当前滚动位置（px）。
 * @param cursorColor 光标颜色。
 * @param density 密度信息 — 用于 dp→px 换算。
 */
private fun DrawScope.drawVisualCaretRect(
    caretRect: Rect,
    scrollY: Int,
    cursorColor: Color,
    density: androidx.compose.ui.unit.Density,
) {
    val viewportTop = caretRect.top - scrollY.toFloat()
    val viewportBottom = caretRect.bottom - scrollY.toFloat()
    val width = caretRect.width
    val caretWidthPx =
        if (width > 0f) {
            width
        } else {
            with(density) { 2.dp.toPx() }
        }
    val caretLeft =
        if (width > 0f) {
            caretRect.left
        } else {
            // width == 0：rect 是单条竖线位置，以 left 为中心画 caretWidthPx 宽
            caretRect.left - caretWidthPx / 2f
        }
    drawRect(
        color = cursorColor,
        topLeft = Offset(caretLeft, viewportTop),
        size = androidx.compose.ui.geometry.Size(caretWidthPx, viewportBottom - viewportTop),
    )
}
