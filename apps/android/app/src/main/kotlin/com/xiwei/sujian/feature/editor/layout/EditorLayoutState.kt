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
 * Issue #728 评论 5754045689：删除 U+200B 软断行投影后，[result] 就是 raw 正文布局
 * （不再含 U+200B），正文、selection、layout 全部是同一份 raw UTF-16 坐标。
 * 不再需要 projection / rawText / effectiveRawText — offset 直接查 [result]。
 *
 * @param result 系统 [BasicTextField] 的 `onTextLayout` 给出的最终布局结果（raw 正文）。
 * @param selection 当前选区（UTF-16 offset，raw 坐标）。
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
 *
 * Issue #728 评论 5754045689：result 就是 raw 正文布局，offset 直接查 [result]，
 * 不再经过 projection 映射。空段落首行缩进只在 draw 层对可见 caret rect 应用 x 偏移，
 * 正文布局和正文 offset 不改。
 */
fun ComposeLayoutSnapshot.cursorRect(offset: Int): Rect {
    val text = result.layoutInput.text.text
    val safeOffset = offset.coerceIn(0, text.length)
    return result.getCursorRect(safeOffset)
}

/**
 * #641 评论1 第4节：行信息访问 — 直接转发 [TextLayoutResult]，
 * 不缓存第二份行段。
 *
 * Issue #728：offset 直接查 [result]，不再经过 projection 映射。
 */
fun ComposeLayoutSnapshot.lineForOffset(offset: Int): Int {
    val textLength = result.layoutInput.text.text.length
    val safeOffset = offset.coerceIn(0, textLength)
    return result.getLineForOffset(safeOffset)
}

fun ComposeLayoutSnapshot.boundingBox(offset: Int): Rect {
    val textLength = result.layoutInput.text.text.length
    val safeOffset = offset.coerceIn(0, textLength)
    return result.getBoundingBox(safeOffset)
}

/**
 * raw TextRange → Path。range 来自正文/visual unit（raw 坐标），直接查 [result]。
 *
 * Issue #728：删除 projection 后 raw 与 display 一致，直接用 raw range 查 result。
 */
fun ComposeLayoutSnapshot.pathForRawRange(rawRange: TextRange): Path {
    val textLength = result.layoutInput.text.length
    return result.getPathForRange(
        rawRange.start.coerceIn(0, textLength),
        rawRange.end.coerceIn(0, textLength),
    )
}

/** raw TextRange → Rect?（path bounds）。range 来自正文/visual unit（raw 坐标）。 */
fun ComposeLayoutSnapshot.boundsForRawRange(rawRange: TextRange): Rect? {
    val textLength = result.layoutInput.text.length
    if (rawRange.start >= rawRange.end) return null
    if (rawRange.end > textLength) return null
    return try {
        result.getPathForRange(rawRange.start, rawRange.end).getBounds()
    } catch (_: Throwable) {
        null
    }
}

/** raw offset → line index。offset 来自正文/visual unit（raw 坐标）。 */
fun ComposeLayoutSnapshot.lineForRawOffset(rawOffset: Int): Int {
    val textLength = result.layoutInput.text.text.length
    val safeOffset = rawOffset.coerceIn(0, textLength)
    return result.getLineForOffset(safeOffset)
}

/**
 * raw line end for raw offset。
 *
 * Issue #728：result 就是 raw 布局，getLineEnd 直接返回 raw offset。
 */
fun ComposeLayoutSnapshot.rawLineEndForRawOffset(rawOffset: Int): Int {
    val line = lineForRawOffset(rawOffset)
    return result.getLineEnd(line)
}

/**
 * #644 评论 5462826712 第4节：编辑器视口状态 — 管理滚动/视口。
 *
 * 状态只保存：
 * - [scrollState]：Compose ScrollState
 * - [latestLayout]：最新的 TextLayoutResult（raw 正文布局）
 * - [pendingAnchor]：待恢复的视口锚点
 * - [restoredForCurrentAnchor]：当前锚点是否已恢复
 *
 * Issue #728 评论 5754045689：删除 projection 后，layout 就是 raw 坐标，
 * snapshotAnchor / restoreFromAnchor 直接用 raw offset 查 [TextLayoutResult]。
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
     * Issue #728：不再接收 projection 参数，layout 就是 raw 坐标。
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
     *
     * Issue #728：layout 就是 raw 坐标，lineStart 直接是 raw offset，存入 anchor。
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
     * Issue #728：anchor.textOffsetUtf16 是 raw 坐标，直接查 getLineForOffset。
     *
     * @return 需要 scrollTo 的 Y 值；null 表示 anchor 无效。
     */
    private fun restoreFromAnchor(
        layout: TextLayoutResult,
        anchor: ViewportAnchor,
    ): Int? {
        val textLength = layout.layoutInput.text.text.length
        val safeOffset = anchor.textOffsetUtf16.coerceIn(0, textLength)
        val line = layout.getLineForOffset(safeOffset)
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
