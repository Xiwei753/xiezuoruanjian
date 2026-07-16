package com.xiwei.sujian.editor.v2.input

import android.view.inputmethod.BaseInputConnection
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class AndroidInputConnection(
    private val view: AndroidInputAdapter,
    private val mirror: DisplayTextMirror
) : BaseInputConnection(view, true) {

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text.isNullOrEmpty()) return true
        val commandJson = buildInsertCommand(text.toString())
        view.sendCommandToKernel(commandJson)
        return true
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength == 0 && afterLength == 0) return true
        val commandJson = buildDeleteCommand(beforeLength, afterLength)
        view.sendCommandToKernel(commandJson)
        return true
    }

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        if (text == null) return true
        view.handleCompositionUpdate(text.toString(), newCursorPosition)
        return true
    }

    override fun finishComposingText(): Boolean {
        view.handleCompositionFinish()
        return true
    }

    override fun setSelection(start: Int, end: Int): Boolean {
        val commandJson = buildSetSelectionCommand(start, end)
        view.sendCommandToKernel(commandJson)
        return true
    }

    override fun getTextBeforeCursor(n: Int, flags: Int): CharSequence {
        val cursorUtf16 = mirror.getCursorUtf16()
        val start = (cursorUtf16 - n).coerceAtLeast(0)
        return mirror.getText().substring(start, cursorUtf16.coerceAtMost(mirror.getText().length))
    }

    override fun getTextAfterCursor(n: Int, flags: Int): CharSequence {
        val cursorUtf16 = mirror.getCursorUtf16()
        val end = (cursorUtf16 + n).coerceAtMost(mirror.getText().length)
        return mirror.getText().substring(cursorUtf16.coerceAtMost(mirror.getText().length), end)
    }

    private fun buildInsertCommand(text: String): String {
        val byteOffset = mirror.getCursorUtf8()
        return """{"kind":"Insert","byte_offset":$byteOffset,"text":${escapeJson(text)},"cause":"Typing"}"""
    }

    private fun buildDeleteCommand(beforeLength: Int, afterLength: Int): String {
        val indexMap = AndroidTextIndexMap(mirror)
        val cursorUtf16 = mirror.getCursorUtf16()
        val deleteStartUtf16 = (cursorUtf16 - beforeLength).coerceAtLeast(0)
        val deleteEndUtf16 = (cursorUtf16 + afterLength).coerceAtMost(mirror.getText().length)
        val byteStart = indexMap.utf16ToUtf8(deleteStartUtf16)
        val byteEnd = indexMap.utf16ToUtf8(deleteEndUtf16)
        val deletedText = mirror.getText().substring(deleteStartUtf16, deleteEndUtf16)
        return """{"kind":"Delete","byte_start":$byteStart,"byte_end_exclusive":$byteEnd,"deleted_text":${escapeJson(deletedText)},"cause":"Delete"}"""
    }

    private fun buildSetSelectionCommand(anchorUtf16: Int, headUtf16: Int): String {
        val indexMap = AndroidTextIndexMap(mirror)
        val anchorByte = indexMap.utf16ToUtf8(anchorUtf16)
        val headByte = indexMap.utf16ToUtf8(headUtf16)
        return """{"kind":"SetSelection","anchor_byte_offset":$anchorByte,"head_byte_offset":$headByte}"""
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
