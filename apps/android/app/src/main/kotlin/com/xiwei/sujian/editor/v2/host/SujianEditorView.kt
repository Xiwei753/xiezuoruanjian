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
import com.xiwei.sujian.editor.v2.input.AndroidInputAdapter
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.VisualTransactionCoordinator
import com.xiwei.sujian.editor.v2.render.AndroidRenderer
import uniffi.writer_core.EditorEditResultDto

class SujianEditorView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private val mirror = DisplayTextMirror()
    private val textPaint = TextPaint().apply {
        textSize = 48f
        isAntiAlias = true
    }
    private val layoutEngine = AndroidLayoutEngine(mirror, textPaint)
    private val visualPlanner = AndroidVisualPlanner()
    private val resourceStore = VisualResourceStore()
    private val coordinator = VisualTransactionCoordinator(resourceStore)
    private val renderer = AndroidRenderer()
    private val inputAdapter = AndroidInputAdapter(context, mirror, this)

    private var scrollX: Float = 0f
    private var scrollY: Float = 0f
    private var maxScrollY: Float = 0f
    private var searchHighlights: List<Pair<Int, Int>> = emptyList()

    var kernelBridge: EditorKernelBridge? = null

    private var themeBackgroundColor: Int = Color.WHITE

    fun loadText(text: String, cursorUtf8: Int) {
        val bridge = kernelBridge ?: return
        val dto = bridge.loadText(text, cursorUtf8) ?: return
        val result = EditResult.fromDto(dto)
        mirror.loadFromSnapshot(text, cursorUtf8, result.newRevision, result.newSelectionStart, result.newSelectionEnd)
        coordinator.cancelActiveTransaction()
        resourceStore.releaseAll()
        visualPlanner.resetOldRevision()
        updateLayoutConfig()
    }

    fun insertText(byteOffset: Int, text: String, cause: uniffi.writer_core.EditorTransactionCauseDto = uniffi.writer_core.EditorTransactionCauseDto.TYPING) {
        val bridge = kernelBridge ?: return
        if (autoIndentEnabled && text == "\n") {
            val indentPrefix = computeAutoIndentPrefix()
            val dto = bridge.insertLineBreak(byteOffset, indentPrefix, cause, mirror.getRevision()) ?: return
            val result = EditResult.fromDto(dto)
            applyEditResultFull(result)
        } else {
            val dto = bridge.insert(byteOffset, text, cause, mirror.getRevision()) ?: return
            val result = EditResult.fromDto(dto)
            applyEditResultFull(result)
        }
    }

    fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: uniffi.writer_core.EditorTransactionCauseDto = uniffi.writer_core.EditorTransactionCauseDto.DELETE) {
        val bridge = kernelBridge ?: return
        val dto = bridge.delete(byteStart, byteEndExclusive, cause, mirror.getRevision()) ?: return
        val result = EditResult.fromDto(dto)
        applyEditResultFull(result)
    }

    fun replaceRangeTyped(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto = uniffi.writer_core.EditorTransactionCauseDto.TYPING, beforePatch: (() -> Unit)? = null) {
        val bridge = kernelBridge ?: return
        val dto = bridge.replace(byteStart, byteEndExclusive, replacementText, originalText, cause, mirror.getRevision()) ?: return
        val result = EditResult.fromDto(dto)
        applyEditResultFull(result, beforePatch)
    }

    fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int) {
        val bridge = kernelBridge ?: return
        val dto = bridge.setSelection(anchorByteOffset, headByteOffset, mirror.getRevision()) ?: return
        val result = EditResult.fromDto(dto)
        applyEditResultFull(result)
    }

    fun performUndo() {
        val bridge = kernelBridge ?: return
        val dto = bridge.undo(mirror.getRevision()) ?: return
        val result = EditResult.fromDto(dto)
        applyEditResultFull(result)
    }

    fun performRedo() {
        val bridge = kernelBridge ?: return
        val dto = bridge.redo(mirror.getRevision()) ?: return
        val result = EditResult.fromDto(dto)
        applyEditResultFull(result)
    }

    private fun applyEditResultFull(result: EditResult, beforePatch: (() -> Unit)? = null) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val rebaseSnapshot = coordinator.captureCurrentFrame(frameTimeMs)
        val oldRevision = layoutEngine.captureImmutableRevision()
        val affectedOldLineIndices = visualPlanner.computeAffectedLineIndices(result.visualIntent, oldRevision, useNewRanges = false)
        val oldSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedOldLineIndices)
        if (beforePatch != null) {
            beforePatch.invoke()
        } else {
            mirror.restoreCompositionBeforePatch()
        }
        mirror.applyEditResult(result)
        layoutEngine.requestLayout()
        val newRevision = layoutEngine.getCurrentRevision()
        val affectedNewLineIndices = visualPlanner.computeAffectedLineIndices(result.visualIntent, newRevision, useNewRanges = true)
        val newSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedNewLineIndices)
        val transaction = visualPlanner.prepare(result.visualIntent, oldRevision, newRevision, resourceStore, oldSnapshots, newSnapshots, rebaseSnapshot)
        coordinator.submitTransaction(transaction)
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        if (result.displayPatches.isNotEmpty()) {
            onContentChanged?.invoke(mirror.getText())
        }
        invalidate()
    }

    fun onCompositionUpdated() {
        layoutEngine.requestLayout()
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    fun applyCompositionUpdate(visualIntent: com.xiwei.sujian.editor.v2.mirror.VisualIntent, mirrorUpdate: (() -> Unit)? = null) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val rebaseSnapshot = coordinator.captureCurrentFrame(frameTimeMs)
        val oldRevision = layoutEngine.captureImmutableRevision()
        val affectedOldLineIndices = visualPlanner.computeAffectedLineIndices(visualIntent, oldRevision, useNewRanges = false)
        val oldSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedOldLineIndices)
        mirrorUpdate?.invoke()
        layoutEngine.requestLayout()
        val newRevision = layoutEngine.getCurrentRevision()
        val affectedNewLineIndices = visualPlanner.computeAffectedLineIndices(visualIntent, newRevision, useNewRanges = true)
        val newSnapshots = layoutEngine.captureLineBitmapSnapshotsWithClusters(affectedNewLineIndices)
        val transaction = visualPlanner.prepare(visualIntent, oldRevision, newRevision, resourceStore, oldSnapshots, newSnapshots, rebaseSnapshot)
        coordinator.submitTransaction(transaction)
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    fun getText(): String = mirror.getText()

    fun setFontSize(sizeSp: Float) {
        textPaint.textSize = sizeSp * resources.displayMetrics.scaledDensity
        updateLayoutConfig()
    }

    private var lineSpacingMultiplier: Float = 1.0f

    fun setLineSpacingMultiplier(multiplier: Float) {
        lineSpacingMultiplier = multiplier
        layoutEngine.setLineSpacingMultiplier(multiplier)
        updateLayoutConfig()
    }

    fun getSelectionStart(): Int = mirror.getSelectionStartUtf8()

    fun getSelectionEnd(): Int = mirror.getSelectionEndUtf8()

    fun setSelectionRange(start: Int, end: Int) {
        setSelectionTyped(start, end)
    }

    fun scrollToSelection() {
        val layout = layoutEngine.getLayout() ?: return
        val cursorUtf16 = mirror.getCursorUtf16()
        if (cursorUtf16 < 0 || cursorUtf16 > mirror.getLengthUtf16()) return
        val line = layout.getLineForOffset(cursorUtf16)
        val lineTop = layout.getLineTop(line).toFloat()
        val lineBottom = layout.getLineBottom(line).toFloat()
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
        replaceRangeTyped(start, end, newText, "", uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC)
    }

    fun replaceAll(searchStr: String, replaceStr: String) {
        val bridge = kernelBridge ?: return
        val dto = bridge.replaceAll(searchStr, replaceStr, mirror.getRevision()) ?: return
        val result = EditResult.fromDto(dto)
        applyEditResultFull(result)
    }

    fun applyCompositionCommit(dto: uniffi.writer_core.EditorEditResultDto) {
        val result = EditResult.fromDto(dto)
        applyEditResultFull(result) {
            mirror.restoreCompositionBeforePatch()
        }
    }

    fun clearCompositionAndReplace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto) {
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
        }
    }

    private fun updateMaxScroll() {
        val layout = layoutEngine.getLayout()
        if (layout != null) {
            maxScrollY = (layout.height - height).coerceAtLeast(0).toFloat()
        }
    }

    private fun updateLayoutConfig() {
        layoutEngine.setWidth(width.toFloat())
        layoutEngine.requestLayout()
        updateMaxScroll()
        scrollY = scrollY.coerceIn(0f, maxScrollY)
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save()
        canvas.translate(-scrollX, -scrollY)
        val frameTimeMs = System.nanoTime() / 1_000_000
        val layout = layoutEngine.getLayout()
        if (layout != null) {
            val rev = layoutEngine.getCurrentRevision()
            val searchHighlightsUtf16 = searchHighlights.map { (startUtf8, endUtf8) ->
                val indexMap = com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap(mirror)
                Pair(indexMap.utf8ToUtf16(startUtf8), indexMap.utf8ToUtf16(endUtf8))
            }
            val frame = coordinator.computeFrame(
                frameTimeMs,
                cursorUtf16 = rev?.cursorUtf16 ?: mirror.getCursorUtf16(),
                cursorX = rev?.cursorX ?: 0f,
                cursorY = rev?.cursorY ?: 0f,
                cursorHeight = rev?.cursorHeight ?: 0f,
                selectionStartUtf16 = rev?.selectionStartUtf16 ?: mirror.getSelectionStartUtf16(),
                selectionEndUtf16 = rev?.selectionEndUtf16 ?: mirror.getSelectionEndUtf16(),
                compositionStartUtf16 = rev?.compositionStartUtf16 ?: -1,
                compositionEndUtf16 = rev?.compositionEndUtf16 ?: -1,
                searchHighlightsUtf16 = searchHighlightsUtf16,
                viewportWidth = width,
                viewportHeight = height,
                scrollX = scrollX,
                scrollY = scrollY
            )
            renderer.draw(canvas, layout, frame)
        }
        canvas.restore()
        if (coordinator.hasActiveAnimation()) {
            invalidate()
        }
    }

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): InputConnection? {
        return inputAdapter.onCreateInputConnection(outAttrs)
    }

    override fun onCheckIsTextEditor(): Boolean = true

    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo?) {
        super.onInitializeAccessibilityNodeInfo(info)
        info?.isEditable = true
        info?.text = mirror.getText()
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
        val layout = layoutEngine.getLayout() ?: return
        val line = layout.getLineForVertical(y.toInt())
        val offset = layout.getOffsetForHorizontal(line, x)
        val indexMap = com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap(mirror)
        val byteOffset = indexMap.utf16ToUtf8(offset)
        setSelectionTyped(byteOffset, byteOffset)
        showSoftInput()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        when (keyCode) {
            KeyEvent.KEYCODE_DEL -> {
                val selStart = mirror.getSelectionStartUtf8()
                val selEnd = mirror.getSelectionEndUtf8()
                if (selStart != selEnd) {
                    replaceRange(selStart, selEnd, "")
                } else if (selEnd > 0) {
                    val prevGraphemeLen = previousGraphemeByteLen(selEnd)
                    if (prevGraphemeLen > 0) {
                        replaceRange(selEnd - prevGraphemeLen, selEnd, "")
                    }
                }
                return true
            }
            KeyEvent.KEYCODE_FORWARD_DEL -> {
                val selStart = mirror.getSelectionStartUtf8()
                val selEnd = mirror.getSelectionEndUtf8()
                if (selStart != selEnd) {
                    replaceRange(selStart, selEnd, "")
                } else {
                    val textLen = mirror.getText().toByteArray(Charsets.UTF_8).size
                    if (selEnd < textLen) {
                        val nextGraphemeLen = nextGraphemeByteLen(selEnd)
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

    private fun previousGraphemeByteLen(offset: Int): Int {
        val indexMap = com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap(mirror)
        val utf16Offset = indexMap.utf8ToUtf16(offset)
        if (utf16Offset <= 0) return 0
        val iter = android.icu.text.BreakIterator.getCharacterInstance()
        iter.setText(mirror.getText())
        val prev = iter.preceding(utf16Offset)
        if (prev == android.icu.text.BreakIterator.DONE) return 0
        val prevUtf8 = indexMap.utf16ToUtf8(prev)
        return offset - prevUtf8
    }

    private fun nextGraphemeByteLen(offset: Int): Int {
        val indexMap = com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap(mirror)
        val utf16Offset = indexMap.utf8ToUtf16(offset)
        if (utf16Offset >= mirror.getLengthUtf16()) return 0
        val iter = android.icu.text.BreakIterator.getCharacterInstance()
        iter.setText(mirror.getText())
        val next = iter.following(utf16Offset)
        if (next == android.icu.text.BreakIterator.DONE) return 0
        val nextUtf8 = indexMap.utf16ToUtf8(next)
        return nextUtf8 - offset
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
            coordinator.cancelActiveTransaction()
            resourceStore.releaseAll()
            visualPlanner.resetOldRevision()
        }
    }

    fun requestNextFrame() {
        invalidate()
    }

    fun getMirror(): DisplayTextMirror = mirror
    fun getLayoutEngine(): AndroidLayoutEngine = layoutEngine
    fun getRenderer(): AndroidRenderer = renderer
    fun getInputAdapter(): AndroidInputAdapter = inputAdapter

    var onContentChanged: ((String) -> Unit)? = null

    fun setText(text: String) {
        loadText(text, 0)
    }

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long) {
        val bridge = kernelBridge ?: return
        bridge.setAnimationEnabled(enabled)
        bridge.setAnimationDurationMs(durationMs)
        if (!enabled) {
            coordinator.cancelActiveTransaction()
        }
    }

    private var smoothCursorEnabled: Boolean = true
    private var smoothCursorDurationMs: Long = 160

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
    }

    fun isSmoothCursorEnabled(): Boolean = smoothCursorEnabled

    private var autoIndentEnabled: Boolean = false
    private var autoIndentWidthSp: Float = 2f

    fun setAutoIndent(enabled: Boolean, widthSp: Float) {
        autoIndentEnabled = enabled
        autoIndentWidthSp = widthSp
    }

    fun isAutoIndentEnabled(): Boolean = autoIndentEnabled

    fun getAutoIndentWidthSp(): Float = autoIndentWidthSp

    private var coordinatedAnimationEnabled: Boolean = true

    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        coordinatedAnimationEnabled = enabled
    }

    fun isCoordinatedAnimationEnabled(): Boolean = coordinatedAnimationEnabled

    private fun computeAutoIndentPrefix(): String {
        if (!autoIndentEnabled) return ""
        val indexMap = com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap(mirror)
        val cursorUtf8 = mirror.getCursorUtf8()
        val cursorUtf16 = indexMap.utf8ToUtf16(cursorUtf8)
        val text = mirror.getText()
        val safeCursorUtf16 = cursorUtf16.coerceIn(0, text.length)
        val lineStartUtf16 = text.lastIndexOf('\n', (safeCursorUtf16 - 1).coerceAtLeast(0)) + 1
        val linePrefix = text.substring(lineStartUtf16, safeCursorUtf16)
        val indent = linePrefix.takeWhile { it == ' ' || it == '\t' }
        return indent
    }

    fun applyThemeColorsFromAdapter(colors: com.xiwei.sujian.ui.compose.theme.EditorThemeColors) {
        themeBackgroundColor = colors.background
        textPaint.color = colors.text
        renderer.setThemeColors(
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
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager ?: return
        val layout = layoutEngine.getLayout() ?: return
        val cursorUtf16 = mirror.getCursorUtf16()
        if (cursorUtf16 < 0 || cursorUtf16 > mirror.getLengthUtf16()) return

        val line = layout.getLineForOffset(cursorUtf16)
        val x = layout.getPrimaryHorizontal(cursorUtf16)
        val lineTop = layout.getLineTop(line)
        val lineBottom = layout.getLineBottom(line)

        val info = android.view.inputmethod.CursorAnchorInfo.Builder()
            .setSelectionRange(cursorUtf16, cursorUtf16)
            .setInsertionMarkerLocation(x, lineTop.toFloat(), lineBottom.toFloat(), lineBottom.toFloat(), android.view.inputmethod.CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION)
            .build()
        imm.updateCursorAnchorInfo(this, info)
    }
}

interface EditorKernelBridge {
    fun insert(byteOffset: Int, text: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto?
    fun delete(byteStart: Int, byteEndExclusive: Int, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto?
    fun replace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto?
    fun setSelection(anchorByteOffset: Int, headByteOffset: Int, expectedRevision: Long): EditorEditResultDto?
    fun undo(expectedRevision: Long): EditorEditResultDto?
    fun redo(expectedRevision: Long): EditorEditResultDto?
    fun loadText(text: String, cursorUtf8: Int): EditorEditResultDto?
    fun compositionCommit(
        compositionReplaceStart: Int,
        compositionReplaceEndExclusive: Int,
        committedText: String,
        originalText: String
    ): EditorEditResultDto?
    fun compositionUpdateVisualIntent(
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String
    ): uniffi.writer_core.EditorVisualIntentDto?
    fun setAnimationEnabled(enabled: Boolean)
    fun setAnimationDurationMs(durationMs: Long)
    fun replaceAll(search: String, replacement: String, expectedRevision: Long): EditorEditResultDto?
    fun insertLineBreak(byteOffset: Int, autoIndentPrefix: String, cause: uniffi.writer_core.EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto?
}
