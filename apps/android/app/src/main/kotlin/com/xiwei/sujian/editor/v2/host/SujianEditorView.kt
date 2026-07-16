package com.xiwei.sujian.editor.v2.host

import android.content.Context
import android.graphics.Canvas
import android.text.TextPaint
import android.view.View
import android.view.inputmethod.InputConnection
import com.xiwei.sujian.editor.v2.input.AndroidInputAdapter
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.render.AndroidRenderer

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
    private val inputAdapter = AndroidInputAdapter(context, mirror, layoutEngine, visualPlanner, renderer, this)

    var kernelBridge: EditorKernelBridge? = null

    fun loadText(text: String, cursorUtf8: Int) {
        val bridge = kernelBridge ?: return
        val resultJson = bridge.loadText(text, cursorUtf8)
        val result = EditResult.fromJson(resultJson)
        mirror.applyPatches(result.displayPatches)
        layoutEngine.setWidth(width.toFloat())
        invalidate()
    }

    fun applyCommand(commandJson: String) {
        val bridge = kernelBridge ?: return
        val resultJson = bridge.apply(commandJson)
        val result = EditResult.fromJson(resultJson)
        mirror.applyPatches(result.displayPatches)
        layoutEngine.requestLayout()
        val transaction = visualPlanner.prepare(result.visualIntent, layoutEngine)
        renderer.submitTransaction(transaction)
        invalidate()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        layoutEngine.setWidth(w.toFloat())
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val frameTimeMs = System.nanoTime() / 1_000_000
        renderer.renderFrame(canvas, frameTimeMs)
    }

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): InputConnection? {
        return inputAdapter.onCreateInputConnection(outAttrs)
    }

    override fun onCheckIsTextEditor(): Boolean = true

    fun requestNextFrame() {
        invalidate()
    }

    fun getMirror(): DisplayTextMirror = mirror
    fun getLayoutEngine(): AndroidLayoutEngine = layoutEngine
    fun getRenderer(): AndroidRenderer = renderer
    fun getInputAdapter(): AndroidInputAdapter = inputAdapter
}

interface EditorKernelBridge {
    fun apply(commandJson: String): String
    fun loadText(text: String, cursorUtf8: Int): String
}
