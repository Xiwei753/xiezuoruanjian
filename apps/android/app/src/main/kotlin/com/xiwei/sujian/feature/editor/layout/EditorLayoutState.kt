package com.xiwei.sujian.feature.editor.layout

import androidx.compose.foundation.ScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.projection.ViewportAnchor

/**
 * #641 评论1 第4节：排版层只认 [BasicTextField] 给出的 [TextLayoutResult]。
 *
 * [ComposeLayoutSnapshot] 是从系统 [TextLayoutResult] 读出的不可变快照，
 * 自动换行落在哪一行只有 [BasicTextField] 的 [TextLayoutResult] 一个答案。
 *
 * @param result 系统 [BasicTextField] 的 `onTextLayout` 给出的最终布局结果。
 * @param selection 当前选区（UTF-16 offset）。
 * @param scrollY 当前滚动位置（px）。
 * @param projection Issue #717 评论 5741910919：西文软断行显示投影。
 *     [result] 是含 U+200B 的 display 文本布局，而外部调用方持有的是 raw 正文 offset；
 *     [cursorRect]/[lineForOffset]/[boundingBox] 通过 [projection] 把 raw offset
 *     转成 display offset 再查 [result]。默认 [EditorSoftBreakProjection.identity]
 *     时 raw 与 display 一致，行为与旧实现相同。
 * @param rawText Issue #717 评论 5742904417 修复1 / 评论 5743443030 修复1：原始正文（不含 U+200B）。
 *     用于文本身份/diff/intent 匹配/offset-map 长度；而 [result]+[projection] 用于
 *     几何/行/path/cursor。visual pipeline 的文本身份判断必须用 [rawText]，
 *     不能用 `result.layoutInput.text.text`（那是 display 文本，含 U+200B）。
 *     `null` 表示"没传"（测试/旧调用方），[effectiveRawText] 会 fallback 到
 *     `result.layoutInput.text.text`；`""` 表示"真实空正文"，直接使用。
 */
data class ComposeLayoutSnapshot(
    val result: TextLayoutResult,
    val selection: TextRange,
    val scrollY: Int,
    val projection: EditorSoftBreakProjection = EditorSoftBreakProjection.identity(),
    val rawText: String? = null,
)

/**
 * Issue #717 评论 5742904417 修复1 / 评论 5743443030 修复1：effective raw text —
 *
 * 生产代码中 [ComposeEditorVisualState.onAuthoritativeLayout] 会显式传入正确的 [rawText]；
 * 但测试和旧调用方可能不传（默认 `null`），此时从 [result.layoutInput.text.text] 推导。
 * 测试中的 TextLayoutResult 不含 U+200B，result 即 raw，fallback 安全。
 *
 * Issue #717 评论 5743443030 修复1：[rawText] 改成 `String?` 后，
 * `null` 表示"没传"（fallback 到 result.layoutInput.text.text），
 * 非 `null`（含 `""`）时直接使用。这样"真实空正文"（rawText=""）和"没传"（rawText=null）
 * 在类型上区分开，旧实现用 `isEmpty()` 判断会把真实空正文误判成"没传"。
 *
 * visual pipeline 的文本身份判断一律用 [effectiveRawText]，不直接用 [rawText]。
 */
val ComposeLayoutSnapshot.effectiveRawText: String
    get() = rawText ?: result.layoutInput.text.text

/**
 * #641 评论1 第5节：视觉光标矩形 — 从真实 [TextLayoutResult] 取，
 * 不再由动画层或 View 自行推算。
 *
 * #706 评论 5715257924 症状3：统一 caret 几何入口。
 *
 * Issue #725 评论 5750735497：停止自绘屏幕 caret 后，本函数只供"查询光标几何"使用
 * （如视口锚点、诊断），屏幕 caret 始终由 BasicTextField 自己画。
 *
 * 空段落首行缩进已进入显示布局本身（OutputTransformation + projection 零宽占位符），
 * 不再有 caret-only 特判分支。正文、系统选区手柄都消费同一份 transformed TextLayoutResult。
 *
 * raw→display 映射统一走 [EditorSoftBreakProjection.rawToDisplay]（range 映射），
 * 不再区分 wedge Start / wedge End affinity — caret 在 U+200B 的哪一侧只由 BasicTextField 决定。
 */
