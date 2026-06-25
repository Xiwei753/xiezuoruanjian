package com.xiwei.sujian.ui.span

import android.util.Log
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper
import com.xiwei.sujian.ui.WriterEditText

class SujianInputConnection(
    target: InputConnection,
    private val editText: WriterEditText
) : InputConnectionWrapper(target, false) {

    private val TAG = "WriterInputConn"

    private var isComposing = false

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text.isNullOrEmpty()) return super.commitText(text, newCursorPosition)
        Log.d(TAG, "commitText: \"$text\", cursorPos=$newCursorPosition")
        editText.onInputCommitText(text, newCursorPosition)
        return super.commitText(text, newCursorPosition)
    }

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        Log.d(TAG, "setComposingText: \"$text\", cursorPos=$newCursorPosition")
        isComposing = true
        editText.onInputSetComposingText(text, newCursorPosition)
        return super.setComposingText(text, newCursorPosition)
    }

    override fun finishComposingText(): Boolean {
        Log.d(TAG, "finishComposingText")
        if (isComposing) {
            isComposing = false
            editText.onInputFinishComposing()
        }
        return super.finishComposingText()
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        Log.d(TAG, "deleteSurroundingText: before=$beforeLength, after=$afterLength")
        if (beforeLength > 0) {
            editText.onInputDeleteChar(beforeLength)
        }
        return super.deleteSurroundingText(beforeLength, afterLength)
    }

    override fun sendKeyEvent(event: android.view.KeyEvent): Boolean {
        if (event.action == android.view.KeyEvent.ACTION_DOWN) {
            when (event.keyCode) {
                android.view.KeyEvent.KEYCODE_DEL -> {
                    Log.d(TAG, "sendKeyEvent: Backspace")
                    editText.onInputBackspaceKeyEvent()
                }
                android.view.KeyEvent.KEYCODE_FORWARD_DEL -> {
                    Log.d(TAG, "sendKeyEvent: ForwardDelete")
                }
            }
        }
        return super.sendKeyEvent(event)
    }
}
