package com.xiwei.sujian.editor.v2.input

import android.content.Context
import android.view.View
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.host.SujianEditorView

class AndroidInputAdapter(
    context: Context,
    private val mirror: DisplayTextMirror,
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
        editorView.onCompositionUpdated()
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        val committedText = currentCompositionText
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        currentCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        mirror.clearComposition()

        val commandJson = """{"kind":"Replace","byte_start":$replaceStart,"byte_end_exclusive":$replaceEnd,"replacement_text":${escapeJson(committedText)},"original_text":"","cause":"TypingCommit"}"""
        sendCommandToKernel(commandJson)
    }

    fun handleCompositionCancel() {
        if (!isComposing) return
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        currentCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        mirror.clearComposition()

        if (replaceStart < replaceEnd) {
            val commandJson = """{"kind":"Delete","byte_start":$replaceStart,"byte_end_exclusive":$replaceEnd,"deleted_text":"","cause":"Delete"}"""
            sendCommandToKernel(commandJson)
        }
    }

    fun isComposing(): Boolean = isComposing

    fun getCompositionText(): String = currentCompositionText

    private fun escapeJson(s: String): String {
        val escaped = s.replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")
        return "\"$escaped\""
    }
}
