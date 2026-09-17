package com.xiwei.sujian.feature.editor.layout

import androidx.compose.foundation.ScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Rect
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
 */
data class ComposeLayoutSnapshot(
    val result: TextLayoutResult,
    val selection: TextRange,
    val scrollY: Int,
)

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
    val raw = result.getCursorRect(safeOffset)

    // 只对"逻辑段落开头且该段落当前为空"的 caret 修正首行缩进。
    // atParagraphStart：offset 在段落首字符处（文档开头或前一个字符是 \n）。
    // emptyParagraph：offset 所在段落为空（文档末尾或当前字符是 \n）。
    val atParagraphStart = safeOffset == 0 || text[safeOffset - 1] == '\n'
    val emptyParagraph = safeOffset == text.length || text[safeOffset] == '\n'
    if (!atParagraphStart || !emptyParagraph) return raw

    val textIndent = result.layoutInput.style.textIndent ?: return raw
    val firstLine = textIndent.firstLine
    if (!firstLine.isSpecified || firstLine.value == 0f) return raw

    // 把首行缩进 sp 换算成 px，按段落方向加到行起始边。
    val density = result.layoutInput.density
    val firstLinePx = with(density) { firstLine.toPx() }
    if (firstLinePx == 0f) return raw

    val line = result.getLineForOffset(safeOffset)
    val direction = result.getParagraphDirection(safeOffset)
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

    val atParagraphStart = safeOffset == 0 || text[safeOffset - 1] == '\n'
    val emptyParagraph = safeOffset == text.length || text[safeOffset] == '\n'
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
 */
fun ComposeLayoutSnapshot.lineForOffset(offset: Int): Int = result.getLineForOffset(offset)

fun ComposeLayoutSnapshot.boundingBox(offset: Int): Rect = result.getBoundingBox(offset)

/**
 * #644 评论 5462826712 第4节：编辑器视口状态 — 管理滚动/视口。
 *
 * 状态只保存：
 * - [scrollState]：Compose ScrollState
 * - [latestLayout]：最新的 TextLayoutResult
 * - [pendingAnchor]：待恢复的视口锚点
 * - [restoredForCurrentAnchor]：当前锚点是否已恢复
 */
class EditorViewportState(
    val scrollState: ScrollState,
    initialAnchor: ViewportAnchor?,
) {
    private var latestLayout: TextLayoutResult? = null
    private var pendingAnchor: ViewportAnchor? = initialAnchor
    private var restoredForCurrentAnchor: Boolean = false

    /**
     * #644 评论 5462826712 第4节：系统给出权威布局时调用。
     * 有 pending anchor 时只恢复一次。
     *
     * @return 需要 scrollTo 的 Y 值；null 表示无需恢复。调用方用 coroutine scope 调 scrollTo。
     */
    fun onLayout(result: TextLayoutResult): Int? {
        latestLayout = result
        val anchor = pendingAnchor ?: return null
        if (restoredForCurrentAnchor) return null
        restoredForCurrentAnchor = true
        return restoreFromAnchor(result, anchor)
    }

    /**
     * #644 评论 5462826712 第4节：用当前 scrollState + TextLayoutResult 算逻辑锚点。
     */
    fun snapshotAnchor(): ViewportAnchor? {
        val layout = latestLayout ?: return null
        val scrollY = scrollState.value
        val line = layout.getLineForVerticalPosition(scrollY.toFloat())
        val textOffsetUtf16 = layout.getLineStart(line)
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
     * @return 需要 scrollTo 的 Y 值；null 表示 anchor 无效。
     */
    private fun restoreFromAnchor(
        layout: TextLayoutResult,
        anchor: ViewportAnchor,
    ): Int? {
        val line = layout.getLineForOffset(anchor.textOffsetUtf16)
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
