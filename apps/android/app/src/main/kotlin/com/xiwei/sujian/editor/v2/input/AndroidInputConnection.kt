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
            val byteStart = selStart
            val byteEnd = selEnd
            val originalText = extractUtf8Text(byteStart, byteEnd)
            adapter.sendReplaceToKernel(byteStart, byteEnd, text.toString(), originalText, EditorTransactionCauseDto.TYPING)
        } else {
            val byteOffset = mirror.getCursorUtf8()
            adapter.sendInsertToKernel(byteOffset, text.toString(), EditorTransactionCauseDto.TYPING)
        }
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
        if (beforeLength == 0 && afterLength == 0) return true
        if (adapter.isComposing()) {
            adapter.handleCompositionFinish()
            return true
        }
        val text = mirror.getText()
        val cursorUtf16 = mirror.getCursorUtf16()

        var deleteStartUtf16 = cursorUtf16
        var count = beforeLength
        while (count > 0 && deleteStartUtf16 > 0) {
            deleteStartUtf16 = text.offsetByCodePoints(deleteStartUtf16, -1)
            count--
        }

        var deleteEndUtf16 = cursorUtf16
        count = afterLength
        while (count > 0 && deleteEndUtf16 < text.length) {
            deleteEndUtf16 = text.offsetByCodePoints(deleteEndUtf16, 1)
            count--
        }

        val indexMap = AndroidTextIndexMap(mirror)
        val byteStart = indexMap.utf16ToUtf8(deleteStartUtf16)
        val byteEnd = indexMap.utf16ToUtf8(deleteEndUtf16)
        adapter.sendDeleteToKernel(byteStart, byteEnd, EditorTransactionCauseDto.DELETE)
        notifySelectionChanged()
        return true
    }

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text == null) return true
        adapter.handleCompositionUpdate(text.toString(), newCursorPosition)
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
        val selectedText = extractUtf8Text(byteStart, byteEnd)
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

    private fun extractUtf8Text(byteStart: Int, byteEnd: Int): String {
        val textBytes = mirror.getText().toByteArray(Charsets.UTF_8)
        val safeStart = byteStart.coerceIn(0, textBytes.size)
        val safeEnd = byteEnd.coerceIn(safeStart, textBytes.size)
        return String(textBytes, safeStart, safeEnd - safeStart, Charsets.UTF_8)
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
