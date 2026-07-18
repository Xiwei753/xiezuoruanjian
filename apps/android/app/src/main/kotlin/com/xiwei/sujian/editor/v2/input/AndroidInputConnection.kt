package com.xiwei.sujian.editor.v2.input

import android.view.View
import android.view.inputmethod.BaseInputConnection
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputConnection(
    private val adapter: AndroidInputAdapter,
    private val mirror: DisplayTextMirror,
    private val pipeline: AndroidEditorPipeline,
    private val hostView: View
) : BaseInputConnection(hostView, true) {

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        val commitStr = text?.toString() ?: ""
        if (adapter.isComposing()) {
            adapter.handleCompositionCommitWithText(commitStr, newCursorPosition)
            notifySelectionChanged()
            return true
        }
        val selStart = mirror.getCommittedSelectionStartUtf8()
        val selEnd = mirror.getCommittedSelectionEndUtf8()
        val byteStart = selStart
        val byteEnd = selEnd
        val originalText = if (byteStart != byteEnd) extractCommittedUtf8Text(byteStart, byteEnd) else ""
        val resultingHead = if (newCursorPosition > 0) {
            byteStart + commitStr.toByteArray(Charsets.UTF_8).size + newCursorPosition - 1
        } else if (newCursorPosition < 0) {
            byteStart + newCursorPosition
        } else {
            byteStart + commitStr.toByteArray(Charsets.UTF_8).size
        }
        adapter.sendCommitTextToKernel(byteStart, byteEnd, commitStr, originalText, resultingHead, resultingHead, EditorTransactionCauseDto.TYPING)
        notifySelectionChanged()
        return true
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength == 0 && afterLength == 0) return true
        if (adapter.isComposing()) {
            return handleCompositionDelete(beforeLength, afterLength)
        }
        val indexMap = AndroidTextIndexMap(mirror)
        val committedCursorUtf8 = mirror.getCommittedCursorUtf8()
        val cursorUtf16 = indexMap.utf8ToUtf16(committedCursorUtf8)
        val selStart = mirror.getCommittedSelectionStartUtf8()
        val selEnd = mirror.getCommittedSelectionEndUtf8()
        val (selMin, selMax) = if (selStart <= selEnd) Pair(selStart, selEnd) else Pair(selEnd, selStart)

        val deleteStartUtf16 = (cursorUtf16 - beforeLength).coerceAtLeast(0)
        val deleteEndUtf16 = (cursorUtf16 + afterLength).coerceAtMost(indexMap.getUtf16Length())

        if (deleteStartUtf16 >= deleteEndUtf16) return false

        val beforeStartUtf8 = indexMap.utf16ToUtf8(deleteStartUtf16)
        val beforeEndUtf8 = indexMap.utf16ToUtf8(cursorUtf16)
        val afterStartUtf8 = indexMap.utf16ToUtf8(cursorUtf16)
        val afterEndUtf8 = indexMap.utf16ToUtf8(deleteEndUtf16)

        val hasBefore = beforeStartUtf8 < beforeEndUtf8 && beforeEndUtf8 <= selMin
        val hasAfter = afterStartUtf8 < afterEndUtf8 && afterStartUtf8 >= selMax

        if (!hasBefore && !hasAfter) return false

        adapter.sendDeleteSurroundingToKernel(
            if (hasBefore) beforeStartUtf8 else 0,
            if (hasBefore) beforeEndUtf8 else 0,
            if (hasAfter) afterStartUtf8 else 0,
            if (hasAfter) afterEndUtf8 else 0,
            EditorTransactionCauseDto.DELETE
        )
        notifySelectionChanged()
        return true
    }

    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength == 0 && afterLength == 0) return true
        if (adapter.isComposing()) {
            return handleCompositionDeleteCodePoints(beforeLength, afterLength)
        }
        val indexMap = AndroidTextIndexMap(mirror)
        val committedCursorUtf8 = mirror.getCommittedCursorUtf8()
        val cursorUtf16 = indexMap.utf8ToUtf16(committedCursorUtf8)
        val fullText = mirror.getText()
        val selStart = mirror.getCommittedSelectionStartUtf8()
        val selEnd = mirror.getCommittedSelectionEndUtf8()
        val (selMin, selMax) = if (selStart <= selEnd) Pair(selStart, selEnd) else Pair(selEnd, selStart)

        var deleteStartUtf16 = cursorUtf16
        var count = beforeLength
        while (count > 0 && deleteStartUtf16 > 0) {
            deleteStartUtf16 = fullText.offsetByCodePoints(deleteStartUtf16, -1)
            count--
        }

        var deleteEndUtf16 = cursorUtf16
        count = afterLength
        while (count > 0 && deleteEndUtf16 < fullText.length) {
            deleteEndUtf16 = fullText.offsetByCodePoints(deleteEndUtf16, 1)
            count--
        }

        if (deleteStartUtf16 >= deleteEndUtf16) return false

        val beforeStartUtf8 = indexMap.utf16ToUtf8(deleteStartUtf16)
        val beforeEndUtf8 = indexMap.utf16ToUtf8(cursorUtf16)
        val afterStartUtf8 = indexMap.utf16ToUtf8(cursorUtf16)
        val afterEndUtf8 = indexMap.utf16ToUtf8(deleteEndUtf16)

        val hasBefore = beforeStartUtf8 < beforeEndUtf8 && beforeEndUtf8 <= selMin
        val hasAfter = afterStartUtf8 < afterEndUtf8 && afterStartUtf8 >= selMax

        if (!hasBefore && !hasAfter) return false

        adapter.sendDeleteSurroundingToKernel(
            if (hasBefore) beforeStartUtf8 else 0,
            if (hasBefore) beforeEndUtf8 else 0,
            if (hasAfter) afterStartUtf8 else 0,
            if (hasAfter) afterEndUtf8 else 0,
            EditorTransactionCauseDto.DELETE
        )
        notifySelectionChanged()
        return true
    }

    private fun handleCompositionDelete(beforeLength: Int, afterLength: Int): Boolean {
        val compositionText = adapter.getCompositionText()
        if (compositionText.isEmpty()) return true
        val compositionRange = adapter.getCompositionRangeUtf8() ?: return false
        val cursorInComposition = adapter.getCompositionCursorOffset() ?: compositionText.length

        val deleteStartInComp = (cursorInComposition - beforeLength).coerceAtLeast(0)
        val deleteEndInComp = (cursorInComposition + afterLength).coerceAtMost(compositionText.length)
        if (deleteStartInComp >= deleteEndInComp) return false

        val newPreedit = compositionText.removeRange(deleteStartInComp, deleteEndInComp)
        if (newPreedit.isEmpty()) {
            adapter.handleCompositionCancel()
        } else {
            val newCursor = deleteStartInComp
            adapter.handleCompositionUpdate(newPreedit, newCursor + 1)
        }
        notifySelectionChanged()
        return true
    }

    private fun handleCompositionDeleteCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        val compositionText = adapter.getCompositionText()
        if (compositionText.isEmpty()) return true
        val cursorInComposition = adapter.getCompositionCursorOffset() ?: compositionText.length

        var deleteStart = cursorInComposition
        var count = beforeLength
        while (count > 0 && deleteStart > 0) {
            deleteStart = compositionText.offsetByCodePoints(deleteStart, -1)
            count--
        }

        var deleteEnd = cursorInComposition
        count = afterLength
        while (count > 0 && deleteEnd < compositionText.length) {
            deleteEnd = compositionText.offsetByCodePoints(deleteEnd, 1)
            count--
        }

        if (deleteStart >= deleteEnd) return false

        val newPreedit = compositionText.removeRange(deleteStart, deleteEnd)
        if (newPreedit.isEmpty()) {
            adapter.handleCompositionCancel()
        } else {
            adapter.handleCompositionUpdate(newPreedit, deleteStart + 1)
        }
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
        mirror.updateComposition(byteStart, byteEnd, selectedText)
        adapter.sendBeginCompositionToKernel(byteStart, byteEnd)
        notifySelectionChanged()
        return true
    }

    override fun setSelection(start: Int, end: Int): Boolean {
        if (start < 0 || end < 0) return false
        val indexMap = AndroidTextIndexMap(mirror)
        val anchorUtf8 = indexMap.utf16ToUtf8(start)
        val headUtf8 = indexMap.utf16ToUtf8(end)
        if (adapter.isComposing()) {
            mirror.setSelectionInternal(anchorUtf8, headUtf8)
        } else {
            adapter.sendSetSelectionToKernel(anchorUtf8, headUtf8)
        }
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
        if (safeStart >= safeEnd) return ""
        return String(textBytes, safeStart, safeEnd - safeStart, Charsets.UTF_8)
    }

    private fun extractCommittedUtf8Text(byteStart: Int, byteEnd: Int): String {
        val committedText = mirror.getCommittedText()
        val textBytes = committedText.toByteArray(Charsets.UTF_8)
        val safeStart = byteStart.coerceIn(0, textBytes.size)
        val safeEnd = byteEnd.coerceIn(safeStart, textBytes.size)
        if (safeStart >= safeEnd) return ""
        return String(textBytes, safeStart, safeEnd - safeStart, Charsets.UTF_8)
    }

    private fun notifySelectionChanged() {
        val imm = hostView.context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager ?: return
        val selStart = mirror.getSelectionStartUtf16()
        val selEnd = mirror.getSelectionEndUtf16()
        val candidatesStart = mirror.getCompositionRangeUtf16()?.first ?: -1
        val candidatesEnd = mirror.getCompositionRangeUtf16()?.second ?: -1
        imm.updateSelection(hostView, selStart, selEnd, candidatesStart, candidatesEnd)
    }
}
