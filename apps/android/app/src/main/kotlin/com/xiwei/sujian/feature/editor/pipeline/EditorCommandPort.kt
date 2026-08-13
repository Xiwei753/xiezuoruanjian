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

    /**
     * #624 评论2：统一的换行命令 — 软键盘 commitText("\n")、硬件 Enter、
     * 粘贴换行全部收敛到这一个入口。无选区时走 Core insertLineBreak（继承
     * auto-indent 策略）；有选区时也必须通过 Core 的单一“换行替换”语义完成
     * （一次 replace 命令把选区换成 \n），不能在平台端先删选区再插入换行。
     */
    fun insertLineBreak(cause: EditorTransactionCauseDto): PipelineOutput

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

    fun applyCompositionCommit(dto: EditorEditResultDto): PipelineOutput

    fun applyCompositionUpdateAnimated(mirrorUpdate: (() -> Unit)?)

    fun applyCompositionCancelAnimated(mirrorUpdate: (() -> Unit)?)

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

    /** 见 [InputCommandPort.insertLineBreak] — 唯一换行命令入口。 */
    fun insertLineBreak(cause: EditorTransactionCauseDto): PipelineOutput

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

    fun applyCompositionCommit(dto: EditorEditResultDto): PipelineOutput

    fun applyCompositionUpdateAnimated(mirrorUpdate: (() -> Unit)? = null)

    fun applyCompositionCancelAnimated(mirrorUpdate: (() -> Unit)? = null)

    fun reloadFromKernel(): Boolean

    fun getCursorUtf8(): Int

    fun getRevision(): Long

    fun getText(): String

    val kernelBridge: EditorKernelBridge?
    val mirror: DisplayTextMirror
}
