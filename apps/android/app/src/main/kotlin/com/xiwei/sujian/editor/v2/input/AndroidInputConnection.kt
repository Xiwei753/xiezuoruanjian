package com.xiwei.sujian.editor.v2.input

import android.view.inputmethod.BaseInputConnection
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputConnection(
    private val adapter: AndroidInputAdapter,
    private val mirror: DisplayTextMirror,
    private val editorView: com.xiwei.sujian.editor.v2.host.SujianEditorView
) : BaseInputConnection(editorView, true) {

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text.isNullOrEmpty()) return true
        if (adapter.isComposing()) {
            adapter.handleCompositionCommitWithText(text.toString(), newCursorPosition)
            return true
        }
        val selStart = mirror.getSelectionStartUtf8()
        val selEnd = mirror.getSelectionEndUtf8()
        if (selStart != selEnd) {
            val indexMap = AndroidTextIndexMap(mirror)
            val byteStart = selStart
            val byteEnd = selEnd
            val textBytes = mirror.getText().toByteArray(Charsets.UTF_8)
            val originalText = String(textBytes, byteStart.coerceAtMost(textBytes.size)..byteEnd.coerceAtMost(textBytes.size), Charsets.UTF_8)
            adapter.sendReplaceToKernel(byteStart, byteEnd, text.toString(), originalText, EditorTransactionCauseDto.TYPING)
        } else {
            val byteOffset = mirror.getCursorUtf8()
            adapter.sendInsertToKernel(byteOffset, text.toString(), EditorTransactionCauseDto.TYPING)
        }
        applyNewCursorPosition(newCursorPosition, text.length)
        notifySelectionChanged()
        return true
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength == 0 && afterLength == 0) return true
        if (adapter.isComposing()) {
            adapter.handleCompositionFinish()
            return true
        }
        val indexMap = AndroidTextIndexMap(mirror)
        val cursorUtf16 = mirror.getCursorUtf16()
        val deleteStartUtf16 = (cursorUtf16 - beforeLength).coerceAtLeast(0)
        val deleteEndUtf16 = (cursorUtf16 + afterLength).coerceAtMost(mirror.getText().length)
        val byteStart = indexMap.utf16ToUtf8(deleteStartUtf16)
        val byteEnd = indexMap.utf16ToUtf8(deleteEndUtf16)
        adapter.sendDeleteToKernel(byteStart, byteEnd, EditorTransactionCauseDto.DELETE)
        notifySelectionChanged()
        return true
    }

    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        return deleteSurroundingText(beforeLength, afterLength)
    }

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text == null) return true
        adapter.handleCompositionUpdate(text.toString(), newCursorPosition)
        applyNewCursorPosition(newCursorPosition)
        notifySelectionChanged()
        return true
    }

    override fun finishComposingText(): Boolean {
        if (adapter.isComposing()) {
            adapter.handleCompositionFinish()
        }
        notifySelectionChanged()
        return true
    }

    override fun setComposingRegion(start: Int, end: Int): Boolean {
        if (start < 0 || end < 0 || start > end) return false
        if (adapter.isComposing()) {
            adapter.handleCompositionCancel()
        }
        val indexMap = AndroidTextIndexMap(mirror)
        val byteStart = indexMap.utf16ToUtf8(start)
        val byteEnd = indexMap.utf16ToUtf8(end)
        val textBytes = mirror.getText().toByteArray(Charsets.UTF_8)
        val selectedText = String(textBytes, byteStart.coerceAtMost(textBytes.size)..byteEnd.coerceAtMost(textBytes.size), Charsets.UTF_8)
        adapter.startComposingRegion(byteStart, byteEnd, selectedText)
        notifySelectionChanged()
        return true
    }

    override fun setSelection(start: Int, end: Int): Boolean {
        val indexMap = AndroidTextIndexMap(mirror)
        val anchorByte = indexMap.utf16ToUtf8(start)
        val headByte = indexMap.utf16ToUtf8(end)
        adapter.sendSetSelectionToKernel(anchorByte, headByte)
        notifySelectionChanged()
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

    private fun applyNewCursorPosition(newCursorPosition: Int, committedTextLength: Int = 0) {
        if (newCursorPosition == 0 || newCursorPosition == 1) return
        val indexMap = AndroidTextIndexMap(mirror)
        val cursorUtf16 = mirror.getCursorUtf16()
        val newCursorUtf16 = if (newCursorPosition > 1) {
            cursorUtf16 + newCursorPosition - 1
        } else {
            (cursorUtf16 - committedTextLength + newCursorPosition + 1).coerceAtLeast(0)
        }
        if (newCursorUtf16 < 0 || newCursorUtf16 > mirror.getLengthUtf16()) return
        val newCursorUtf8 = indexMap.utf16ToUtf8(newCursorUtf16)
        adapter.sendSetSelectionToKernel(newCursorUtf8, newCursorUtf8)
    }

    private fun notifySelectionChanged() {
        val imm = editorView.context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager ?: return
        val selStart = mirror.getSelectionStartUtf16()
        val selEnd = mirror.getSelectionEndUtf16()
        val candidatesStart = mirror.getCompositionRangeUtf16()?.first ?: -1
        val candidatesEnd = mirror.getCompositionRangeUtf16()?.second ?: -1
        imm.updateSelection(editorView, selStart, selEnd, candidatesStart, candidatesEnd)
    }
}
