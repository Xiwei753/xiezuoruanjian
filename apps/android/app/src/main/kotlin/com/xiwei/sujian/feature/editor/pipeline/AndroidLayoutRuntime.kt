package com.xiwei.sujian.feature.editor.pipeline

import android.text.Layout
import android.text.TextPaint
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutEngine
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection

class AndroidLayoutRuntime(
    val mirror: DisplayTextMirror,
    val layoutEngine: AndroidLayoutEngine,
) {
    constructor(mirror: DisplayTextMirror, textPaint: TextPaint) : this(
        mirror,
        AndroidLayoutEngine(mirror, textPaint),
    )

    private var currentProjection: DisplayTextProjection = DisplayTextProjection.identity("")
    private var secretDisplayMode: Boolean = false

    fun requestLayout() {
        layoutEngine.requestLayout()
    }

    fun getLayout(): Layout? = layoutEngine.getLayout()

    fun getCurrentRevision(): AndroidLayoutRevision? = layoutEngine.getCurrentRevision()

    fun getWidth(): Float = layoutEngine.getWidth()

    fun setWidth(width: Float) {
        layoutEngine.setWidth(width)
    }

    fun setLineSpacingMultiplier(multiplier: Float) {
        layoutEngine.setLineSpacingMultiplier(multiplier)
    }

    fun applyProjection(projection: DisplayTextProjection) {
        currentProjection = projection
        if (projection.isMasked) {
            layoutEngine.setDisplayTextOverride(projection.displayText, projection)
        } else {
            layoutEngine.clearDisplayTextOverride(projection)
        }
        layoutEngine.requestLayout()
    }

    fun clearProjection() {
        currentProjection = DisplayTextProjection.identity(mirror.getText())
        layoutEngine.clearDisplayTextOverride(currentProjection)
        layoutEngine.requestLayout()
    }

    fun getCurrentProjection(): DisplayTextProjection = currentProjection

    fun setSecretDisplayMode(enabled: Boolean) {
        if (secretDisplayMode != enabled) {
            secretDisplayMode = enabled
        }
    }

    fun isSecretDisplayMode(): Boolean = secretDisplayMode

    fun rebuildDisplayProjection() {
        rebuildProjectionContent()
        layoutEngine.requestLayout()
    }

    fun applySecretDisplayIfActive() {
        rebuildDisplayProjection()
    }

    fun applySecretDisplayIfActiveWithLayout() {
        rebuildDisplayProjection()
    }

    private fun rebuildProjectionContent() {
        val text = mirror.getText()
        currentProjection =
            if (secretDisplayMode) {
                val compRange = mirror.getCompositionRangeUtf16()
                if (compRange != null && compRange.first >= 0 && compRange.second > compRange.first) {
                    val compText = mirror.getSpannable().substring(compRange.first, compRange.second)
                    DisplayTextProjection.maskedWithComposition(text, compRange.first, compRange.second, compText)
                } else {
                    DisplayTextProjection.masked(text)
                }
            } else {
                DisplayTextProjection.identity(text)
            }
        if (currentProjection.isMasked) {
            layoutEngine.setDisplayTextOverride(currentProjection.displayText, currentProjection)
        } else {
            layoutEngine.clearDisplayTextOverride(currentProjection)
        }
    }

    fun captureLineBitmapSnapshots(
        lineIndices: Set<Int>,
    ): Map<Int, com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot> =
        layoutEngine.captureLineBitmapSnapshots(lineIndices)

    fun captureLineBitmapSnapshotsWithClusters(
        lineIndices: Set<Int>,
    ): Map<Int, com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot> =
        layoutEngine.captureLineBitmapSnapshotsWithClusters(lineIndices)

    fun getLineForUtf8(byteOffset: Int): Int = layoutEngine.getLineForUtf8(byteOffset)

    fun getCursorLine(): Int = layoutEngine.getCursorLine()

    fun getPrimaryHorizontalUtf8(byteOffset: Int): Float = layoutEngine.getPrimaryHorizontalUtf8(byteOffset)
}
