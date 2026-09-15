package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #684 评论 5672654866：光标运动路径 —
 * 把"光标路径"从单纯 start/end 两点升级成和文字 unit 对应的路径。
 *
 * 路径描述"这一笔应该经过哪里"，但不描述"现在屏幕光标在哪里"。
 * 当前屏幕位置由 overlay 内长生命周期的 [androidx.compose.animation.core.Animatable] 自己持有。
 *
 * - 单字符输入/删除：就是一个目标点。
 * - 一次提交多个插入 unit：按 [newAnimationUnits] 顺序，每个字/cluster 出现后光标应到的位置组成路径，
 *   [CursorMotionPoint.endFraction] 与 timeline 的 unit-wise 分段时序一致。
 * - 多 Core intent 合成一个屏幕事务：优先用每笔 intent 的 cursor old/new offset，
 *   经 offset-map chain 映射到最终布局后组成路径；不能映射的中间点不猜，
 *   最终点仍取最后一笔真实 cursor。
 * - 删除选择区这种没有真实中间 layout 的事务：不伪造中间位置，只保留最终目标。
 *
 * @param rect 该路径点对应的光标矩形（从 [androidx.compose.ui.text.TextLayoutResult.getCursorRect] 取）。
 * @param endFraction 该路径点在整条动画 timeline 上到达的归一化时间（0..1）。
 *   与 timeline 的 unit-wise 分段时序一致：
 *   N 个 unit，unit i 的 endFraction = (i + 1f) / N。
 */
data class CursorMotionPoint(
    val rect: Rect,
    val endFraction: Float,
)

/**
 * 光标运动路径 — 一笔视觉事务内光标应依次经过的点序列。
 *
 * - 单点路径（单字符输入/删除、cursor-only 事务）：只含一个 [CursorMotionPoint]，
 *   endFraction = 1f。
 * - 多点路径（一次提交多个插入 unit）：按 unit 顺序排列，
 *   最后一个点的 endFraction = 1f。
 * - 空路径（无光标动画）：用 null 表示，不构造本类。
 */
data class CursorMotionPath(
    val points: List<CursorMotionPoint>,
)

/**
 * #684 评论 5672654866：构建光标运动路径 — 纯函数。
 *
 * 规则：
 * 1. 单字符输入/删除：就是一个目标点（endFraction = 1f）。
 * 2. 一次提交多个插入 unit：按 [newAnimationUnits] 顺序，用
 *    `newLayout.result.getCursorRect(unit.end)` 得到每个字/cluster 出现后光标应该到的位置；
 *    `endFraction = (index + 1f) / unitCount`，和 timeline 的 unit-wise 分段时序一致。
 * 3. 多 Core intent 合成一个屏幕事务时，优先使用每笔 intent 的 cursor old/new offset，
 *    经现有 offset-map chain 映射到最终布局后组成路径；不能映射的中间点不要猜，
 *    最终点仍取最后一笔真实 cursor。
 * 4. 删除选择区这种没有真实中间 layout 的事务，不伪造中间位置，只保留最终目标。
 *
 * @param oldLayout 旧布局快照（T0）。
 * @param newLayout 新布局快照（Tn）。
 * @param intents 本事务合并的所有 Core intent（按到达顺序）。
 * @param oldAnimationUnits 旧动画单元（删除/移动用）。
 * @param newAnimationUnits 新动画单元（插入/移动用）。
 * @return 光标运动路径；无光标动画语义时返回 null。
 */
