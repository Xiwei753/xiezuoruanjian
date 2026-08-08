package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.host.EditorKernelBridge
import com.xiwei.sujian.feature.editor.mirror.DisplayTextMirror
import com.xiwei.sujian.feature.editor.mirror.EditResult
import uniffi.writer_core.EditorTransactionCauseDto

class EditPipeline(
    val mirror: DisplayTextMirror,
) {
    var kernelBridge: EditorKernelBridge? = null
        private set

    fun setKernelBridge(bridge: EditorKernelBridge?) {
        kernelBridge = bridge
    }

    fun loadText(
        text: String,
        cursorUtf8: Int,
    ): AndroidEditorPipeline.LoadTextResult {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.LoadTextResult.Failed
        val dto = bridge.loadText(text, cursorUtf8) ?: return AndroidEditorPipeline.LoadTextResult.Failed
        val result = EditResult.fromDto(dto)
        if (result.isStale()) {
            return AndroidEditorPipeline.LoadTextResult.Failed
        }
        if (result.isApplied() || result.isNoChange()) {
            mirror.loadFromSnapshot(
                text,
                result.newSelectionEnd,
                result.newRevision,
                result.newSelectionStart,
                result.newSelectionEnd,
            )
            return AndroidEditorPipeline.LoadTextResult.Loaded(result)
        }
        return AndroidEditorPipeline.LoadTextResult.Failed
    }

    fun insertText(
        byteOffset: Int,
        text: String,
        cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING,
    ): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.insert(byteOffset, text, cause, mirror.getRevision()) ?: return null
        return EditResult.fromDto(dto)
    }

    fun insertLineBreak(
        byteOffset: Int,
        indentPrefix: String,
        cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING,
    ): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.insertLineBreak(byteOffset, indentPrefix, cause, mirror.getRevision()) ?: return null
        return EditResult.fromDto(dto)
    }

    fun deleteRange(
        byteStart: Int,
        byteEndExclusive: Int,
        cause: EditorTransactionCauseDto = EditorTransactionCauseDto.DELETE,
    ): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.delete(byteStart, byteEndExclusive, cause, mirror.getRevision()) ?: return null
        return EditResult.fromDto(dto)
    }

    fun replaceRange(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING,
    ): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto =
            bridge.replace(byteStart, byteEndExclusive, replacementText, originalText, cause, mirror.getRevision())
                ?: return null
        return EditResult.fromDto(dto)
    }

    fun setSelection(
        anchorByteOffset: Int,
        headByteOffset: Int,
    ): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.setSelection(anchorByteOffset, headByteOffset, mirror.getRevision()) ?: return null
        return EditResult.fromDto(dto)
    }

    fun undo(): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.undo(mirror.getRevision()) ?: return null
        return EditResult.fromDto(dto)
    }

    fun redo(): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.redo(mirror.getRevision()) ?: return null
        return EditResult.fromDto(dto)
    }

    fun replaceAll(
        searchStr: String,
        replaceStr: String,
    ): EditResult? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.replaceAll(searchStr, replaceStr, mirror.getRevision()) ?: return null
        return EditResult.fromDto(dto)
    }

    fun applyEditResult(result: EditResult) {
        mirror.applyEditResult(result)
    }

    fun loadFromSnapshot(
        text: String,
        cursorUtf8: Int,
        revision: Long,
        selStartUtf8: Int,
        selEndUtf8: Int,
    ) {
        mirror.loadFromSnapshot(text, cursorUtf8, revision, selStartUtf8, selEndUtf8)
    }

    fun reloadFromKernel(): Boolean {
        val bridge = kernelBridge ?: return false
        val snapshot = bridge.sessionSnapshot() ?: return false
        val cursorUtf8 = snapshot.cursor.toInt()
        val selAnchorUtf8 = snapshot.selectionAnchor.toInt()
        val selHeadUtf8 = cursorUtf8
        mirror.loadFromSnapshot(
            snapshot.text,
            cursorUtf8,
            snapshot.revision.toLong(),
            selAnchorUtf8,
            selHeadUtf8,
        )
        return true
    }

    fun getText(): String = mirror.getText()

    fun getRevision(): Long = mirror.getRevision()

    fun getCursorUtf8(): Int = mirror.getCursorUtf8()

    fun getCursorUtf16(): Int = mirror.getCursorUtf16()

    fun getSelectionStartUtf8(): Int = mirror.getSelectionStartUtf8()

    fun getSelectionEndUtf8(): Int = mirror.getSelectionEndUtf8()

    fun getSelectionStartUtf16(): Int = mirror.getSelectionStartUtf16()

    fun getSelectionEndUtf16(): Int = mirror.getSelectionEndUtf16()

    fun getLengthUtf16(): Int = mirror.getLengthUtf16()

    fun getCommittedCursorUtf8(): Int = mirror.getCommittedCursorUtf8()

    fun getCommittedSelectionStartUtf8(): Int = mirror.getCommittedSelectionStartUtf8()

    fun getCommittedSelectionEndUtf8(): Int = mirror.getCommittedSelectionEndUtf8()

    fun getCommittedText(): String = mirror.getCommittedText()

    fun getSpannable(): android.text.SpannableStringBuilder = mirror.getSpannable()
}
