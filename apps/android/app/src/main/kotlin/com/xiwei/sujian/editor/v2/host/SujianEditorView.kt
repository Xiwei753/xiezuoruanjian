package com.xiwei.sujian.editor.v2.host

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.text.TextPaint
import android.util.AttributeSet
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline
import uniffi.writer_core.EditorTransactionCauseDto

class SujianEditorView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private val textPaint = TextPaint().apply {
        textSize = 48f
        isAntiAlias = true
    }
    private val pipeline = AndroidEditorPipeline.create(
        com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror(),
        textPaint,
        this
    )

    private var scrollX: Float = 0f
    private var scrollY: Float = 0f
    private var maxScrollY: Float = 0f
    private var searchHighlights: List<Pair<Int, Int>> = emptyList()
    private var pendingLayoutNeeded: Boolean = false

    var kernelBridge: EditorKernelBridge?
        get() = pipeline.kernelBridge
        set(value) { pipeline.kernelBridge = value }

    private var themeBackgroundColor: Int = Color.WHITE

    init {
        pipeline.inputAdapter?.onPipelineOutput = { output -> handlePipelineOutput(output) }
        pipeline.inputAdapter?.onCompositionVisualUpdate = {
            updateMaxScroll()
            scrollY = scrollY.coerceIn(0f, maxScrollY)
            invalidate()
        }
    }

    fun loadText(text: String, cursorUtf8: Int) {
        val result = pipeline.loadText(text, cursorUtf8)
        if (result is AndroidEditorPipeline.LoadTextResult.Loaded) {
            if (width > 0) {
                updateLayoutConfig()
            } else {
                pendingLayoutNeeded = true
            }
        }
    }

    fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING) {
        val output = pipeline.insertText(byteOffset, text, cause)
        handlePipelineOutput(output)
    }

    fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.DELETE) {
        val output = pipeline.deleteRange(byteStart, byteEndExclusive, cause)
        handlePipelineOutput(output)
    }

    fun replaceRangeTyped(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING, beforePatch: (() -> Unit)? = null) {
        val output = pipeline.replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause, beforePatch)
        handlePipelineOutput(output)
    }

    fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int) {
        val output = pipeline.setSelectionTyped(anchorByteOffset, headByteOffset)
        handlePipelineOutput(output)
    }

    fun performUndo() {
        val output = pipeline.performUndo()
        handlePipelineOutput(output)
    }

    fun performRedo() {
        val output = pipeline.performRedo()
        handlePipelineOutput(output)
    }

    private fun handlePipelineOutput(output: AndroidEditorPipeline.PipelineOutput, suppressContentCallback: Boolean = false) {
        when (output) {
            is AndroidEditorPipeline.PipelineOutput.Edited -> {
                updateMaxScroll()
                scrollY = scrollY.coerceIn(0f, maxScrollY)
                if (!suppressContentCallback && output.result.displayPatches.isNotEmpty()) {
                    onContentChanged?.invoke(pipeline.getText())
                }
                invalidate()
            }
            is AndroidEditorPipeline.PipelineOutput.NeedReload -> {
                reloadFromKernel()
            }
            is AndroidEditorPipeline.PipelineOutput.StaleOrInvalid -> {
                reloadFromKernel()
            }
        }
    }

    private fun reloadFromKernel() {
        if (pipeline.reloadFromKernel()) {
            updateLayoutConfig()
            if (pipeline.getText().isNotEmpty()) {
                onContentChanged?.invoke(pipeline.getText())
            }
        }
    }

    fun onCompositionUpdated() {
        pipeline.onCompositionUpdated()
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    fun applyCompositionUpdate(visualIntent: com.xiwei.sujian.editor.v2.mirror.VisualIntent, mirrorUpdate: (() -> Unit)? = null) {
        pipeline.applyCompositionUpdate(visualIntent, mirrorUpdate)
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    fun getText(): String = pipeline.getText()

    fun setFontSize(sizeSp: Float) {
        textPaint.textSize = sizeSp * resources.displayMetrics.scaledDensity
        updateLayoutConfig()
    }

    private var lineSpacingMultiplier: Float = 1.0f

    fun setLineSpacingMultiplier(multiplier: Float) {
        lineSpacingMultiplier = multiplier
        pipeline.setLineSpacingMultiplier(multiplier)
        updateLayoutConfig()
    }

    fun getSelectionStart(): Int = pipeline.getSelectionStartUtf8()
    fun getSelectionEnd(): Int = pipeline.getSelectionEndUtf8()

    fun setSelectionRange(start: Int, end: Int) {
        setSelectionTyped(start, end)
    }

    fun scrollToSelection() {
        val cursorUtf16 = pipeline.getCursorUtf16()
        if (cursorUtf16 < 0 || cursorUtf16 > pipeline.getLengthUtf16()) return
        val line = pipeline.getLayoutLineForOffset(cursorUtf16)
        val lineTop = pipeline.getLayoutLineTop(line).toFloat()
        val lineBottom = pipeline.getLayoutLineBottom(line).toFloat()
        val viewHeight = height.toFloat()
        if (lineTop < scrollY) {
            scrollY = lineTop
        } else if (lineBottom > scrollY + viewHeight) {
            scrollY = lineBottom - viewHeight
        }
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    fun replaceRange(start: Int, end: Int, newText: String) {
        replaceRangeTyped(start, end, newText, "", EditorTransactionCauseDto.PROGRAMMATIC)
    }

    fun replaceAll(searchStr: String, replaceStr: String) {
        val output = pipeline.replaceAll(searchStr, replaceStr)
        handlePipelineOutput(output)
    }

    fun applyCompositionCommit(dto: uniffi.writer_core.EditorEditResultDto) {
        val output = pipeline.applyCompositionCommit(dto)
        handlePipelineOutput(output)
    }

    fun clearCompositionAndReplace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto) {
        replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause)
    }

    fun setSearchHighlights(highlights: List<Pair<Int, Int>>) {
        searchHighlights = highlights
        invalidate()
    }

    fun clearSearchHighlights() {
        searchHighlights = emptyList()
        invalidate()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        if (w > 0) {
            updateLayoutConfig()
            pendingLayoutNeeded = false
        }
    }

    private fun updateMaxScroll() {
        maxScrollY = pipeline.getLayoutMaxScrollY(height)
    }

    private fun updateLayoutConfig() {
        pipeline.updateLayout(width.toFloat())
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save()
        canvas.translate(-scrollX, -scrollY)
        val searchHighlightsUtf16 = searchHighlights.map { (startUtf8, endUtf8) ->
            Pair(pipeline.utf8ToUtf16(startUtf8), pipeline.utf8ToUtf16(endUtf8))
        }
        pipeline.drawFrame(canvas, searchHighlightsUtf16, width, height, scrollX, scrollY)
        canvas.restore()
        if (pipeline.hasActiveAnimation()) {
            invalidate()
        }
    }

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): InputConnection? {
        return pipeline.onCreateInputConnection(outAttrs)
    }

    override fun onCheckIsTextEditor(): Boolean = true

    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo?) {
        super.onInitializeAccessibilityNodeInfo(info)
        info?.isEditable = true
        info?.text = pipeline.getText()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.action) {
            MotionEvent.ACTION_DOWN -> {
                touchDownX = event.x + scrollX
                touchDownY = event.y + scrollY
                isDragging = false
                requestFocus()
                return true
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = event.x + scrollX - touchDownX
                val dy = event.y + scrollY - touchDownY
                if (!isDragging && (Math.abs(dx) > 10 || Math.abs(dy) > 10)) {
                    isDragging = true
                }
                if (isDragging) {
                    scrollY = (scrollY - dy).coerceIn(0f, maxScrollY)
                    touchDownX = event.x + scrollX
                    touchDownY = event.y + scrollY
                    invalidate()
                }
                return true
            }
            MotionEvent.ACTION_UP -> {
                if (!isDragging) {
                    handleTap(event.x + scrollX, event.y + scrollY)
                }
                isDragging = false
                return true
            }
        }
        return super.onTouchEvent(event)
    }

    private var touchDownX: Float = 0f
    private var touchDownY: Float = 0f
    private var isDragging: Boolean = false

    private fun handleTap(x: Float, y: Float) {
        val line = pipeline.getLayoutLineForVertical(y.toInt())
        val offset = pipeline.getLayoutOffsetForHorizontal(line, x)
        val byteOffset = pipeline.utf16ToUtf8(offset)
        setSelectionTyped(byteOffset, byteOffset)
        showSoftInput()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        when (keyCode) {
            KeyEvent.KEYCODE_DEL -> {
                val selStart = pipeline.getSelectionStartUtf8()
                val selEnd = pipeline.getSelectionEndUtf8()
                if (selStart != selEnd) {
                    replaceRange(selStart, selEnd, "")
                } else if (selEnd > 0) {
                    val prevGraphemeLen = pipeline.previousGraphemeByteLen(selEnd)
                    if (prevGraphemeLen > 0) {
                        replaceRange(selEnd - prevGraphemeLen, selEnd, "")
                    }
                }
                return true
            }
            KeyEvent.KEYCODE_FORWARD_DEL -> {
                val selStart = pipeline.getSelectionStartUtf8()
                val selEnd = pipeline.getSelectionEndUtf8()
                if (selStart != selEnd) {
                    replaceRange(selStart, selEnd, "")
                } else {
                    val textLen = pipeline.getText().toByteArray(Charsets.UTF_8).size
                    if (selEnd < textLen) {
                        val nextGraphemeLen = pipeline.nextGraphemeByteLen(selEnd)
                        if (nextGraphemeLen > 0) {
                            replaceRange(selEnd, selEnd + nextGraphemeLen, "")
                        }
                    }
                }
                return true
            }
        }
        return super.onKeyDown(keyCode, event)
    }

    override fun onFocusChanged(gained: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(gained, direction, previouslyFocusedRect)
        if (gained) {
            showSoftInput()
        }
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (!hasWindowFocus) {
            pipeline.cancelActiveTransaction()
            pipeline.releaseAllResources()
        }
    }

    fun requestNextFrame() {
        invalidate()
    }

    fun getPipeline(): AndroidEditorPipeline = pipeline

    var onContentChanged: ((String) -> Unit)? = null

    fun setText(text: String) {
        loadText(text, 0)
    }

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long) {
        pipeline.kernelBridge?.setAnimationEnabled(enabled)
        pipeline.kernelBridge?.setAnimationDurationMs(durationMs)
        if (!enabled) {
            pipeline.cancelActiveTransaction()
        }
    }

    private var smoothCursorEnabled: Boolean = true
    private var smoothCursorDurationMs: Long = 160

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
    }

    fun isSmoothCursorEnabled(): Boolean = smoothCursorEnabled

    fun setAutoIndent(enabled: Boolean, widthSp: Float) {
        pipeline.setAutoIndent(enabled, widthSp)
    }

    fun isAutoIndentEnabled(): Boolean = pipeline.isAutoIndentEnabled()
    fun getAutoIndentWidthSp(): Float = pipeline.getAutoIndentWidthSp()

    private var coordinatedAnimationEnabled: Boolean = true

    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        coordinatedAnimationEnabled = enabled
    }

    fun isCoordinatedAnimationEnabled(): Boolean = coordinatedAnimationEnabled

    fun applyThemeColorsFromAdapter(colors: com.xiwei.sujian.ui.compose.theme.EditorThemeColors) {
        themeBackgroundColor = colors.background
        textPaint.color = colors.text
        pipeline.setThemeColors(
            textColor = colors.text,
            cursorColor = colors.cursor,
            selectionColor = colors.selection,
            preeditColor = colors.composing,
            bgColor = colors.background
        )
        invalidate()
    }

    private fun showSoftInput() {
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
        imm?.showSoftInput(this, 0)
    }

    fun notifyCursorAnchorInfo() {
        val imm = context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager ?: return
        val cursorUtf16 = pipeline.getCursorUtf16()
        if (cursorUtf16 < 0 || cursorUtf16 > pipeline.getLengthUtf16()) return

        val line = pipeline.getLayoutLineForOffset(cursorUtf16)
        val x = pipeline.getLayoutPrimaryHorizontal(cursorUtf16)
        val lineTop = pipeline.getLayoutLineTop(line)
        val lineBottom = pipeline.getLayoutLineBottom(line)

        val info = android.view.inputmethod.CursorAnchorInfo.Builder()
            .setSelectionRange(cursorUtf16, cursorUtf16)
            .setInsertionMarkerLocation(x, lineTop.toFloat(), lineBottom.toFloat(), lineBottom.toFloat(), android.view.inputmethod.CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION)
            .build()
        imm.updateCursorAnchorInfo(this, info)
    }
}

interface EditorKernelBridge {
    fun insert(byteOffset: Int, text: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun delete(byteStart: Int, byteEndExclusive: Int, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun replace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun setSelection(anchorByteOffset: Int, headByteOffset: Int, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun undo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun redo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun loadText(text: String, cursorUtf8: Int): uniffi.writer_core.EditorEditResultDto?
    fun compositionCommit(
        compositionReplaceStart: Int,
        compositionReplaceEndExclusive: Int,
        committedText: String,
        originalText: String
    ): uniffi.writer_core.EditorEditResultDto?
    fun compositionUpdateVisualIntent(
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String
    ): uniffi.writer_core.EditorVisualIntentDto?
    fun setAnimationEnabled(enabled: Boolean)
    fun setAnimationDurationMs(durationMs: Long)
    fun replaceAll(search: String, replacement: String, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun insertLineBreak(byteOffset: Int, autoIndentPrefix: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?
    fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto?
}