fun ComposeLayoutSnapshot.cursorRect(offset: Int): Rect {
    val text = result.layoutInput.text.text
    val safeOffset = offset.coerceIn(0, text.length)
    val displayOffset = projection.rawToDisplay(safeOffset).coerceIn(0, text.length)
    return result.getCursorRect(displayOffset)
}

/**
 * #641 评论1 第4节：行信息访问 — 直接转发 [TextLayoutResult]，
 * 不缓存第二份行段。
 *
 * Issue #717 评论 5741910919：外部传入的 offset 是 raw 正文 offset，
 * 经 [projection] 映射成 display offset 再查 [result]。
 */
fun ComposeLayoutSnapshot.lineForOffset(offset: Int): Int {
    val displayOffset =
        projection.rawToDisplay(offset).coerceIn(0, result.layoutInput.text.text.length)
    return result.getLineForOffset(displayOffset)
}

fun ComposeLayoutSnapshot.boundingBox(offset: Int): Rect {
    val displayOffset =
        projection.rawToDisplay(offset).coerceIn(0, result.layoutInput.text.text.length)
    return result.getBoundingBox(displayOffset)
}

/**
 * Issue #717 评论 5742273757 修复3：raw→display 映射收口入口。
 *
 * VisualTextUnit.range / VisualTextUnit.targetRange / ComposeVisualScene.hiddenRanges
 * 都是正文 raw UTF-16 坐标。在传给 TextLayoutResult 之前必须经过 projection 映射。
 * 以下三个方法统一收口，所有视觉路径通过 snapshot 做 raw→display，不再逐个地方临时 +offset。
 *
 * raw TextRange → display Path。range 来自正文/visual unit（raw 坐标）。
 */
fun ComposeLayoutSnapshot.pathForRawRange(rawRange: TextRange): Path {
    val displayRange = projection.toDisplayRange(rawRange)
    val textLength = result.layoutInput.text.length
    return result.getPathForRange(
        displayRange.start.coerceIn(0, textLength),
        displayRange.end.coerceIn(0, textLength),
    )
}

/** raw TextRange → display Rect?（path bounds）。range 来自正文/visual unit（raw 坐标）。 */
fun ComposeLayoutSnapshot.boundsForRawRange(rawRange: TextRange): Rect? {
    val displayRange = projection.toDisplayRange(rawRange)
    val textLength = result.layoutInput.text.length
    if (displayRange.start >= displayRange.end) return null
    if (displayRange.end > textLength) return null
    return try {
        result.getPathForRange(displayRange.start, displayRange.end).getBounds()
    } catch (_: Throwable) {
        null
    }
}

/** raw offset → display line index。offset 来自正文/visual unit（raw 坐标）。 */
fun ComposeLayoutSnapshot.lineForRawOffset(rawOffset: Int): Int {
    val displayOffset =
        projection.rawToDisplay(rawOffset).coerceIn(0, result.layoutInput.text.text.length)
    return result.getLineForOffset(displayOffset)
}

/**
 * Issue #717 评论 5742904417 修复2：raw line end for raw offset。
 *
 * [TextLayoutResult.getLineEnd] 返回的是 display offset（含 U+200B），
 * 但 retained move 切片需要 raw offset。本函数先把 rawOffset 映射到 display 查行，
 * 再把 display lineEnd 映射回 raw offset。
 */
fun ComposeLayoutSnapshot.rawLineEndForRawOffset(rawOffset: Int): Int {
    val line = lineForRawOffset(rawOffset)
    val displayEnd = result.getLineEnd(line)
    return projection.displayToRaw(displayEnd)
}

/**
 * #644 评论 5462826712 第4节：编辑器视口状态 — 管理滚动/视口。
 *
 * 状态只保存：
 * - [scrollState]：Compose ScrollState
 * - [latestLayout]：最新的 TextLayoutResult
 * - [latestProjection]：与 [latestLayout] 对应的软断行投影
 * - [pendingAnchor]：待恢复的视口锚点
 * - [restoredForCurrentAnchor]：当前锚点是否已恢复
 *
 * Issue #717 评论 5742273757 修复4：保存 projection 做 raw↔display 转换。
 * snapshotAnchor：display lineStart → projection.displayToRaw() → ViewportAnchor（raw 坐标）
 * restoreFromAnchor：ViewportAnchor raw → projection.rawToDisplay() → getLineForOffset（display 坐标）
 */
