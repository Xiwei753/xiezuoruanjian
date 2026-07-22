package com.xiwei.sujian.editor.v2.projection

import android.text.TextPaint
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult

class TargetReadonlyProjection(
    private val mirror: DisplayTextMirror,
    private val textPaint: TextPaint
) {
    private val layoutEngine: AndroidLayoutEngine = AndroidLayoutEngine(mirror, textPaint)
    private var projection: DisplayTextProjection = DisplayTextProjection.identity("")
    private var searchHighlightsUtf8: List<Pair<Int, Int>> = emptyList()
    private var selectionStartUtf8: Int = 0
    private var selectionEndUtf8: Int = 0

    fun updateFromSnapshot(text: String, cursorUtf8: Int, revision: Long) {
        mirror.loadFromSnapshot(text, cursorUtf8, revision)
        rebuildProjectionAndLayout()
    }

    fun applyEditResult(result: EditResult) {
        if (!result.isApplied()) return
        mirror.applyEditResult(result)
        rebuildProjectionContent()
        layoutEngine.requestLayout()
    }

    private fun rebuildProjectionContent() {
        val text = mirror.getText()
        projection = if (projection.isMasked) {
            DisplayTextProjection.masked(text)
        } else {
            DisplayTextProjection.identity(text)
        }
        if (projection.isMasked) {
            layoutEngine.setDisplayTextOverride(projection.displayText)
        } else {
            layoutEngine.clearDisplayTextOverride()
        }
    }

    private fun rebuildProjectionAndLayout() {
        rebuildProjectionContent()
        layoutEngine.requestLayout()
    }

    fun setSecretMasked(masked: Boolean) {
        val text = mirror.getText()
        projection = if (masked) {
            DisplayTextProjection.masked(text)
        } else {
            DisplayTextProjection.identity(text)
        }
        if (masked) {
            layoutEngine.setDisplayTextOverride(projection.displayText)
        } else {
            layoutEngine.clearDisplayTextOverride()
        }
        layoutEngine.requestLayout()
    }

    fun setSearchHighlights(highlights: List<Pair<Int, Int>>) {
        searchHighlightsUtf8 = highlights
    }

    fun setSelection(startUtf8: Int, endUtf8: Int) {
        selectionStartUtf8 = startUtf8
        selectionEndUtf8 = endUtf8
    }

    fun getSearchHighlightsUtf16(): List<Pair<Int, Int>> {
        return searchHighlightsUtf8.map { (start, end) ->
            Pair(projection.realUtf8ToDisplayUtf16(start), projection.realUtf8ToDisplayUtf16(end))
        }
    }

    fun getSelectionStartUtf16(): Int = projection.realUtf8ToDisplayUtf16(selectionStartUtf8)
    fun getSelectionEndUtf16(): Int = projection.realUtf8ToDisplayUtf16(selectionEndUtf8)

    fun getLayoutEngine(): AndroidLayoutEngine = layoutEngine
    fun getLayoutRevision(): AndroidLayoutRevision? = layoutEngine.getCurrentRevision()
    fun getText(): String = mirror.getText()
    fun getRevision(): Long = mirror.getRevision()
    fun getProjection(): DisplayTextProjection = projection

    fun setWidth(width: Float) {
        layoutEngine.setWidth(width)
        layoutEngine.requestLayout()
    }

    fun release() {
        layoutEngine.release()
    }
}
