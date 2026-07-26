package com.xiwei.sujian.editor.v2.projection

import android.text.TextPaint
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime
import com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime
import com.xiwei.sujian.editor.v2.pipeline.FrameState
import android.graphics.Canvas
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue

class TargetDisplayRuntime(
    private val mirror: DisplayTextMirror,
    private val textPaint: TextPaint,
    private val timeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
    private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource(),
) : WindowDisplayFrameClock.FrameListener {
    private val layoutEngine: AndroidLayoutEngine = AndroidLayoutEngine(mirror, textPaint)
    private val visualRuntime: AndroidVisualRuntime = AndroidVisualRuntime(timeSource, transactionIdSource)
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

    var displayStateVersion by mutableLongStateOf(0L)
        private set

    private var cachedFrameState: FrameState? = null

    private var frameClock: WindowDisplayFrameClock? = null
    private var isRegisteredWithClock: Boolean = false

    override fun needsFrame(): Boolean = hasActiveAnimation() || cachedFrameState == null

    override fun onFrame(frameTimeNanos: Long) {
        val versionAtFrameStart = displayStateVersion
        val frameTimeMs = visualRuntime.currentTimeNanos() / 1_000_000
        val layout = layoutEngine.getLayout()
        if (layout != null) {
            val highlightsUtf16 = getSearchHighlightsUtf16()
            val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
            val selStartDisplayUtf16 = projection.realUtf8ToDisplayUtf16(selectionStartUtf8)
            val selEndDisplayUtf16 = projection.realUtf8ToDisplayUtf16(selectionEndUtf8)
            val tickResult = visualRuntime.tick(
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
            if (tickResult != null) {
                cachedFrameState = FrameState(tickResult.renderInput, versionAtFrameStart)
            }
        }
        frameGeneration++
    }

    fun setFrameClock(clock: WindowDisplayFrameClock?) {
        val oldClock = frameClock
        if (oldClock != null && isRegisteredWithClock) {
            oldClock.removeListener(this)
            isRegisteredWithClock = false
        }
        frameClock = clock
        if (clock != null && (hasActiveAnimation() || cachedFrameState == null)) {
            clock.addListener(this)
            isRegisteredWithClock = true
            clock.requestFrame()
        }
    }

    private fun ensureRegisteredWithClock() {
        val clock = frameClock ?: return
        if (!isRegisteredWithClock) {
            clock.addListener(this)
            isRegisteredWithClock = true
        }
        clock.requestFrame()
    }

    fun invalidateDisplayState() {
        cachedFrameState = null
        displayStateVersion++
        frameGeneration++
        ensureRegisteredWithClock()
    }

    fun updateFromSnapshot(text: String, cursorUtf8: Int, revision: Long) {
        mirror.loadFromSnapshot(text, cursorUtf8, revision)
        rebuildProjectionContent()
        layoutEngine.requestLayout()
        invalidateDisplayState()
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
        invalidateDisplayState()
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

    fun setSecretMasked(masked: Boolean) {
        secretDisplayMode = masked
        rebuildProjectionContent()
        layoutEngine.requestLayout()
        invalidateDisplayState()
    }

    fun setSearchHighlights(highlights: List<Pair<Int, Int>>) {
        searchHighlightsUtf8 = highlights
        invalidateDisplayState()
    }

    fun setSelection(startUtf8: Int, endUtf8: Int) {
        selectionStartUtf8 = startUtf8
        selectionEndUtf8 = endUtf8
        invalidateDisplayState()
    }

    fun clearDecorations() {
        searchHighlightsUtf8 = emptyList()
        selectionStartUtf8 = 0
        selectionEndUtf8 = 0
        invalidateDisplayState()
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
        invalidateDisplayState()
    }

    fun setScrollPosition(sx: Float, sy: Float) {
        scrollX = sx
        scrollY = sy
        invalidateDisplayState()
    }

    fun getScrollX(): Float = scrollX
    fun getScrollY(): Float = scrollY

    fun setViewportSize(w: Int, h: Int) {
        viewportWidth = w
        viewportHeight = h
        invalidateDisplayState()
    }

    fun getViewportWidth(): Int = viewportWidth
    fun getViewportHeight(): Int = viewportHeight

    fun setLineSpacingMultiplier(multiplier: Float) {
        layoutEngine.setLineSpacingMultiplier(multiplier)
        layoutEngine.requestLayout()
        invalidateDisplayState()
    }

    fun setFontSize(sizePx: Float) {
        textPaint.textSize = sizePx
        if (projection.isMasked) {
            layoutEngine.setDisplayTextOverride(projection.displayText, projection)
        } else {
            layoutEngine.clearDisplayTextOverride(projection)
        }
        layoutEngine.requestLayout()
        invalidateDisplayState()
    }

    fun setThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int, searchHighlightColor: Int = 0) {
        renderRuntime.setThemeColors(textColor, cursorColor, selectionColor, preeditColor, bgColor, searchHighlightColor)
        invalidateDisplayState()
    }

    fun drawFrame(canvas: Canvas) {
        val cached = cachedFrameState
        if (cached != null && cached.displayStateVersion == displayStateVersion) {
            renderRuntime.drawFromFrameState(canvas, cached)
            return
        }
        val frameTimeMs = timeSource.nowNanos() / 1_000_000
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
            val versionedFrameState = FrameState(frameState.renderInput, displayStateVersion)
            cachedFrameState = versionedFrameState
            renderRuntime.drawFromFrameState(canvas, versionedFrameState)
        }
    }

    fun hasActiveAnimation(): Boolean = visualRuntime.hasActiveAnimation()

    fun release() {
        val clock = frameClock
        if (clock != null && isRegisteredWithClock) {
            clock.removeListener(this)
            isRegisteredWithClock = false
        }
        frameClock = null
        visualRuntime.release()
        layoutEngine.release()
    }
}
