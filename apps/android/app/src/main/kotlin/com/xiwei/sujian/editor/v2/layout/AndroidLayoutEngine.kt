package com.xiwei.sujian.editor.v2.layout

import android.text.DynamicLayout
import android.text.Layout
import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap

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
        if (width <= 0f) return

        layout = DynamicLayout.Builder.obtain(text, textPaint, width)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(0f, 1.0f)
            .setIncludePad(false)
            .build()

        revisionCounter++
        currentRevision = buildRevision()
    }

    fun getLayout(): Layout? = layout

    fun getCurrentRevision(): AndroidLayoutRevision? = currentRevision

    fun getWidth(): Float = width

    fun getLineForUtf8(byteOffset: Int): Int {
        val rev = currentRevision ?: return 0
        val indexMap = AndroidTextIndexMap(mirror)
        val utf16 = indexMap.utf8ToUtf16(byteOffset)
        val l = layout ?: return 0
        return l.getLineForOffset(utf16)
    }

    fun getCursorLine(): Int {
        return getLineForUtf8(mirror.getCursorUtf8())
    }

    fun getPrimaryHorizontalUtf8(byteOffset: Int): Float {
        val indexMap = AndroidTextIndexMap(mirror)
        val utf16 = indexMap.utf8ToUtf16(byteOffset)
        val l = layout ?: return 0f
        val line = l.getLineForOffset(utf16)
        return l.getPrimaryHorizontal(utf16, line)
    }

    private fun buildRevision(): AndroidLayoutRevision {
        val l = layout ?: return AndroidLayoutRevision(
            revisionCounter, mirror.getRevision(), width, "",
            0, emptyList(), mirror.getCursorUtf8(), mirror.getCursorUtf16(), emptyList()
        )

        val indexMap = AndroidTextIndexMap(mirror)
        val lineRanges = mutableListOf<AndroidLayoutRevision.LineRange>()
        for (i in 0 until l.lineCount) {
            val lineStartUtf16 = l.getLineStart(i)
            val lineEndUtf16 = l.getLineEnd(i)
            val top = l.getLineTop(i).toFloat()
            val bottom = l.getLineBottom(i).toFloat()
            val baseline = l.getLineBaseline(i).toFloat()
            val left = l.getLineLeft(i)
            val right = l.getLineRight(i)

            val startUtf8 = indexMap.utf16ToUtf8(lineStartUtf16)
            val endUtf8 = indexMap.utf16ToUtf8(lineEndUtf16)

            lineRanges.add(AndroidLayoutRevision.LineRange(
                startUtf8 = startUtf8,
                endUtf8 = endUtf8,
                startUtf16 = lineStartUtf16,
                endUtf16 = lineEndUtf16,
                top = top,
                bottom = bottom,
                baseline = baseline,
                left = left,
                right = right
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
