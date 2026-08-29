package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #641 评论 5459754425 + 评论 5459896691 第2项：视觉帧 —
 * overlay 当前已经画到哪里的纯显示数据。所有 slice 扁平保存，
 * 每个 slice 携带自己的 [sourceLayout]，不再受"一帧只能有 old/new 两份 layout"限制。
 *
 * 新事务到来时先用旧 transaction + 当前 progress 物化本帧，
 * 再把它作为新事务的 [ComposeVisualTransaction.startFrame]。
 * 每次物化都把当前屏幕正在显示的所有内容 flatten 成一层新 frame，
 * 不再形成 startFrame → startFrame → startFrame 的链。
 *
 * @param slices 扁平的 rebase slice 列表 — 每个 slice 携带自己的 sourceLayout。
 * @param cursorRect 物化时的 cursor rect。
 * @param cursorAlpha 物化时的 cursor alpha。
 * @param suppressedCurrentRanges 物化时由 overlay 接管的 current text ranges。
 */
data class ComposeVisualFrame(
    val slices: List<RebasedTextSlice>,
    val cursorRect: Rect? = null,
    val cursorAlpha: Float = 1f,
    val suppressedCurrentRanges: List<TextRange> = emptyList(),
)

/**
 * #641 评论 5459754425 第1项 + 评论 5459896691 第2项：rebase 起点 + 目标模型 —
 * 每个 slice 携带自己的 [sourceLayout]，A/B/C 任意多次 rebase 都能扁平保存。
 *
 * 对仍存活的 slice（[targetRange] != null）：alpha = lerp(sourceAlpha, 1f, rebaseProgress)，
 *   position 从 source 位置插值到 target 位置。
 * 对只属于旧画面的 slice（[targetRange] == null）：alpha = lerp(sourceAlpha, 0f, rebaseProgress)。
 *
 * @param sourceLayout 该 slice 来自哪份 layout — 冻结时的 [ComposeLayoutSnapshot]。
 * @param sourceRange 该 slice 在 [sourceLayout] 中的 UTF-16 range。
 * @param sourceTranslate 当前 translate 偏移（相对 [sourceLayout] 原位置）。
 * @param sourceAlpha 当前 alpha。
 * @param targetRange 该 slice 在当前 new text 中的目标 range —
 *   null = 这段只属于旧画面，最终应消失；
 *   非 null = 仍存活，绘制时从 frozen 状态插值到当前最终 layout。
 */
data class RebasedTextSlice(
    val sourceLayout: ComposeLayoutSnapshot,
    val sourceRange: TextRange,
    val sourceTranslate: Offset,
    val sourceAlpha: Float,
    val targetRange: TextRange?,
)
