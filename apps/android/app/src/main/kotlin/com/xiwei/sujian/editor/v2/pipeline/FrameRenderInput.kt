package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

data class FrameRenderInput(
    val layout: android.text.Layout,
    val layoutRevision: AndroidLayoutRevision?,
    val transaction: PreparedVisualTransaction?,
    val timelineProgress: Float,
    val searchHighlightsUtf16: List<Pair<Int, Int>>,
    val viewportWidth: Int,
    val viewportHeight: Int,
    val scrollX: Float,
    val scrollY: Float,
    val cursorVisible: Boolean,
    val selectionAllowed: Boolean,
    val mirror: DisplayTextMirror
)

class FrameState(
    val renderInput: FrameRenderInput
)
