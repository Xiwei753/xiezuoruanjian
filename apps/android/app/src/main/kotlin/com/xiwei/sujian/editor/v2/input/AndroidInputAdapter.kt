package com.xiwei.sujian.editor.v2.input

import android.content.Context
import android.view.View
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.mirror.EditResult
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputAdapter(
    context: Context,
    private val mirror: DisplayTextMirror,
    private val editorView: SujianEditorView
) : View(context) {

    private var currentCompositionText: String = ""
    private var previousCompositionText: String = ""
    private var compositionReplaceStartUtf8: Int = 0
    private var compositionReplaceEndUtf8: Int = 0
    private var isComposing: Boolean = false

    override fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): android.view.inputmethod.InputConnection? {
        if (outAttrs != null) {
            outAttrs.inputType = android.text.InputType.TYPE_CLASS_TEXT or
                    android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
            outAttrs.imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_ENTER_ACTION or
                    android.view.inputmethod.EditorInfo.IME_ACTION_NONE
            return AndroidInputConnection(this, mirror, editorView)
        }
        return null
    }

    override fun onCheckIsTextEditor(): Boolean = true

    fun sendInsertToKernel(byteOffset: Int, text: String, cause: EditorTransactionCauseDto) {
        editorView.insertText(byteOffset, text, cause)
    }

    fun sendDeleteToKernel(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto) {
        editorView.deleteRange(byteStart, byteEndExclusive, cause)
    }

    fun sendReplaceToKernel(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto) {
        editorView.replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause)
    }

    fun sendSetSelectionToKernel(anchorByteOffset: Int, headByteOffset: Int) {
        editorView.setSelectionTyped(anchorByteOffset, headByteOffset)
    }

    fun handleCompositionUpdate(preeditText: String, newCursorPosition: Int) {
        if (!isComposing) {
            val selStart = mirror.getSelectionStartUtf8()
            val selEnd = mirror.getSelectionEndUtf8()
            if (selStart != selEnd) {
                compositionReplaceStartUtf8 = selStart
                compositionReplaceEndUtf8 = selEnd
            } else {
                compositionReplaceStartUtf8 = mirror.getCursorUtf8()
                compositionReplaceEndUtf8 = compositionReplaceStartUtf8
            }
            isComposing = true
        }
        previousCompositionText = currentCompositionText
        currentCompositionText = preeditText

        val bridge = editorView.kernelBridge
        if (bridge != null) {
            val intentDto = bridge.compositionUpdateVisualIntent(
                compositionReplaceStartUtf8.toUInt(),
                compositionReplaceEndUtf8.toUInt(),
                previousCompositionText,
                preeditText,
            )
            if (intentDto != null) {
                val visualIntent = com.xiwei.sujian.editor.v2.mirror.VisualIntent.fromDto(intentDto)
                editorView.applyCompositionUpdate(visualIntent) {
                    mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
                }
                return
            }
        }

        mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
        editorView.onCompositionUpdated()
    }

    fun applyNewCursorPosition(newCursorPosition: Int) {
        // preedit cursor is platform-only visual state; do NOT send SetSelection to Rust
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        val committedText = currentCompositionText
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        val originalText = previousCompositionText
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        val bridge = editorView.kernelBridge
        if (bridge != null) {
            val dto = bridge.compositionCommit(replaceStart, replaceEnd, committedText, originalText)
            if (dto != null) {
                editorView.applyCompositionCommit(dto)
                return
            }
        }

        editorView.clearCompositionAndReplace(replaceStart, replaceEnd, committedText, originalText, EditorTransactionCauseDto.TYPING_COMMIT)
    }

    fun handleCompositionCommitWithText(finalText: String, newCursorPosition: Int) {
        if (!isComposing) return
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        val bridge = editorView.kernelBridge
        if (bridge != null) {
            val dto = bridge.compositionCommit(replaceStart, replaceEnd, finalText, "")
            if (dto != null) {
                editorView.applyCompositionCommit(dto)
                return
            }
        }

        editorView.clearCompositionAndReplace(replaceStart, replaceEnd, finalText, "", EditorTransactionCauseDto.TYPING_COMMIT)
    }

    fun handleCompositionCancel() {
        if (!isComposing) return
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0

        mirror.clearComposition()
        editorView.onCompositionUpdated()
    }

    fun startComposingRegion(byteStart: Int, byteEnd: Int, selectedText: String) {
        compositionReplaceStartUtf8 = byteStart
        compositionReplaceEndUtf8 = byteEnd
        currentCompositionText = selectedText
        previousCompositionText = ""
        isComposing = true
    }

    fun isComposing(): Boolean = isComposing

    fun getCompositionText(): String = currentCompositionText

    fun getCompositionRangeUtf8(): Pair<Int, Int>? {
        if (!isComposing) return null
        return Pair(compositionReplaceStartUtf8, compositionReplaceEndUtf8)
    }
}
