package com.xiwei.sujian.ui.span

import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.ui.WriterEditText

class SujianInputConnection(
    target: InputConnection,
    private val editText: WriterEditText
) : InputConnectionWrapper(target, false) {

    private val TAG = "WriterInputConn"

    private var isComposing = false

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text.isNullOrEmpty()) return super.commitText(text, newCursorPosition)
        val pos = editText.selectionStart
        val textLen = editText.text?.length ?: 0
        val wasComposing = isComposing
        DiagnosticsLogger.d(TAG, "commitText: textLen=${text.length}, cursorPos=$newCursorPosition, sel=$pos, editorTextLen=$textLen, wasComposing=$wasComposing")
        editText.onInputBeforeCommit(pos, textLen)
        val result = super.commitText(text, newCursorPosition)
        if (result && wasComposing) {
            isComposing = false
            editText.onInputFinishComposing(fromCommitText = true)
        }
        return result
    }

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        val clearing = text.isNullOrEmpty()
        DiagnosticsLogger.d(TAG, "setComposingText: textLen=${text?.length ?: 0}, cursorPos=$newCursorPosition, clearing=$clearing")
        val result = super.setComposingText(text, newCursorPosition)

        if (clearing) {
            if (isComposing) {
                isComposing = false
                editText.onInputFinishComposing(fromCommitText = false)
            }
        } else {
            isComposing = true
            editText.onInputSetComposingText(text, newCursorPosition)
        }

        return result
    }

    override fun finishComposingText(): Boolean {
        DiagnosticsLogger.d(TAG, "finishComposingText, isComposing=$isComposing")
        if (isComposing) {
            isComposing = false
            editText.onInputFinishComposing(fromCommitText = false)
        }
        return super.finishComposingText()
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        val pos = editText.selectionStart
        val textLen = editText.text?.length ?: 0
        DiagnosticsLogger.d(TAG, "deleteSurroundingText: before=$beforeLength, after=$afterLength, sel=$pos, textLen=$textLen")
        return super.deleteSurroundingText(beforeLength, afterLength)
    }

    override fun sendKeyEvent(event: android.view.KeyEvent): Boolean {
        if (event.action == android.view.KeyEvent.ACTION_DOWN) {
            when (event.keyCode) {
                android.view.KeyEvent.KEYCODE_DEL -> {
                    DiagnosticsLogger.d(TAG, "sendKeyEvent: Backspace")
                }
                android.view.KeyEvent.KEYCODE_FORWARD_DEL -> {
                    DiagnosticsLogger.d(TAG, "sendKeyEvent: ForwardDelete")
                }
            }
        }
        return super.sendKeyEvent(event)
    }
}
