package com.xiwei.sujian.editor.v2.layout

import android.text.StaticLayout
import android.text.Layout
import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidLayoutRevision(
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
    class LineRange(
        val startUtf8: Int,
        val endUtf8: Int,
        val startUtf16: Int,
        val endUtf16: Int,
        val top: Float,
        val bottom: Float,
        val baseline: Float
    )
}

class AndroidLayoutEngine(
    private val mirror: DisplayTextMirror,
    private val textPaint: TextPaint
) {
    private var layout: Layout? = null
    private var currentRevision: AndroidLayoutRevision? = null
    private var width: Float = 0f
    private var revisionCounter: Long = 0

    fun setWidth(width: Float) {
        if (this.width != width) {
            this.width = width
            requestLayout()
        }
    }

    fun requestLayout() {
        val text = mirror.getSpannable()
        layout = StaticLayout.Builder.obtain(text, 0, text.length, textPaint, width.toInt())
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(0f, 1.0f)
            .setIncludePad(false)
            .build()

        revisionCounter++
        currentRevision = buildRevision()
    }

    fun getLayout(): Layout? = layout

    fun getCurrentRevision(): AndroidLayoutRevision? = currentRevision

    private fun buildRevision(): AndroidLayoutRevision {
        val l = layout ?: return AndroidLayoutRevision(
            revisionCounter, mirror.getRevision(), width, "",
            0, emptyList(), mirror.getCursorUtf8(), mirror.getCursorUtf16(), emptyList()
        )

        val lineRanges = mutableListOf<AndroidLayoutRevision.LineRange>()
        for (i in 0 until l.lineCount) {
            val lineStart = l.getLineStart(i)
            val lineEnd = l.getLineEnd(i)
            val top = l.getLineTop(i).toFloat()
            val bottom = l.getLineBottom(i).toFloat()
            val baseline = l.getLineBaseline(i).toFloat()
            lineRanges.add(AndroidLayoutRevision.LineRange(
                0, 0, lineStart, lineEnd, top, bottom, baseline
            ))
        }

        return AndroidLayoutRevision(
            revisionCounter,
            mirror.getRevision(),
            width,
            textPaint.textSize.toString(),
            l.lineCount,
            lineRanges,
            mirror.getCursorUtf8(),
            mirror.getCursorUtf16(),
            emptyList()
        )
    }
}