class EditorViewportState(
    val scrollState: ScrollState,
    initialAnchor: ViewportAnchor?,
) {
    private var latestLayout: TextLayoutResult? = null
    private var latestProjection: EditorSoftBreakProjection = EditorSoftBreakProjection.identity()
    private var pendingAnchor: ViewportAnchor? = initialAnchor
    private var restoredForCurrentAnchor: Boolean = false

    /**
     * #644 评论 5462826712 第4节：系统给出权威布局时调用。
     * 有 pending anchor 时只恢复一次。
     *
     * Issue #717 评论 5742273757 修复4：接收 projection 参数，保存用于 raw↔display 转换。
     *
     * @return 需要 scrollTo 的 Y 值；null 表示无需恢复。调用方用 coroutine scope 调 scrollTo。
     */
    fun onLayout(
        result: TextLayoutResult,
        projection: EditorSoftBreakProjection = EditorSoftBreakProjection.identity(),
    ): Int? {
        latestLayout = result
        latestProjection = projection
        val anchor = pendingAnchor ?: return null
        if (restoredForCurrentAnchor) return null
        restoredForCurrentAnchor = true
        return restoreFromAnchor(result, anchor, projection)
    }

    /**
     * #644 评论 5462826712 第4节：用当前 scrollState + TextLayoutResult 算逻辑锚点。
     *
     * Issue #717 评论 5742273757 修复4：display lineStart → projection.displayToRaw() → ViewportAnchor（raw 坐标）。
     */
    fun snapshotAnchor(): ViewportAnchor? {
        val layout = latestLayout ?: return null
        val projection = latestProjection
        val scrollY = scrollState.value
        val line = layout.getLineForVerticalPosition(scrollY.toFloat())
        val displayLineStart = layout.getLineStart(line)
        // display offset → raw offset，存入 anchor 的是 raw 坐标
        val textOffsetUtf16 = projection.displayToRaw(displayLineStart)
        val lineTop = layout.getLineTop(line)
        val lineBottom = layout.getLineBottom(line)
        val fraction =
            if (lineBottom > lineTop) {
                ((scrollY - lineTop) / (lineBottom - lineTop)).coerceIn(0f, 1f)
            } else {
                0f
            }
        return ViewportAnchor(
            textOffsetUtf16 = textOffsetUtf16,
            offsetWithinLineFraction = fraction,
        )
    }

    /**
     * #644 评论 5462826712 第4节：用新 layout 反算滚动位置。
     *
     * Issue #717 评论 5742273757 修复4：ViewportAnchor raw → projection.rawToDisplay() → getLineForOffset（display 坐标）。
     *
     * @return 需要 scrollTo 的 Y 值；null 表示 anchor 无效。
     */
    private fun restoreFromAnchor(
        layout: TextLayoutResult,
        anchor: ViewportAnchor,
        projection: EditorSoftBreakProjection,
    ): Int? {
        // raw offset → display offset，再调 getLineForOffset
        val displayOffset =
            projection.rawToDisplay(anchor.textOffsetUtf16).coerceIn(0, layout.layoutInput.text.text.length)
        val line = layout.getLineForOffset(displayOffset)
        val lineTop = layout.getLineTop(line)
        val lineBottom = layout.getLineBottom(line)
        val lineHeight = lineBottom - lineTop
        val y = (lineTop + lineHeight * anchor.offsetWithinLineFraction).toInt()
        return y.coerceIn(0, scrollState.maxValue)
    }
}

/**
 * #644 评论 5462826712 第4节：remember EditorViewportState —
 * ScrollState 也跟 targetId 一起新建，不能 target 换了只换 wrapper。
 */
@Composable
fun rememberEditorViewportState(
    targetId: String,
    initialAnchor: ViewportAnchor?,
): EditorViewportState {
    val scrollState = remember(targetId) { ScrollState(0) }
    return remember(targetId) {
        EditorViewportState(
            scrollState = scrollState,
            initialAnchor = initialAnchor,
        )
    }
}