@Suppress("CyclomaticComplexity", "CognitiveComplexMethod")
fun buildCursorMotionPath(
    oldLayout: ComposeLayoutSnapshot?,
    newLayout: ComposeLayoutSnapshot?,
    intents: List<EditorVisualIntent>,
    oldAnimationUnits: List<TextRange>,
    newAnimationUnits: List<TextRange>,
): CursorMotionPath? {
    if (newLayout == null) return null
    // 没有任何 cursor intent → 无光标动画。
    val cursors = intents.mapNotNull { it.cursor }
    if (cursors.isEmpty()) return null
    // 光标位置未变 → 无光标动画。
    val firstCursor = cursors.first()
    val lastCursor = cursors.last()
    if (firstCursor.oldEndUtf16 == lastCursor.newEndUtf16) return null

    val newTextLen = newLayout.result.layoutInput.text.length

    // 一次提交多个插入 unit：按 newAnimationUnits 顺序取每个 unit 出现后的 caret rect。
    // 仅当本事务是纯插入（oldAnimationUnits 为空）且有多个 newAnimationUnits 时走多段路径，
    // 这样光标依次经过每个 unit 的 caret rect，与正文逐 unit 吐字时序一致。
    // Delete/Move 不伪造中间位置（没有真实中间 layout），只保留最终目标。
    if (oldAnimationUnits.isEmpty() && newAnimationUnits.size > 1) {
        val points = mutableListOf<CursorMotionPoint>()
        val n = newAnimationUnits.size
        for ((i, unit) in newAnimationUnits.withIndex()) {
            val offset = unit.end.coerceIn(0, newTextLen)
            val rect = safeCursorRect(newLayout, offset) ?: continue
            val endFraction = (i + 1f) / n
            points.add(CursorMotionPoint(rect = rect, endFraction = endFraction))
        }
        if (points.isEmpty()) return null
        // 保证最后一个点 endFraction = 1f。
        return CursorMotionPath(points = normalizeEndFractions(points))
    }

    // 多 Core intent：遍历每笔 intent 的 cursor，映射中间点到最终布局。
    // 仅当本事务不是纯插入多 unit（上面已处理）且有多个 intent 时走此路径。
    if (intents.size > 1) {
        val points = mutableListOf<CursorMotionPoint>()
        // 收集有 cursor 的 intents 及其在 chain 中的原始索引
        val cursorIntents =
            intents.mapIndexedNotNull { idx, intent ->
                intent.cursor?.let { idx to it }
            }
        val n = cursorIntents.size
        for ((i, pair) in cursorIntents.withIndex()) {
            val (intentIdx, cursor) = pair
            if (i == n - 1) {
                // 最后一笔：无条件作为终点，用 newLayout 查真实 cursor rect
                val endOffset = cursor.newEndUtf16.coerceIn(0, newTextLen)
                val endRect = safeCursorRect(newLayout, endOffset)
                if (endRect != null) {
                    points.add(CursorMotionPoint(rect = endRect, endFraction = 1f))
                }
                continue
            }
            // 中间笔：把 cursor.newEndUtf16 沿后续 offset maps 映射到 Tn 坐标
            val mappedOffset = ComposeVisualRebase.mapCursorOffsetThroughChain(intents, intentIdx, cursor.newEndUtf16)
            if (mappedOffset != null) {
                val mappedOffsetCoerced = mappedOffset.coerceIn(0, newTextLen)
                val rect = safeCursorRect(newLayout, mappedOffsetCoerced)
                if (rect != null) {
                    val endFraction = (i + 1f) / n
                    points.add(CursorMotionPoint(rect = rect, endFraction = endFraction))
                }
            }
            // 不能映射的中间点不猜，跳过
        }
        if (points.isNotEmpty()) {
            return CursorMotionPath(points = normalizeEndFractions(points))
        }
        // 所有中间点都不能映射时，回退到只保留最终目标点
    }

    // 单字符 / 单 unit / Delete / Move / cursor-only：只保留最终目标点。
    // 最终光标位置取最后一笔 intent 的 newEndUtf16，查 newLayout。
    val endOffset = lastCursor.newEndUtf16.coerceIn(0, newTextLen)
    val endRect = safeCursorRect(newLayout, endOffset) ?: return null
    return CursorMotionPath(
        points = listOf(CursorMotionPoint(rect = endRect, endFraction = 1f)),
    )
}

/**
 * 安全取 cursor rect — offset 越界或 layout 抛异常时返回 null。
 */
private fun safeCursorRect(
    layout: ComposeLayoutSnapshot,
    offset: Int,
): Rect? =
    try {
        layout.result.getCursorRect(offset)
    } catch (_: Throwable) {
        null
    }

/**
 * 规整 endFraction — 保证非递减且最后一个点为 1f。
 * 避免因 unit.end 重复或跳过导致 endFraction 不单调。
 */
private fun normalizeEndFractions(points: List<CursorMotionPoint>): List<CursorMotionPoint> {
    if (points.isEmpty()) return points
    val result = mutableListOf<CursorMotionPoint>()
    var lastFraction = 0f
    for (point in points) {
        val fraction = point.endFraction.coerceIn(lastFraction, 1f)
        result.add(point.copy(endFraction = fraction))
        lastFraction = fraction
    }
    if (result.isEmpty()) return result
    // 强制最后一个点 endFraction = 1f。
    val last = result.removeAt(result.size - 1)
    result.add(last.copy(endFraction = 1f))
    return result
}
