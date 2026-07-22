package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.projection.DisplayTextProjection
import android.text.TextPaint
import android.text.Layout

class AndroidLayoutRuntime(
    val mirror: DisplayTextMirror,
    val layoutEngine: AndroidLayoutEngine
) {
    constructor(mirror: DisplayTextMirror, textPaint: TextPaint) : this(
        mirror,
        AndroidLayoutEngine(mirror, textPaint)
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
            layoutEngine.setDisplayTextOverride(projection.displayText)
        } else {
            layoutEngine.clearDisplayTextOverride()
        }
        layoutEngine.requestLayout()
    }

    fun clearProjection() {
        currentProjection = DisplayTextProjection.identity(mirror.getText())
        layoutEngine.clearDisplayTextOverride()
        layoutEngine.requestLayout()
    }

    fun getCurrentProjection(): DisplayTextProjection = currentProjection

    fun setSecretDisplayMode(enabled: Boolean) {
        if (secretDisplayMode != enabled) {
            secretDisplayMode = enabled
            rebuildSecretProjection()
        }
    }

    fun isSecretDisplayMode(): Boolean = secretDisplayMode

    fun applySecretDisplayIfActive() {
        if (secretDisplayMode) {
            rebuildSecretProjectionContent()
        }
    }

    fun applySecretDisplayIfActiveWithLayout() {
        if (secretDisplayMode) {
            rebuildSecretProjection()
        }
    }

    private fun rebuildSecretProjectionContent() {
        val text = mirror.getText()
        currentProjection = if (secretDisplayMode) {
            DisplayTextProjection.masked(text)
        } else {
            DisplayTextProjection.identity(text)
        }
        if (currentProjection.isMasked) {
            layoutEngine.setDisplayTextOverride(currentProjection.displayText)
        } else {
            layoutEngine.clearDisplayTextOverride()
        }
    }

    private fun rebuildSecretProjection() {
        rebuildSecretProjectionContent()
        layoutEngine.requestLayout()
    }

    fun captureLineBitmapSnapshots(lineIndices: Set<Int>): Map<Int, com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot> =
        layoutEngine.captureLineBitmapSnapshots(lineIndices)

    fun captureLineBitmapSnapshotsWithClusters(lineIndices: Set<Int>): Map<Int, com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot> =
        layoutEngine.captureLineBitmapSnapshotsWithClusters(lineIndices)

    fun getLineForUtf8(byteOffset: Int): Int = layoutEngine.getLineForUtf8(byteOffset)
    fun getCursorLine(): Int = layoutEngine.getCursorLine()
    fun getPrimaryHorizontalUtf8(byteOffset: Int): Float = layoutEngine.getPrimaryHorizontalUtf8(byteOffset)
}
