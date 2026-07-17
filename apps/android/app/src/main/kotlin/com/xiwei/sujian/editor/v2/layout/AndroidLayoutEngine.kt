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
    private var layout: DynamicLayout? = null
    private var currentRevision: AndroidLayoutRevision? = null
    private var width: Float = 0f
    private var lineSpacingMultiplier: Float = 1.0f
    private var revisionCounter: Long = 0
    private var lastConfigFingerprint: String = ""
    private var lastEditorRevision: Long = -1

    private fun computeConfigFingerprint(): String {
        return "${width}_${textPaint.textSize}_${textPaint.typeface?.hashCode() ?: 0}_${lineSpacingMultiplier}"
    }

    fun setWidth(width: Float) {
        if (this.width != width) {
            this.width = width
        }
    }

    fun setLineSpacingMultiplier(multiplier: Float) {
        lineSpacingMultiplier = multiplier
    }

    fun requestLayout() {
        val text = mirror.getSpannable()
        if (width <= 0f) return

        val currentConfigFp = computeConfigFingerprint()
        val currentEditorRevision = mirror.getRevision()
        val existingLayout = layout

        if (existingLayout != null && currentConfigFp == lastConfigFingerprint) {
            revisionCounter++
            currentRevision = buildRevision(existingLayout)
            lastEditorRevision = currentEditorRevision
            return
        }

        layout = DynamicLayout.Builder.obtain(text, textPaint, width.toInt())
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(0f, lineSpacingMultiplier)
            .setIncludePad(false)
            .build()

        lastConfigFingerprint = currentConfigFp
        lastEditorRevision = currentEditorRevision
        revisionCounter++
        currentRevision = buildRevision(layout!!)
    }

    fun getLayout(): Layout? = layout

    fun getCurrentRevision(): AndroidLayoutRevision? = currentRevision

    fun getWidth(): Float = width

    fun getMirror(): DisplayTextMirror = mirror

    fun getLineForUtf8(byteOffset: Int): Int {
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
        return l.getPrimaryHorizontal(utf16)
    }

    private fun buildRevision(l: Layout): AndroidLayoutRevision {
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

        val fontFingerprint = "${textPaint.textSize}_${textPaint.typeface?.hashCode() ?: 0}"

        return AndroidLayoutRevision(
            revisionCounter,
            mirror.getRevision(),
            width,
            fontFingerprint,
            l.lineCount,
            lineRanges.toList(),
            mirror.getCursorUtf8(),
            mirror.getCursorUtf16(),
            emptyList()
        )
    }
}
