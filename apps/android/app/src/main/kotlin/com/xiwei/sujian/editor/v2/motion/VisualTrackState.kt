package com.xiwei.sujian.editor.v2.motion

import androidx.compose.runtime.Immutable
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction

/**
 * #595 五：视觉事务文字轨和光标轨的明确终态。
 *
 * 完成规则：
 * ```text
 * textFinished = text timeline 不存在或已结束
 * cursorFinished = cursor timeline 不存在或已结束
 * transactionFinished = textFinished && cursorFinished
 * ```
 *
 * 渲染规则：
 * - 文字未结束 + 光标未结束 → animated text + animated cursor
 * - 文字已结束 + 光标未结束 → static new-layout text + animated cursor
 * - 文字未结束 + 光标已结束 → animated text + cursor 固定在终点
 * - 两者都结束 → 释放事务
 */
@Immutable
data class VisualTrackState(
    val renderTextTransaction: PreparedVisualTransaction?,
    val renderCursorTransition: Boolean,
    val textProgress: Float?,
    val cursorProgress: Float?,
    val textFinished: Boolean,
    val cursorFinished: Boolean,
    val transactionComplete: Boolean,
) {
    companion object {
        val Idle = VisualTrackState(
            renderTextTransaction = null,
            renderCursorTransition = false,
            textProgress = null,
            cursorProgress = null,
            textFinished = true,
            cursorFinished = true,
            transactionComplete = true,
        )
    }
}
