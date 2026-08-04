package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction

data class FrameRenderInput(
    val layout: android.text.Layout,
    val layoutRevision: AndroidLayoutRevision?,
    val transaction: PreparedVisualTransaction?,
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
    val selectionEndUtf16: Int
)

class FrameState(
    val renderInput: FrameRenderInput,
    val displayStateVersion: Long = 0L,
    val completeAfterDraw: Boolean = false
)
