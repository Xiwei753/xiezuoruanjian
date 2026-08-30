package com.xiwei.sujian.feature.editor.layout

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.session.ViewportAnchor

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
 */
fun ComposeLayoutSnapshot.cursorRect(): Rect = result.getCursorRect(selection.end)

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
     */
    fun onLayout(result: TextLayoutResult) {
        latestLayout = result
        val anchor = pendingAnchor ?: return
        if (restoredForCurrentAnchor) return
        restoredForCurrentAnchor = true
        restoreFromAnchor(result, anchor)
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
        val fraction = if (lineBottom > lineTop) {
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
     */
    private fun restoreFromAnchor(layout: TextLayoutResult, anchor: ViewportAnchor) {
        val line = layout.getLineForOffset(anchor.textOffsetUtf16)
        val lineTop = layout.getLineTop(line)
        val lineBottom = layout.getLineBottom(line)
        val lineHeight = lineBottom - lineTop
        val y = (lineTop + lineHeight * anchor.offsetWithinLineFraction).toInt()
        val clampedY = y.coerceIn(0, scrollState.maxValue)
        scrollState.scrollTo(clampedY)
    }
}

/**
 * #644 评论 5462826712 第4节：remember EditorViewportState —
 * 内部让 ScrollState 也跟 targetId 一起新建，不能 target 换了只换 wrapper。
 */
@Composable
fun rememberEditorViewportState(
    targetId: String,
    initialAnchor: ViewportAnchor?,
): EditorViewportState {
    val scrollState = rememberScrollState()
    return remember(targetId) {
        EditorViewportState(
            scrollState = scrollState,
            initialAnchor = initialAnchor,
        )
    }
}
