package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange

/**
 * #641 评论 5457777142 问题2：视觉帧 — overlay 当前已经画到哪里的纯显示数据。
 *
 * 新事务到来时先用旧 transaction + 当前 progress 物化本帧，
 * 再把它作为新事务的 [ComposeVisualTransaction.startFrame]，
 * 避免 `progress=0` 生硬重开或直接覆盖旧事务。
 *
 * 这是纯显示数据：range、当前 translate、当前 alpha、当前 cursor rect。
 * 不持有 [androidx.compose.ui.text.TextLayoutResult] 引用，不写正文业务状态。
 */
data class ComposeVisualFrame(
    val slices: List<VisualFrameSlice>,
    val cursorRect: Rect? = null,
    val cursorAlpha: Float = 1f,
)

/**
 * #641 评论 5457777142 问题2：单个视觉帧 slice —
 * 一段受影响文字在某一时刻的显示状态。
 *
 * @param range 该 slice 对应的 UTF-16 range（old 或 new，由调用方决定）。
 * @param translate 当前 translate 偏移（用于 retained move 位移插值）。
 * @param alpha 当前 alpha（用于淡入/淡出插值）。
 */
data class VisualFrameSlice(
    val range: TextRange,
    val translate: Offset,
    val alpha: Float,
)
