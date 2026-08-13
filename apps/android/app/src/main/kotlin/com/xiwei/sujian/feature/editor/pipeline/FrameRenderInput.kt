package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.layout.LayoutRevisionSource
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction

data class FrameRenderInput(
    val layout: android.text.Layout,
    val layoutRevision: LayoutRevisionSource?,
    /** 文字动画事务 — 文字轨结束时为 null（静态新布局直接绘制），但 [cursorTransition] 仍可携带。 */
    val transaction: PreparedVisualTransaction?,
    /** #595 五：光标过渡几何 — 文字轨结束/抑制但光标轨未结束时仍非 null，
     *  静态文字路径据此绘制平滑光标；光标轨结束为 null。 */
    val cursorTransition: PreparedVisualTransaction.CursorTransition?,
    val timelineProgress: Float,
    val cursorProgress: Float?,
    val searchHighlightsUtf16: List<Pair<Int, Int>>,
    val viewportWidth: Int,
    val viewportHeight: Int,
    val scrollX: Float,
    val scrollY: Float,
    val cursorVisible: Boolean,
    val selectionAllowed: Boolean,
    val cursorUtf16: Int,
    val selectionStartUtf16: Int,
    val selectionEndUtf16: Int,
)

class FrameState(
    val renderInput: FrameRenderInput,
    val displayStateVersion: Long = 0L,
    val completeAfterDraw: Boolean = false,
)
