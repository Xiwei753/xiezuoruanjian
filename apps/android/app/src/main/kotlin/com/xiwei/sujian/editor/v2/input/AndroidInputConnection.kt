package com.xiwei.sujian.editor.v2.input

import android.view.inputmethod.BaseInputConnection
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputConnection(
    private val adapter: AndroidInputAdapter,
    private val mirror: DisplayTextMirror
) : BaseInputConnection(adapter, true) {

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text.isNullOrEmpty()) return true
        if (adapter.isComposing()) {
            adapter.handleCompositionFinish()
        }
        val byteOffset = mirror.getCursorUtf8()
        adapter.sendInsertToKernel(byteOffset, text.toString(), EditorTransactionCauseDto.TYPING)
        return true
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength == 0 && afterLength == 0) return true
        if (adapter.isComposing()) {
            adapter.handleCompositionFinish()
        }
        val indexMap = AndroidTextIndexMap(mirror)
        val cursorUtf16 = mirror.getCursorUtf16()
        val deleteStartUtf16 = (cursorUtf16 - beforeLength).coerceAtLeast(0)
        val deleteEndUtf16 = (cursorUtf16 + afterLength).coerceAtMost(mirror.getText().length)
        val byteStart = indexMap.utf16ToUtf8(deleteStartUtf16)
        val byteEnd = indexMap.utf16ToUtf8(deleteEndUtf16)
        adapter.sendDeleteToKernel(byteStart, byteEnd, EditorTransactionCauseDto.DELETE)
        return true
    }

    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        return deleteSurroundingText(beforeLength, afterLength)
    }

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text == null) return true
        adapter.handleCompositionUpdate(text.toString(), newCursorPosition)
        return true
    }

    override fun finishComposingText(): Boolean {
        adapter.handleCompositionFinish()
        return true
    }

    override fun setSelection(start: Int, end: Int): Boolean {
        val indexMap = AndroidTextIndexMap(mirror)
        val anchorByte = indexMap.utf16ToUtf8(start)
        val headByte = indexMap.utf16ToUtf8(end)
        adapter.sendSetSelectionToKernel(anchorByte, headByte)
        return true
    }

    override fun getTextBeforeCursor(n: Int, flags: Int): CharSequence {
        val cursorUtf16 = mirror.getCursorUtf16()
        val start = (cursorUtf16 - n).coerceAtLeast(0)
        val text = mirror.getText()
        return text.substring(start, cursorUtf16.coerceAtMost(text.length))
    }

    override fun getTextAfterCursor(n: Int, flags: Int): CharSequence {
        val cursorUtf16 = mirror.getCursorUtf16()
        val text = mirror.getText()
        val end = (cursorUtf16 + n).coerceAtMost(text.length)
        return text.substring(cursorUtf16.coerceAtMost(text.length), end)
    }

    override fun getSelectedText(flags: Int): CharSequence? {
        val selStart = mirror.getSelectionStartUtf16()
        val selEnd = mirror.getSelectionEndUtf16()
        if (selStart < 0 || selEnd < 0 || selStart == selEnd) return null
        val text = mirror.getText()
        val start = selStart.coerceAtMost(text.length)
        val end = selEnd.coerceAtMost(text.length)
        return text.substring(start.coerceAtMost(end), end.coerceAtLeast(start))
    }
}
