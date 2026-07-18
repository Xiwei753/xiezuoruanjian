package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorSessionSnapshotDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

class TextEditSessionBridge(
    private val appServiceBridge: AppServiceBridge,
    private val sessionId: ULong
) : EditorKernelBridge {

    override fun insert(byteOffset: Int, text: String, cause: EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionInsert(sessionId, byteOffset.toUInt(), text, cause, expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun delete(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionDelete(sessionId, byteStart.toUInt(), byteEndExclusive.toUInt(), cause, expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun replace(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionReplace(sessionId, byteStart.toUInt(), byteEndExclusive.toUInt(), replacementText, originalText, cause, expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun setSelection(anchorByteOffset: Int, headByteOffset: Int, expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionSetSelection(sessionId, anchorByteOffset.toUInt(), headByteOffset.toUInt(), expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun undo(expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionUndo(sessionId, expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun redo(expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionRedo(sessionId, expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun loadText(text: String, cursorUtf8: Int): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionLoadText(sessionId, text, cursorUtf8.toUInt())) {
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
        expectedRevision: Long
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionCommitText(
            sessionId,
            byteStart.toUInt(),
            byteEndExclusive.toUInt(),
            replacementText,
            resultingSelectionAnchor.toUInt(),
            resultingSelectionHead.toUInt(),
            compositionSessionId.toULong(),
            compositionBaseRevision.toULong(),
            compositionGeneration.toULong(),
            cause,
            expectedRevision.toULong()
        )) {
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
        expectedRevision: Long
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionDeleteSurrounding(
            sessionId,
            beforeByteStart.toUInt(),
            beforeByteEndExclusive.toUInt(),
            afterByteStart.toUInt(),
            afterByteEndExclusive.toUInt(),
            cause,
            expectedRevision.toULong()
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun beginComposition(
        replaceStart: Int,
        replaceEndExclusive: Int,
        expectedRevision: Long
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionBeginComposition(
            sessionId,
            replaceStart.toUInt(),
            replaceEndExclusive.toUInt(),
            expectedRevision.toULong()
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun updateComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        newPreeditText: String,
        newPreeditCursorOffset: Int,
        expectedRevision: Long
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionUpdateComposition(
            sessionId,
            compositionSessionId.toULong(),
            compositionGeneration.toULong(),
            newPreeditText,
            newPreeditCursorOffset.toUInt(),
            expectedRevision.toULong()
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun finishComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionFinishComposition(
            sessionId,
            compositionSessionId.toULong(),
            compositionGeneration.toULong(),
            expectedRevision.toULong()
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun cancelComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long
    ): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionCancelComposition(
            sessionId,
            compositionSessionId.toULong(),
            compositionGeneration.toULong(),
            expectedRevision.toULong()
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun setAnimationEnabled(enabled: Boolean) {
        appServiceBridge.textEditSessionSetAnimationEnabled(sessionId, enabled)
    }

    override fun setAnimationDurationMs(durationMs: Long) {
        appServiceBridge.textEditSessionSetAnimationDurationMs(sessionId, durationMs.toULong())
    }

    override fun compositionUpdateVisualIntent(
        compositionReplaceStart: UInt,
        compositionReplaceEndExclusive: UInt,
        oldPreeditText: String,
        newPreeditText: String
    ): EditorVisualIntentDto? {
        return when (val result = appServiceBridge.textEditSessionCompositionUpdateVisualIntent(
            sessionId,
            compositionReplaceStart,
            compositionReplaceEndExclusive,
            oldPreeditText,
            newPreeditText,
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun sessionSnapshot(): EditorSessionSnapshotDto? {
        return when (val result = appServiceBridge.textEditSessionSnapshot(sessionId)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun replaceAll(search: String, replacement: String, expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionReplaceAll(sessionId, search, replacement, expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    override fun insertLineBreak(byteOffset: Int, autoIndentPrefix: String, cause: EditorTransactionCauseDto, expectedRevision: Long): EditorEditResultDto? {
        return when (val result = appServiceBridge.textEditSessionInsertLineBreak(sessionId, byteOffset.toUInt(), autoIndentPrefix, cause, expectedRevision.toULong())) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }
}
