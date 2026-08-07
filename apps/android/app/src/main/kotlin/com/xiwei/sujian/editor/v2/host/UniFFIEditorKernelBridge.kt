package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * UniFFI-based implementation of [EditorKernelBridge].
 *
 * Delegates all editor operations to Core's `EditorKernel` via [AppServiceBridge].
 * This bridge uses the legacy single-session path (global editor kernel).
 * New code should prefer [TextEditSessionBridge] which supports multi-target sessions
 * (project name, chapter name, starmap title, etc.) with independent revision/generation.
 *
 * ## Offset convention
 *
 * All byte offset parameters (`byteOffset`, `byteStart`, `byteEndExclusive`, etc.)
 * are UTF-8 byte offsets with half-open interval semantics `[start, end)`.
 * Android's UTF-16 code unit offsets must be converted via [AndroidTextIndexMap]
 * before passing to these methods.
 *
 * ## Thread constraint
 *
 * All methods must be called on the UI thread. The underlying UniFFI calls
 * are synchronous and block the caller.
 */
class UniFFIEditorKernelBridge(
    private val appServiceBridge: AppServiceBridge,
) : EditorKernelBridge {
    override fun insert(
        byteOffset: Int,
        text: String,
        cause: EditorTransactionCauseDto,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelInsert(
                    byteOffset.toUInt(),
                    text,
                    cause,
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun delete(
        byteStart: Int,
        byteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelDelete(
                    byteStart.toUInt(),
                    byteEndExclusive.toUInt(),
                    cause,
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun replace(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelReplace(
                    byteStart.toUInt(),
                    byteEndExclusive.toUInt(),
                    replacementText,
                    originalText,
                    cause,
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun setSelection(
        anchorByteOffset: Int,
        headByteOffset: Int,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelSetSelection(
                    anchorByteOffset.toUInt(),
                    headByteOffset.toUInt(),
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun undo(expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.editorKernelUndo(expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun redo(expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.editorKernelRedo(expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun loadText(
        text: String,
        cursorUtf8: Int,
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.editorKernelLoadText(text, cursorUtf8.toUInt())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun commitText(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        compositionSessionId: Long,
        compositionBaseRevision: Long,
        compositionGeneration: Long,
        cause: EditorTransactionCauseDto,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelCommitText(
                    byteStart.toUInt(),
                    byteEndExclusive.toUInt(),
                    replacementText,
                    resultingSelectionAnchor.toUInt(),
                    resultingSelectionHead.toUInt(),
                    compositionSessionId.toULong(),
                    compositionBaseRevision.toULong(),
                    compositionGeneration.toULong(),
                    cause,
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun deleteSurrounding(
        beforeByteStart: Int,
        beforeByteEndExclusive: Int,
        afterByteStart: Int,
        afterByteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelDeleteSurrounding(
                    beforeByteStart.toUInt(),
                    beforeByteEndExclusive.toUInt(),
                    afterByteStart.toUInt(),
                    afterByteEndExclusive.toUInt(),
                    cause,
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun beginComposition(
        replaceStart: Int,
        replaceEndExclusive: Int,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelBeginComposition(
                    replaceStart.toUInt(),
                    replaceEndExclusive.toUInt(),
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun updateComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        newPreeditText: String,
        newPreeditCursorOffset: Int,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelUpdateComposition(
                    compositionSessionId.toULong(),
                    compositionGeneration.toULong(),
                    newPreeditText,
                    newPreeditCursorOffset.toUInt(),
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun finishComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelFinishComposition(
                    compositionSessionId.toULong(),
                    compositionGeneration.toULong(),
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun cancelComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelCancelComposition(
                    compositionSessionId.toULong(),
                    compositionGeneration.toULong(),
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun setAnimationEnabled(enabled: Boolean) {
        appServiceBridge.editorKernelSetAnimationEnabled(enabled)
    }

    override fun setAnimationDurationMs(durationMs: Long) {
        appServiceBridge.editorKernelSetAnimationDurationMs(durationMs.toULong())
    }

    override fun compositionUpdateVisualIntent(
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String,
    ): uniffi.writer_core.EditorVisualIntentDto? {
        return when (
            val result =
                appServiceBridge.editorKernelCompositionUpdateVisualIntent(
                    compositionReplaceStart,
                    compositionReplaceEndExclusive,
                    oldPreeditText,
                    newPreeditText,
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto? {
        return when (val result = appServiceBridge.editorKernelSessionSnapshot()) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun replaceAll(
        search: String,
        replacement: String,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelReplaceAll(
                    search,
                    replacement,
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun insertLineBreak(
        byteOffset: Int,
        autoIndentPrefix: String,
        cause: EditorTransactionCauseDto,
        expectedRevision: Long,
    ): EditorEditResultDto? {
        return when (
            val result =
                appServiceBridge.editorKernelInsertLineBreak(
                    byteOffset.toUInt(),
                    autoIndentPrefix,
                    cause,
                    expectedRevision.toULong(),
                )
        ) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }
}
