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
import androidx.compose.ui.text.style.ResolvedTextDirection
import androidx.compose.ui.unit.isSpecified
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
 * 旧实现 `cursorRect() = result.getCursorRect(selection.end)` 对空段落首行缩进不修正，
 * 导致 Enter 产生空段落时光标落在 x=0 而非缩进后的位置。
 *
 * 新增 [cursorRect] 的 offset 版本作为所有自绘/动画光标的唯一事实源：
 * 普通位置直接返回 Compose 原生 [TextLayoutResult.getCursorRect]；
 * 只对"逻辑段落开头且该段落当前为空"的 caret 按首行缩进修正 X 坐标，
 * Y/高度/caret 宽度沿用原始 rect。
 */
fun ComposeLayoutSnapshot.cursorRect(offset: Int): Rect {
    val text = result.layoutInput.text.text
    val safeOffset = offset.coerceIn(0, text.length)
    // Issue #717 评论 5741910919：raw offset → display offset。
    // result 是含 U+200B 的 display 文本布局，外部传入的 offset 是 raw 正文 offset，
    // 需经投影映射到 display offset 再查 TextLayoutResult。
    val displayOffset = projection.rawToDisplay(safeOffset).coerceIn(0, text.length)
    val raw = result.getCursorRect(displayOffset)

    // 只对"逻辑段落开头且该段落当前为空"的 caret 修正首行缩进。
    // atParagraphStart：offset 在段落首字符处（文档开头或前一个字符是 \n）。
    // emptyParagraph：offset 所在段落为空（文档末尾或当前字符是 \n）。
    val atParagraphStart = displayOffset == 0 || text[displayOffset - 1] == '\n'
    val emptyParagraph = displayOffset == text.length || text[displayOffset] == '\n'
    if (!atParagraphStart || !emptyParagraph) return raw

    val textIndent = result.layoutInput.style.textIndent ?: return raw
    val firstLine = textIndent.firstLine
    if (!firstLine.isSpecified || firstLine.value == 0f) return raw

    // 把首行缩进 sp 换算成 px，按段落方向加到行起始边。
    val density = result.layoutInput.density
    val firstLinePx = with(density) { firstLine.toPx() }
    if (firstLinePx == 0f) return raw

    val line = result.getLineForOffset(displayOffset)
    val direction = result.getParagraphDirection(displayOffset)
    val newLeft =
        when (direction) {
            ResolvedTextDirection.Ltr -> result.getLineLeft(line) + firstLinePx
            ResolvedTextDirection.Rtl -> result.getLineRight(line) - firstLinePx
            else -> raw.left
        }
    return Rect(
        left = newLeft,
        top = raw.top,
        right = newLeft + raw.width,
        bottom = raw.bottom,
    )
}

fun ComposeLayoutSnapshot.cursorRect(): Rect = cursorRect(selection.end)

/**
 * #706 评论 5718539128 修复3：空段落首行缩进 caret override 判定 —
 * 返回 true 当且仅当 [offset] 在空段落首位且该段落有非零首行缩进。
 *
 * 用于 smooth cursor 关闭时让 draw 层接管空段落缩进位置的静态 caret：
 * 关闭平滑光标时 Enter 后空段落走 BasicTextField 原生 raw caret（x=0），
 * 但 [cursorRect] 已含缩进修正。本函数让 [WritingEditorSurface] 知道何时
 * 把系统 cursor 设透明并让 draw 层画 [cursorRect] 的缩进位置。
 *
 * 判断逻辑与 [cursorRect] 的缩进修正分支完全一致，不引入第二套语义。
 */
fun ComposeLayoutSnapshot.isIndentedEmptyParagraphCaret(offset: Int): Boolean {
    val text = result.layoutInput.text.text
    val safeOffset = offset.coerceIn(0, text.length)
    // Issue #717 评论 5741910919：raw offset → display offset，与 cursorRect 保持一致。
    val displayOffset = projection.rawToDisplay(safeOffset).coerceIn(0, text.length)

    val atParagraphStart = displayOffset == 0 || text[displayOffset - 1] == '\n'
    val emptyParagraph = displayOffset == text.length || text[displayOffset] == '\n'
    if (!atParagraphStart || !emptyParagraph) return false

    val textIndent = result.layoutInput.style.textIndent ?: return false
    val firstLine = textIndent.firstLine
    if (!firstLine.isSpecified || firstLine.value == 0f) return false

    val density = result.layoutInput.density
    val firstLinePx = with(density) { firstLine.toPx() }
    return firstLinePx != 0f
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
