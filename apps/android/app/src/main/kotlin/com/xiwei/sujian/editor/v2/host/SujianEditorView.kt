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
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.TransactionState
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
    private val renderer = AndroidRenderer(mirror, layoutEngine, resourceStore)
    private val inputAdapter = AndroidInputAdapter(context, mirror, this)

    private var scrollX: Float = 0f
    private var scrollY: Float = 0f
    private var maxScrollY: Float = 0f
    private var selectionStartUtf8: Int = 0
    private var selectionEndUtf8: Int = 0
    private var isSelectionActive: Boolean = false
    private var touchDownX: Float = 0f
    private var touchDownY: Float = 0f
    private var isDragging: Boolean = false
    private var searchHighlights: List<Pair<Int, Int>> = emptyList()

    var kernelBridge: EditorKernelBridge? = null

    fun loadText(text: String, cursorUtf8: Int) {
        val bridge = kernelBridge ?: return
        val dto = bridge.loadText(text, cursorUtf8) ?: return
        val result = EditResult.fromDto(dto)
        mirror.applyPatches(result.displayPatches)
        layoutEngine.setWidth(width.toFloat())
        visualPlanner.resetOldRevision()
        selectionStartUtf8 = cursorUtf8
        selectionEndUtf8 = cursorUtf8
        isSelectionActive = false
        invalidate()
    }

    fun insertText(byteOffset: Int, text: String, cause: uniffi.writer_core.EditorTransactionCauseDto = uniffi.writer_core.EditorTransactionCauseDto.TYPING) {
        val bridge = kernelBridge ?: return
        val dto = bridge.insert(byteOffset, text, cause) ?: return
        val result = EditResult.fromDto(dto)
        mirror.applyPatches(result.displayPatches)
        applyCommandResult(result)
    }

    fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: uniffi.writer_core.EditorTransactionCauseDto = uniffi.writer_core.EditorTransactionCauseDto.DELETE) {
        val bridge = kernelBridge ?: return
        val dto = bridge.delete(byteStart, byteEndExclusive, cause) ?: return
        val result = EditResult.fromDto(dto)
        mirror.applyPatches(result.displayPatches)
        applyCommandResult(result)
    }

    fun replaceRangeTyped(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto = uniffi.writer_core.EditorTransactionCauseDto.TYPING) {
        val bridge = kernelBridge ?: return
        val dto = bridge.replace(byteStart, byteEndExclusive, replacementText, originalText, cause) ?: return
        val result = EditResult.fromDto(dto)
        mirror.applyPatches(result.displayPatches)
        applyCommandResult(result)
    }

    fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int) {
        val bridge = kernelBridge ?: return
        val dto = bridge.setSelection(anchorByteOffset, headByteOffset) ?: return
        val result = EditResult.fromDto(dto)
        applyCommandResult(result)
    }

    fun performUndo() {
        val bridge = kernelBridge ?: return
        val dto = bridge.undo() ?: return
        val result = EditResult.fromDto(dto)
        mirror.applyPatches(result.displayPatches)
        applyCommandResult(result)
    }

    fun performRedo() {
        val bridge = kernelBridge ?: return
        val dto = bridge.redo() ?: return
        val result = EditResult.fromDto(dto)
        mirror.applyPatches(result.displayPatches)
        applyCommandResult(result)
    }

    fun applyCommandResult(result: EditResult) {
        layoutEngine.requestLayout()
        val transaction = visualPlanner.prepare(result.visualIntent, layoutEngine, resourceStore)
        renderer.submitTransaction(transaction)
        selectionStartUtf8 = result.newSelectionStart
        selectionEndUtf8 = result.newSelectionEnd
        isSelectionActive = selectionStartUtf8 != selectionEndUtf8
        if (result.displayPatches.isNotEmpty()) {
            onContentChanged?.invoke(mirror.getText())
        }
        invalidate()
    }

    fun onCompositionUpdated() {
        layoutEngine.requestLayout()
        invalidate()
    }

    fun applyCompositionUpdate(visualIntent: com.xiwei.sujian.editor.v2.mirror.VisualIntent) {
        layoutEngine.requestLayout()
        val transaction = visualPlanner.prepare(visualIntent, layoutEngine, resourceStore)
        renderer.submitTransaction(transaction)
        invalidate()
    }

    fun getText(): String = mirror.getText()

    fun setFontSize(sizeSp: Float) {
        textPaint.textSize = sizeSp * resources.displayMetrics.scaledDensity
        layoutEngine.setWidth(width.toFloat())
        layoutEngine.requestLayout()
        invalidate()
    }

    private var lineSpacingMultiplier: Float = 1.0f

    fun setLineSpacingMultiplier(multiplier: Float) {
        lineSpacingMultiplier = multiplier
        layoutEngine.setLineSpacingMultiplier(multiplier)
        layoutEngine.requestLayout()
        invalidate()
    }

    fun getSelectionStart(): Int = selectionStartUtf8

    fun getSelectionEnd(): Int = selectionEndUtf8

    fun setSelectionRange(start: Int, end: Int) {
        selectionStartUtf8 = start
        selectionEndUtf8 = end
        isSelectionActive = start != end
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
        val text = mirror.getText()
        val newText = text.replace(searchStr, replaceStr)
        if (newText != text) {
            loadText(newText, mirror.getCursorUtf8())
            onContentChanged?.invoke(newText)
        }
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
        layoutEngine.setWidth(w.toFloat())
        updateMaxScroll()
    }

    private fun updateMaxScroll() {
        val layout = layoutEngine.getLayout()
        if (layout != null) {
            maxScrollY = (layout.height - height).coerceAtLeast(0).toFloat()
        }
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save()
        canvas.translate(-scrollX, -scrollY)
        val frameTimeMs = System.nanoTime() / 1_000_000
        renderer.renderFrame(canvas, frameTimeMs)
        canvas.restore()
        if (renderer.hasActiveAnimation()) {
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

    private fun handleTap(x: Float, y: Float) {
        val layout = layoutEngine.getLayout() ?: return
        val line = layout.getLineForVertical(y.toInt())
        val offset = layout.getOffsetForHorizontal(line, x)
        val indexMap = com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap(mirror)
        val byteOffset = indexMap.utf16ToUtf8(offset)
        selectionStartUtf8 = byteOffset
        selectionEndUtf8 = byteOffset
        isSelectionActive = false
        setSelectionTyped(byteOffset, byteOffset)
        showSoftInput()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        when (keyCode) {
            KeyEvent.KEYCODE_DEL -> {
                if (isSelectionActive) {
                    replaceRange(selectionStartUtf8, selectionEndUtf8, "")
                } else if (selectionEndUtf8 > 0) {
                    val prevCharLen = previousCharByteLen(selectionEndUtf8)
                    replaceRange(selectionEndUtf8 - prevCharLen, selectionEndUtf8, "")
                }
                return true
            }
            KeyEvent.KEYCODE_FORWARD_DEL -> {
                if (isSelectionActive) {
                    replaceRange(selectionStartUtf8, selectionEndUtf8, "")
                } else {
                    val textLen = mirror.getText().toByteArray(Charsets.UTF_8).size
                    if (selectionEndUtf8 < textLen) {
                        val nextCharLen = nextCharByteLen(selectionEndUtf8)
                        replaceRange(selectionEndUtf8, selectionEndUtf8 + nextCharLen, "")
                    }
                }
                return true
            }
        }
        return super.onKeyDown(keyCode, event)
    }

    private fun previousCharByteLen(offset: Int): Int {
        val bytes = mirror.getText().toByteArray(Charsets.UTF_8)
        if (offset <= 0 || offset > bytes.size) return 0
        var p = offset - 1
        while (p > 0 && (bytes[p].toInt() and 0xC0) == 0x80) p--
        return offset - p
    }

    private fun nextCharByteLen(offset: Int): Int {
        val text = mirror.getText()
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (offset >= bytes.size) return 0
        var len = 1
        while (offset + len < bytes.size && (bytes[offset + len].toInt() and 0xC0) == 0x80) len++
        return len
    }

    override fun onFocusChanged(gained: Boolean, direction: Int, previouslyFocusedRect: android.graphics.Rect?) {
        super.onFocusChanged(gained, direction, previouslyFocusedRect)
        if (gained) {
            showSoftInput()
        }
    }

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (!hasWindowFocus && renderer.hasActiveAnimation()) {
            resourceStore.releaseAll()
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
        onContentChanged?.invoke(text)
    }

    fun setTypingAnimationEnabled(enabled: Boolean, durationMs: Long) {
        val bridge = kernelBridge ?: return
        bridge.setAnimationEnabled(enabled)
        bridge.setAnimationDurationMs(durationMs)
    }

    private var smoothCursorEnabled: Boolean = true
    private var smoothCursorDurationMs: Long = 160

    fun setSmoothCursorEnabled(enabled: Boolean, durationMs: Long) {
        smoothCursorEnabled = enabled
        smoothCursorDurationMs = durationMs
    }

    private var autoIndentEnabled: Boolean = false
    private var autoIndentWidthSp: Float = 2f

    fun setAutoIndent(enabled: Boolean, widthSp: Float) {
        autoIndentEnabled = enabled
        autoIndentWidthSp = widthSp
    }

    private var coordinatedAnimationEnabled: Boolean = true

    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        coordinatedAnimationEnabled = enabled
    }

    fun applyThemeColorsFromAdapter(colors: com.xiwei.sujian.ui.compose.theme.EditorThemeColors) {
        setBackgroundColor(colors.background)
        textPaint.color = colors.text
        renderer.setThemeColors(
            textColor = colors.text,
            cursorColor = colors.cursor,
            selectionColor = colors.selection,
            preeditColor = colors.composing
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
    fun insert(byteOffset: Int, text: String, cause: uniffi.writer_core.EditorTransactionCauseDto): EditorEditResultDto?
    fun delete(byteStart: Int, byteEndExclusive: Int, cause: uniffi.writer_core.EditorTransactionCauseDto): EditorEditResultDto?
    fun replace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: uniffi.writer_core.EditorTransactionCauseDto): EditorEditResultDto?
    fun setSelection(anchorByteOffset: Int, headByteOffset: Int): EditorEditResultDto?
    fun undo(): EditorEditResultDto?
    fun redo(): EditorEditResultDto?
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
}
