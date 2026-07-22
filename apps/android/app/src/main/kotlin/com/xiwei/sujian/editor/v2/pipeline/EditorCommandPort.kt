package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.host.EditorKernelBridge
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorTransactionCauseDto

interface EditorCommandPort {
    fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto): PipelineOutput
    fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto): PipelineOutput
    fun replaceRangeTyped(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto, beforePatch: (() -> Unit)? = null): PipelineOutput
    fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int): PipelineOutput
    fun replaceAll(searchStr: String, replaceStr: String): PipelineOutput
    fun applyEditResult(result: EditResult, beforePatch: (() -> Unit)? = null): PipelineOutput
    fun applyCompositionCommit(dto: EditorEditResultDto, preeditText: String): PipelineOutput
    fun applyCompositionUpdateAnimated(replaceStartUtf8: Int, replaceEndUtf8: Int, newPreeditText: String, oldPreeditText: String, mirrorUpdate: (() -> Unit)? = null)
    fun applyCompositionCancelAnimated(replaceStartUtf8: Int, replaceEndUtf8: Int, oldPreeditText: String, mirrorUpdate: (() -> Unit)? = null)
    fun onCompositionUpdated()
    fun reloadFromKernel(): Boolean
    fun getCursorUtf8(): Int
    fun getRevision(): Long
    fun getText(): String
    val kernelBridge: EditorKernelBridge?
    val mirror: DisplayTextMirror
}
