package com.xiwei.sujian.editor.v2.input

import android.view.View
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline
import com.xiwei.sujian.editor.v2.mirror.EditResult
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputAdapter(
    private val mirror: DisplayTextMirror,
    private val pipeline: AndroidEditorPipeline
) {

    var onPipelineOutput: ((AndroidEditorPipeline.PipelineOutput) -> Unit)? = null
    var onCompositionVisualUpdate: (() -> Unit)? = null

    private var hostView: View? = null

    fun setHostView(view: View) {
        hostView = view
    }

    fun getHostView(): View? = hostView

    private var currentCompositionText: String = ""
    private var previousCompositionText: String = ""
    private var compositionReplaceStartUtf8: Int = 0
    private var compositionReplaceEndUtf8: Int = 0
    private var isComposing: Boolean = false
    private var compositionCursorUtf16: Int = 0

    fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): android.view.inputmethod.InputConnection? {
        val host = hostView ?: return null
        if (outAttrs != null) {
            outAttrs.inputType = android.text.InputType.TYPE_CLASS_TEXT or
                    android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
            outAttrs.imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_ENTER_ACTION or
                    android.view.inputmethod.EditorInfo.IME_ACTION_NONE
            return AndroidInputConnection(this, mirror, pipeline, host)
        }
        return null
    }

    fun sendInsertToKernel(byteOffset: Int, text: String, cause: EditorTransactionCauseDto) {
        val output = pipeline.insertText(byteOffset, text, cause)
        onPipelineOutput?.invoke(output)
    }

    fun sendDeleteToKernel(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto) {
        val output = pipeline.deleteRange(byteStart, byteEndExclusive, cause)
        onPipelineOutput?.invoke(output)
    }

    fun sendReplaceToKernel(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto) {
        val output = pipeline.replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause)
        onPipelineOutput?.invoke(output)
    }

    fun sendSetSelectionToKernel(anchorByteOffset: Int, headByteOffset: Int) {
        val output = pipeline.setSelectionTyped(anchorByteOffset, headByteOffset)
        onPipelineOutput?.invoke(output)
    }

    fun sendCommitTextToKernel(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, resultingSelectionAnchor: Int, resultingSelectionHead: Int, cause: EditorTransactionCauseDto) {
        val bridge = pipeline.kernelBridge ?: return
        val (sessionId, baseRev, generation) = compositionSessionInfo()
        val dto = bridge.commitText(
            byteStart, byteEndExclusive, replacementText,
            resultingSelectionAnchor, resultingSelectionHead,
            sessionId, baseRev, generation,
            cause, mirror.getRevision()
        )
        val result = dto?.let { EditResult.fromDto(it) }
        if (result != null && result.isApplied()) {
            clearCompositionState()
            val output = pipeline.applyCompositionCommit(dto)
            onPipelineOutput?.invoke(output)
            return
        }
        clearCompositionState()
        pipeline.reloadFromKernel()
    }

    fun sendDeleteSurroundingToKernel(beforeByteStart: Int, beforeByteEndExclusive: Int, afterByteStart: Int, afterByteEndExclusive: Int, cause: EditorTransactionCauseDto) {
        val bridge = pipeline.kernelBridge ?: return
        val dto = bridge.deleteSurrounding(
            beforeByteStart, beforeByteEndExclusive,
            afterByteStart, afterByteEndExclusive,
            cause, mirror.getRevision()
        ) ?: return
        val result = EditResult.fromDto(dto)
        val output = pipeline.applyEditResult(result)
        onPipelineOutput?.invoke(output)
    }

    fun sendBeginCompositionToKernel(replaceStart: Int, replaceEndExclusive: Int): Boolean {
        val bridge = pipeline.kernelBridge ?: return false
        val dto = bridge.beginComposition(replaceStart, replaceEndExclusive, mirror.getRevision()) ?: return false
        val result = EditResult.fromDto(dto)
        if (result.isApplied()) {
            val sessionDto = dto.compositionSession
            if (sessionDto != null) {
                compositionSessionId = sessionDto.sessionId.toLong()
                compositionBaseRevision = sessionDto.baseRevision.toLong()
                compositionGeneration = sessionDto.generation.toLong().toUInt()
                return true
            }
        }
        return false
    }

    fun sendUpdateCompositionToKernel(newPreeditText: String, newPreeditCursorOffset: Int): Boolean {
        val bridge = pipeline.kernelBridge ?: return false
        val (sessionId, _baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return false
        val dto = bridge.updateComposition(
            sessionId, generation,
            newPreeditText, newPreeditCursorOffset,
            mirror.getRevision()
        ) ?: return false
        val result = EditResult.fromDto(dto)
        if (result.isApplied()) {
            compositionGeneration++
            return true
        }
        return false
    }

    fun sendFinishCompositionToKernel() {
        val bridge = pipeline.kernelBridge ?: return
        val (sessionId, _baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return
        val dto = bridge.finishComposition(sessionId, generation, mirror.getRevision())
        if (dto != null) {
            val result = EditResult.fromDto(dto)
            if (result.isApplied()) {
                clearCompositionState()
                val output = pipeline.applyCompositionCommit(dto)
                onPipelineOutput?.invoke(output)
                return
            }
        }
        clearCompositionState()
        pipeline.reloadFromKernel()
    }

    fun sendCancelCompositionToKernel(): Boolean {
        val bridge = pipeline.kernelBridge ?: return false
        val (sessionId, _baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return false
        val dto = bridge.cancelComposition(sessionId, generation, mirror.getRevision())
        if (dto == null) {
            return false
        }
        val result = EditResult.fromDto(dto)
        if (!result.isApplied()) {
            return false
        }
        return true
    }

    private var compositionSessionId: Long = 0L
    private var compositionBaseRevision: Long = 0L
    private var compositionGeneration: UInt = 0u

    private fun clearCompositionState() {
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0
        compositionCursorUtf16 = 0
        compositionSessionId = 0L
        compositionBaseRevision = 0L
        compositionGeneration = 0u
    }

    fun invalidateCompositionSession() {
        if (isComposing) {
            mirror.clearComposition()
            clearCompositionState()
            onCompositionVisualUpdate?.invoke()
        }
    }

    fun syncCompositionGeneration() {
        compositionGeneration++
    }

    fun compositionSessionInfo(): Triple<Long, Long, Long> {
        return Triple(compositionSessionId, compositionBaseRevision, compositionGeneration.toLong())
    }

    fun handleCompositionUpdate(preeditText: String, newCursorPosition: Int) {
        if (!isComposing) {
            val selStart = mirror.getCommittedSelectionStartUtf8()
            val selEnd = mirror.getCommittedSelectionEndUtf8()
            if (selStart != selEnd) {
                compositionReplaceStartUtf8 = selStart
                compositionReplaceEndUtf8 = selEnd
            } else {
                compositionReplaceStartUtf8 = mirror.getCommittedCursorUtf8()
                compositionReplaceEndUtf8 = compositionReplaceStartUtf8
            }
            val beginOk = sendBeginCompositionToKernel(compositionReplaceStartUtf8, compositionReplaceEndUtf8)
            if (!beginOk) {
                pipeline.reloadFromKernel()
                return
            }
            isComposing = true
        }
        previousCompositionText = currentCompositionText
        currentCompositionText = preeditText
        val preeditUtf16Len = countUtf16CodeUnits(preeditText)
        compositionCursorUtf16 = if (newCursorPosition > 0) {
            (preeditUtf16Len + newCursorPosition - 1).coerceIn(0, preeditUtf16Len)
        } else {
            (0 + newCursorPosition).coerceIn(0, preeditUtf16Len)
        }

        val bridge = pipeline.kernelBridge
        if (bridge != null) {
            val updateOk = sendUpdateCompositionToKernel(preeditText, compositionCursorUtf16)
            if (!updateOk) {
                mirror.clearComposition()
                clearCompositionState()
                pipeline.reloadFromKernel()
                onCompositionVisualUpdate?.invoke()
                return
            }
        }

        mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
        onCompositionVisualUpdate?.invoke()
    }

    fun handleCompositionCommitWithText(finalText: String, newCursorPosition: Int) {
        if (!isComposing) return
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8

        val (resultingAnchor, resultingHead) = computeResultingSelectionUtf8(newCursorPosition, replaceStart, replaceEnd, finalText)

        val bridge = pipeline.kernelBridge
        if (bridge != null) {
            val (sessionId, baseRev, generation) = compositionSessionInfo()
            val dto = bridge.commitText(
                replaceStart, replaceEnd, finalText,
                resultingAnchor, resultingHead,
                sessionId, baseRev, generation,
                EditorTransactionCauseDto.TYPING_COMMIT, mirror.getRevision()
            )
            if (dto != null) {
                val result = EditResult.fromDto(dto)
                if (result.isApplied()) {
                    clearCompositionState()
                    val output = pipeline.applyCompositionCommit(dto)
                    onPipelineOutput?.invoke(output)
                    return
                }
            }
            clearCompositionState()
            pipeline.reloadFromKernel()
            return
        }

        clearCompositionState()
        pipeline.reloadFromKernel()
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        sendFinishCompositionToKernel()
    }

    private fun countUtf16CodeUnits(text: String): Int {
        var count = 0
        var i = 0
        while (i < text.length) {
            val codePoint = text.codePointAt(i)
            count += Character.charCount(codePoint)
            i += Character.charCount(codePoint)
        }
        return count
    }

    private fun computeResultingSelectionUtf8(newCursorPosition: Int, replaceStartUtf8: Int, replaceEndUtf8: Int, replacementText: String): Pair<Int, Int> {
        val committedText = mirror.getCommittedText()
        val committedBytes = committedText.toByteArray(Charsets.UTF_8)
        val safeStart = replaceStartUtf8.coerceIn(0, committedBytes.size)
        val safeEnd = replaceEndUtf8.coerceIn(safeStart, committedBytes.size)
        val virtualText = String(committedBytes, 0, safeStart, Charsets.UTF_8) + replacementText + String(committedBytes, safeEnd, committedBytes.size - safeEnd, Charsets.UTF_8)
        val virtualIndexMap = AndroidTextIndexMap.fromText(virtualText)
        val replaceStartUtf16 = virtualIndexMap.utf8ToUtf16(safeStart)
        val replacementUtf16Len = countUtf16CodeUnits(replacementText)
        val replaceEndUtf16 = replaceStartUtf16 + replacementUtf16Len
        val totalUtf16 = virtualIndexMap.getUtf16Length()

        val targetUtf16: Int
        if (newCursorPosition > 0) {
            targetUtf16 = (replaceEndUtf16 + newCursorPosition - 1).coerceIn(0, totalUtf16)
        } else {
            targetUtf16 = (replaceStartUtf16 + newCursorPosition).coerceIn(0, totalUtf16)
        }
        val targetUtf8 = virtualIndexMap.utf16ToUtf8(targetUtf16)
        return Pair(targetUtf8, targetUtf8)
    }

    fun handleCompositionCancel() {
        if (!isComposing) return

        val cancelOk = sendCancelCompositionToKernel()
        mirror.clearComposition()
        clearCompositionState()

        if (!cancelOk) {
            pipeline.reloadFromKernel()
            onCompositionVisualUpdate?.invoke()
            return
        }

        pipeline.applyCompositionUpdate(
            com.xiwei.sujian.editor.v2.mirror.VisualIntent(
                cause = uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION,
                operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL,
                oldAffectedByteRanges = emptyList(),
                newAffectedByteRanges = emptyList(),
                animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0L,
                coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(0, 0, false)
            )
        ) {
            mirror.clearComposition()
        }
        onCompositionVisualUpdate?.invoke()
    }

    fun startComposingRegion(byteStart: Int, byteEnd: Int, selectedText: String) {
        compositionReplaceStartUtf8 = byteStart
        compositionReplaceEndUtf8 = byteEnd
        currentCompositionText = selectedText
        previousCompositionText = ""
        isComposing = true
        compositionCursorUtf16 = selectedText.length
    }

    fun isComposing(): Boolean = isComposing
    fun getCompositionText(): String = currentCompositionText

    fun getCompositionCursorOffset(): Int? {
        if (!isComposing) return null
        return compositionCursorUtf16
    }

    fun getCompositionRangeUtf8(): Pair<Int, Int>? {
        if (!isComposing) return null
        return Pair(compositionReplaceStartUtf8, compositionReplaceEndUtf8)
    }
}
