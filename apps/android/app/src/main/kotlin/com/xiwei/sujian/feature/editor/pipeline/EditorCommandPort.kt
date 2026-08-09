package com.xiwei.sujian.feature.editor.pipeline

import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.EditResult
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorTransactionCauseDto

interface InputCommandPort {
    fun insertText(
        byteOffset: Int,
        text: String,
        cause: EditorTransactionCauseDto,
    ): PipelineOutput

    fun deleteRange(
        byteStart: Int,
        byteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
    ): PipelineOutput

    fun replaceRangeTyped(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto,
        beforePatch: (() -> Unit)?,
        source: EditorEditSource,
    ): PipelineOutput

    fun setSelectionTyped(
        anchorByteOffset: Int,
        headByteOffset: Int,
        source: EditorEditSource,
    ): PipelineOutput

    fun applyEditResult(
        result: EditResult,
        beforePatch: (() -> Unit)?,
        source: EditorEditSource,
    ): PipelineOutput

    fun applyCompositionCommit(
        dto: EditorEditResultDto,
        preeditText: String,
    ): PipelineOutput

    fun applyCompositionUpdateAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        newPreeditText: String,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)?,
    )

    fun applyCompositionCancelAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)?,
    )

    fun onCompositionUpdated()

    fun reloadFromKernel(): Boolean

    fun getCursorUtf8(): Int

    fun getRevision(): Long

    fun getText(): String

    fun commitComposition(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        compositionSessionId: Long,
        compositionBaseRevision: Long,
        compositionGeneration: Long,
        cause: EditorTransactionCauseDto,
    ): EditorEditResultDto?

    fun deleteSurrounding(
        beforeByteStart: Int,
        beforeByteEndExclusive: Int,
        afterByteStart: Int,
        afterByteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
    ): EditorEditResultDto?

    fun beginComposition(
        replaceStart: Int,
        replaceEndExclusive: Int,
    ): EditorEditResultDto?

    fun updateComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        newPreeditText: String,
        newPreeditCursorOffset: Int,
    ): EditorEditResultDto?

    fun finishComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
    ): EditorEditResultDto?

    fun cancelComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
    ): EditorEditResultDto?

    val mirror: DisplayTextMirror
}

interface EditorCommandPort {
    fun insertText(
        byteOffset: Int,
        text: String,
        cause: EditorTransactionCauseDto,
    ): PipelineOutput

    fun deleteRange(
        byteStart: Int,
        byteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
    ): PipelineOutput

    fun replaceRangeTyped(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto,
        beforePatch: (() -> Unit)? = null,
        source: EditorEditSource = EditorEditSource.NORMAL,
    ): PipelineOutput

    fun setSelectionTyped(
        anchorByteOffset: Int,
        headByteOffset: Int,
        source: EditorEditSource = EditorEditSource.NORMAL,
    ): PipelineOutput

    fun applyEditResult(
        result: EditResult,
        beforePatch: (() -> Unit)? = null,
        source: EditorEditSource = EditorEditSource.NORMAL,
    ): PipelineOutput

    fun applyCompositionCommit(
        dto: EditorEditResultDto,
        preeditText: String,
    ): PipelineOutput

    fun applyCompositionUpdateAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        newPreeditText: String,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)? = null,
    )

    fun applyCompositionCancelAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)? = null,
    )

    fun onCompositionUpdated()

    fun reloadFromKernel(): Boolean

    fun getCursorUtf8(): Int

    fun getRevision(): Long

    fun getText(): String

    val kernelBridge: EditorKernelBridge?
    val mirror: DisplayTextMirror
}
