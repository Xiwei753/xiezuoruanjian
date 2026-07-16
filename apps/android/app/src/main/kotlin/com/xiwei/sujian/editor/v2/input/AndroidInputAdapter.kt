package com.xiwei.sujian.editor.v2.input

import android.content.Context
import android.view.View
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.render.AndroidRenderer
import com.xiwei.sujian.editor.v2.host.SujianEditorView

class AndroidInputAdapter(
    context: Context,
    private val mirror: DisplayTextMirror,
    private val layoutEngine: AndroidLayoutEngine,
    private val visualPlanner: AndroidVisualPlanner,
    private val renderer: AndroidRenderer,
    private val editorView: SujianEditorView
) : View(context) {

    private var currentCompositionText: String = ""
    private var compositionReplaceStartUtf8: Int = 0
    private var compositionReplaceEndUtf8: Int = 0
    private var isComposing: Boolean = false

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): android.view.inputmethod.InputConnection? {
        if (outAttrs != null) {
            outAttrs.inputType = android.text.InputType.TYPE_CLASS_TEXT or
                    android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
            outAttrs.imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_ENTER_ACTION or
                    android.view.inputmethod.EditorInfo.IME_ACTION_NONE
            return AndroidInputConnection(this, mirror)
        }
        return null
    }

    override fun onCheckIsTextEditor(): Boolean = true

    fun sendCommandToKernel(commandJson: String) {
        editorView.applyCommand(commandJson)
    }

    fun handleCompositionUpdate(preeditText: String, newCursorPosition: Int) {
        if (!isComposing) {
            compositionReplaceStartUtf8 = mirror.getCursorUtf8()
            compositionReplaceEndUtf8 = compositionReplaceStartUtf8
            isComposing = true
        }
        currentCompositionText = preeditText
        mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
        layoutEngine.requestLayout()
        editorView.invalidate()
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        val committedText = currentCompositionText
        currentCompositionText = ""
        isComposing = false

        val commandJson = """{"kind":"Replace","byte_start":$compositionReplaceStartUtf8,"byte_end_exclusive":$compositionReplaceEndUtf8,"replacement_text":${escapeJson(committedText)},"original_text":"","cause":"TypingCommit"}"""
        sendCommandToKernel(commandJson)
    }

    private fun escapeJson(s: String): String {
        val escaped = s.replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")
        return "\"$escaped\""
    }
}
