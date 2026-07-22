package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.host.EditorKernelBridge
import uniffi.writer_core.EditorTransactionCauseDto

class EditPipeline(
    private val mirror: DisplayTextMirror
) {
    private var kernelBridge: EditorKernelBridge? = null

    fun setKernelBridge(bridge: EditorKernelBridge?) {
        kernelBridge = bridge
    }

    fun getKernelBridge(): EditorKernelBridge? = kernelBridge

    fun loadText(text: String, cursorUtf8: Int): AndroidEditorPipeline.LoadTextResult {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.LoadTextResult.Failed
        val dto = bridge.loadText(text, cursorUtf8) ?: return AndroidEditorPipeline.LoadTextResult.Failed
        val result = EditResult.fromDto(dto)
        if (result.isStale()) {
            return AndroidEditorPipeline.LoadTextResult.Failed
        }
        if (result.isApplied() || result.isNoChange()) {
            mirror.loadFromSnapshot(text, result.newSelectionEnd, result.newRevision, result.newSelectionStart, result.newSelectionEnd)
            return AndroidEditorPipeline.LoadTextResult.Loaded(result)
        }
        return AndroidEditorPipeline.LoadTextResult.Failed
    }

    fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING): AndroidEditorPipeline.PipelineOutput {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val dto = bridge.insert(byteOffset, text, cause, mirror.getRevision()) ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.DELETE): AndroidEditorPipeline.PipelineOutput {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val dto = bridge.delete(byteStart, byteEndExclusive, cause, mirror.getRevision()) ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun replaceRange(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING): AndroidEditorPipeline.PipelineOutput {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val dto = bridge.replace(byteStart, byteEndExclusive, replacementText, originalText, cause, mirror.getRevision()) ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun setSelection(anchorByteOffset: Int, headByteOffset: Int): AndroidEditorPipeline.PipelineOutput {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val dto = bridge.setSelection(anchorByteOffset, headByteOffset, mirror.getRevision()) ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun undo(): AndroidEditorPipeline.PipelineOutput {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val dto = bridge.undo(mirror.getRevision()) ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun redo(): AndroidEditorPipeline.PipelineOutput {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val dto = bridge.redo(mirror.getRevision()) ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    fun replaceAll(searchStr: String, replaceStr: String): AndroidEditorPipeline.PipelineOutput {
        val bridge = kernelBridge ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val dto = bridge.replaceAll(searchStr, replaceStr, mirror.getRevision()) ?: return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        val result = EditResult.fromDto(dto)
        return applyEditResult(result)
    }

    private fun applyEditResult(result: EditResult): AndroidEditorPipeline.PipelineOutput {
        if (result.isStale()) {
            return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
        }
        if (result.isNoChange()) {
            return AndroidEditorPipeline.PipelineOutput.Edited(result)
        }
        if (result.isApplied()) {
            mirror.applyEditResult(result)
            return AndroidEditorPipeline.PipelineOutput.Edited(result)
        }
        return AndroidEditorPipeline.PipelineOutput.StaleOrInvalid
    }
}
