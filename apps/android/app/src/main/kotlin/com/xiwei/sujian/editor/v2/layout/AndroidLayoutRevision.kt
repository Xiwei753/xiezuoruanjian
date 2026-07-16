package com.xiwei.sujian.editor.v2.layout

data class AndroidLayoutRevision(
    val revisionId: Long,
    val editorRevision: Long,
    val widthFingerprint: Float,
    val fontFingerprint: String,
    val lineCount: Int,
    val lineRanges: List<LineRange>,
    val cursorUtf8: Int,
    val cursorUtf16: Int,
    val snapshotHandles: List<Long>
) {
    data class LineRange(
        val startUtf8: Int,
        val endUtf8: Int,
        val startUtf16: Int,
        val endUtf16: Int,
        val top: Float,
        val bottom: Float,
        val baseline: Float,
        val left: Float,
        val right: Float
    )
}
