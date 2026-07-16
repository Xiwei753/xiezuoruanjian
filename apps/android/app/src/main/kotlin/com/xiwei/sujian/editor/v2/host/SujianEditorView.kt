package com.xiwei.sujian.editor.v2.host

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.text.TextPaint
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

class SujianEditorView(
    context: Context
) : View(context) {

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

    fun applyCommand(commandJson: String) {
        val bridge = kernelBridge ?: return
        val dto = bridge.apply(commandJson) ?: return
        val result = EditResult.fromDto(dto)
        mirror.applyPatches(result.displayPatches)
        layoutEngine.requestLayout()
        val transaction = visualPlanner.prepare(result.visualIntent, layoutEngine)
        renderer.submitTransaction(transaction)
        selectionStartUtf8 = result.newSelectionStart
        selectionEndUtf8 = result.newSelectionEnd
        isSelectionActive = selectionStartUtf8 != selectionEndUtf8
        invalidate()
    }

    fun onCompositionUpdated() {
        layoutEngine.requestLayout()
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
        val commandJson = """{"kind":"SetSelection","anchor_byte_offset":$start,"head_byte_offset":$end}"""
        applyCommand(commandJson)
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
        val escapedText = newText.replace("\\", "\\\\").replace("\"", "\\\"")
            .replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
        val commandJson = """{"kind":"Replace","byte_start":$start,"byte_end_exclusive":$end,"replacement_text":"$escapedText","original_text":"","cause":"Programmatic"}"""
        applyCommand(commandJson)
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        layoutEngine.setWidth(w.toFloat())
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
        val commandJson = """{"kind":"SetSelection","anchor_byte_offset":$byteOffset,"head_byte_offset":$byteOffset}"""
        applyCommand(commandJson)
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
        val text = mirror.getText()
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (offset <= 0 || offset > bytes.size) return 0
        var pos = 0
        var charStart = 0
        for (char in text) {
            val charLen = char.toString().toByteArray(Charsets.UTF_8).size
            if (pos + charLen == offset) return charLen
            pos += charLen
        }
        val remaining = offset
        var p = remaining - 1
        while (p > 0 && (bytes[p] and 0xC0.toByte()) == 0x80.toByte()) p--
        return remaining - p
    }

    private fun nextCharByteLen(offset: Int): Int {
        val text = mirror.getText()
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (offset >= bytes.size) return 0
        var len = 1
        while (offset + len < bytes.size && (bytes[offset + len] and 0xC0.toByte()) == 0x80.toByte()) len++
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

    private fun showSoftInput() {
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
        imm?.showSoftInput(this, 0)
    }
}

interface EditorKernelBridge {
    fun apply(commandJson: String): EditorEditResultDto?
    fun loadText(text: String, cursorUtf8: Int): EditorEditResultDto?
    fun compositionCommit(
        compositionReplaceStart: Int,
        compositionReplaceEndExclusive: Int,
        committedText: String,
        originalText: String
    ): EditorEditResultDto?
}
