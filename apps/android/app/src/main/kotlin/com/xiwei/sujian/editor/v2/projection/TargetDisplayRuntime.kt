package com.xiwei.sujian.editor.v2.projection

import android.text.TextPaint
import android.view.Choreographer
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime
import com.xiwei.sujian.editor.v2.render.AndroidTextRenderer
import com.xiwei.sujian.editor.v2.render.AndroidTextAnimationRenderer
import com.xiwei.sujian.editor.v2.render.EditorFrameComposer
import com.xiwei.sujian.editor.v2.render.ComposedFrame
import com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime
import android.graphics.Canvas
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue

class TargetDisplayRuntime(
    private val mirror: DisplayTextMirror,
    private val textPaint: TextPaint
) {
    private val layoutEngine: AndroidLayoutEngine = AndroidLayoutEngine(mirror, textPaint)
    private val visualRuntime: AndroidVisualRuntime = AndroidVisualRuntime()
    private val renderRuntime: AndroidRenderRuntime = AndroidRenderRuntime()
    private var projection: DisplayTextProjection = DisplayTextProjection.identity("")
    private var secretDisplayMode: Boolean = false
    private var searchHighlightsUtf8: List<Pair<Int, Int>> = emptyList()
    private var selectionStartUtf8: Int = 0
    private var selectionEndUtf8: Int = 0
    private var scrollX: Float = 0f
    private var scrollY: Float = 0f
    private var viewportWidth: Int = 0
    private var viewportHeight: Int = 0

    var frameGeneration by mutableLongStateOf(0L)
        private set

    private val choreographer = Choreographer.getInstance()
    private var choreographerCallback: Choreographer.FrameCallback? = null
    private var isTicking: Boolean = false

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!isTicking) return
            if (hasActiveAnimation()) {
                frameGeneration++
                choreographer.postFrameCallback(this)
            } else {
                isTicking = false
            }
        }
    }

    fun startFrameClock() {
        if (isTicking) return
        isTicking = true
        choreographer.postFrameCallback(frameCallback)
    }

    fun stopFrameClock() {
        isTicking = false
        choreographer.removeFrameCallback(frameCallback)
    }

    fun updateFromSnapshot(text: String, cursorUtf8: Int, revision: Long) {
        mirror.loadFromSnapshot(text, cursorUtf8, revision)
        rebuildProjectionAndLayout()
    }

    fun applyEditResult(result: EditResult) {
        if (!result.isApplied()) return
        visualRuntime.prepareAndSubmit(
            visualIntent = result.visualIntent,
            layoutEngine = layoutEngine,
            mirrorUpdate = {
                mirror.applyEditResult(result)
                rebuildProjectionContent()
            }
        )
        startFrameClock()
    }

    private fun rebuildProjectionContent() {
        val text = mirror.getText()
        projection = if (secretDisplayMode) {
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
        if (projection.isMasked) {
            layoutEngine.setDisplayTextOverride(projection.displayText, projection)
        } else {
            layoutEngine.clearDisplayTextOverride(projection)
        }
    }

    private fun rebuildProjectionAndLayout() {
        rebuildProjectionContent()
        layoutEngine.requestLayout()
    }

    fun setSecretMasked(masked: Boolean) {
        secretDisplayMode = masked
        rebuildProjectionAndLayout()
    }

    fun setSearchHighlights(highlights: List<Pair<Int, Int>>) {
        searchHighlightsUtf8 = highlights
    }

    fun setSelection(startUtf8: Int, endUtf8: Int) {
        selectionStartUtf8 = startUtf8
        selectionEndUtf8 = endUtf8
    }

    fun clearDecorations() {
        searchHighlightsUtf8 = emptyList()
        selectionStartUtf8 = 0
        selectionEndUtf8 = 0
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

    fun setScrollPosition(sx: Float, sy: Float) {
        scrollX = sx
        scrollY = sy
    }

    fun getScrollX(): Float = scrollX
    fun getScrollY(): Float = scrollY

    fun setViewportSize(w: Int, h: Int) {
        viewportWidth = w
        viewportHeight = h
    }

    fun getViewportWidth(): Int = viewportWidth
    fun getViewportHeight(): Int = viewportHeight

    fun setLineSpacingMultiplier(multiplier: Float) {
        layoutEngine.setLineSpacingMultiplier(multiplier)
        layoutEngine.requestLayout()
    }

    fun setFontSize(sizePx: Float) {
        textPaint.textSize = sizePx
        if (projection.isMasked) {
            layoutEngine.setDisplayTextOverride(projection.displayText, projection)
        } else {
            layoutEngine.clearDisplayTextOverride(projection)
        }
        layoutEngine.requestLayout()
    }

    fun setThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int) {
        renderRuntime.setThemeColors(textColor, cursorColor, selectionColor, preeditColor, bgColor)
    }

    fun drawFrame(canvas: Canvas) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val layout = layoutEngine.getLayout() ?: return
        val highlightsUtf16 = getSearchHighlightsUtf16()
        val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
        val selStartDisplayUtf16 = projection.realUtf8ToDisplayUtf16(selectionStartUtf8)
        val selEndDisplayUtf16 = projection.realUtf8ToDisplayUtf16(selectionEndUtf8)
        val frameState = visualRuntime.tick(
            frameTimeMs,
            layout,
            layoutEngine.getCurrentRevision(),
            highlightsUtf16,
            viewportWidth, viewportHeight,
            scrollX, scrollY,
            true, true,
            cursorDisplayUtf16,
            selStartDisplayUtf16,
            selEndDisplayUtf16
        )
        if (frameState != null) {
            renderRuntime.drawFromFrameState(canvas, frameState)
        }
    }

    fun hasActiveAnimation(): Boolean = visualRuntime.hasActiveAnimation()

    fun release() {
        stopFrameClock()
        visualRuntime.release()
        layoutEngine.release()
    }
}
