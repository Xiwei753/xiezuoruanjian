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
        ) ?: return
        val output = pipeline.applyCompositionCommit(dto)
        onPipelineOutput?.invoke(output)
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

    fun sendBeginCompositionToKernel(replaceStart: Int, replaceEndExclusive: Int) {
        val bridge = pipeline.kernelBridge ?: return
        val dto = bridge.beginComposition(replaceStart, replaceEndExclusive, mirror.getRevision()) ?: return
        val result = EditResult.fromDto(dto)
        if (result.isApplied()) {
            compositionSessionId = result.transactionId
            compositionBaseRevision = result.baseRevision
            compositionGeneration = 0u
        }
    }

    fun sendUpdateCompositionToKernel(newPreeditText: String, newPreeditCursorOffset: Int) {
        val bridge = pipeline.kernelBridge ?: return
        val (sessionId, baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return
        val dto = bridge.updateComposition(
            sessionId, baseRev, generation,
            newPreeditText, newPreeditCursorOffset,
            mirror.getRevision()
        ) ?: return
        val result = EditResult.fromDto(dto)
        if (result.isApplied()) {
            compositionGeneration++
        }
    }

    fun sendFinishCompositionToKernel() {
        val bridge = pipeline.kernelBridge ?: return
        val (sessionId, baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return
        val dto = bridge.finishComposition(sessionId, baseRev, generation, mirror.getRevision()) ?: return
        val output = pipeline.applyCompositionCommit(dto)
        onPipelineOutput?.invoke(output)
        compositionSessionId = 0L
        compositionBaseRevision = 0L
        compositionGeneration = 0u
    }

    fun sendCancelCompositionToKernel() {
        val bridge = pipeline.kernelBridge ?: return
        val (sessionId, baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return
        bridge.cancelComposition(sessionId, baseRev, generation, mirror.getRevision())
        compositionSessionId = 0L
        compositionBaseRevision = 0L
        compositionGeneration = 0u
    }

    private var compositionSessionId: Long = 0L
    private var compositionBaseRevision: Long = 0L
    private var compositionGeneration: UInt = 0u

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
            isComposing = true
            sendBeginCompositionToKernel(compositionReplaceStartUtf8, compositionReplaceEndUtf8)
        }
        previousCompositionText = currentCompositionText
        currentCompositionText = preeditText

        val bridge = pipeline.kernelBridge
        if (bridge != null) {
            val intentDto = bridge.compositionUpdateVisualIntent(
                compositionReplaceStartUtf8.toUInt(),
                compositionReplaceEndUtf8.toUInt(),
                previousCompositionText,
                preeditText,
            )
            if (intentDto != null) {
                val visualIntent = com.xiwei.sujian.editor.v2.mirror.VisualIntent.fromDto(intentDto)
                pipeline.applyCompositionUpdate(visualIntent) {
                    mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
                }
                applyNewCursorPositionInComposition(newCursorPosition, preeditText)
                sendUpdateCompositionToKernel(preeditText, 0)
                onCompositionVisualUpdate?.invoke()
                return
            }
        }

        mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
        applyNewCursorPositionInComposition(newCursorPosition, preeditText)
        sendUpdateCompositionToKernel(preeditText, 0)
        pipeline.onCompositionUpdated()
        onCompositionVisualUpdate?.invoke()
    }

    private fun applyNewCursorPositionInComposition(newCursorPosition: Int, preeditText: String) {
        val compositionRangeUtf16 = mirror.getCompositionRangeUtf16() ?: return
        val indexMap = AndroidTextIndexMap(mirror)
        val totalUtf16 = indexMap.getUtf16Length()
        val targetUtf16: Int
        if (newCursorPosition > 0) {
            targetUtf16 = (compositionRangeUtf16.second + newCursorPosition - 1).coerceIn(0, totalUtf16)
        } else {
            targetUtf16 = (compositionRangeUtf16.first + newCursorPosition).coerceIn(0, totalUtf16)
        }
        compositionCursorUtf16 = targetUtf16 - compositionRangeUtf16.first
        val targetUtf8 = indexMap.utf16ToUtf8(targetUtf16)
        mirror.setSelectionInternal(targetUtf8, targetUtf8)
    }

    fun applyNewCursorPosition(newCursorPosition: Int, insertStartUtf8: Int, insertedText: String) {
        val indexMap = AndroidTextIndexMap(mirror)
        val insertStartUtf16 = indexMap.utf8ToUtf16(insertStartUtf8)
        val insertEndUtf16 = indexMap.utf8ToUtf16(insertStartUtf8 + insertedText.toByteArray(Charsets.UTF_8).size)
        val totalUtf16 = indexMap.getUtf16Length()

        val targetUtf16: Int
        if (newCursorPosition > 0) {
            targetUtf16 = (insertEndUtf16 + newCursorPosition - 1).coerceIn(0, totalUtf16)
        } else {
            targetUtf16 = (insertStartUtf16 + newCursorPosition).coerceIn(0, totalUtf16)
        }
        val targetUtf8 = indexMap.utf16ToUtf8(targetUtf16)
        if (isComposing) {
            mirror.setSelectionInternal(targetUtf8, targetUtf8)
        } else {
            val output = pipeline.setSelectionTyped(targetUtf8, targetUtf8)
            onPipelineOutput?.invoke(output)
        }
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        val committedText = currentCompositionText
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0
        compositionCursorUtf16 = 0

        val bridge = pipeline.kernelBridge
        if (bridge != null) {
            val (sessionId, baseRev, generation) = compositionSessionInfo()
            if (sessionId != 0L) {
                val dto = bridge.finishComposition(sessionId, baseRev, generation, mirror.getRevision())
                if (dto != null) {
                    val output = pipeline.applyCompositionCommit(dto)
                    onPipelineOutput?.invoke(output)
                    compositionSessionId = 0L
                    compositionBaseRevision = 0L
                    compositionGeneration = 0u
                    return
                }
            }
            val dto = bridge.compositionCommit(replaceStart, replaceEnd, committedText, "")
            if (dto != null) {
                val output = pipeline.applyCompositionCommit(dto)
                onPipelineOutput?.invoke(output)
                compositionSessionId = 0L
                compositionBaseRevision = 0L
                compositionGeneration = 0u
                return
            }
        }

        val output = pipeline.clearCompositionAndReplace(replaceStart, replaceEnd, committedText, "", EditorTransactionCauseDto.TYPING_COMMIT)
        onPipelineOutput?.invoke(output)
        compositionSessionId = 0L
        compositionBaseRevision = 0L
        compositionGeneration = 0u
    }

    fun handleCompositionCommitWithText(finalText: String, newCursorPosition: Int) {
        if (!isComposing) return
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0
        compositionCursorUtf16 = 0

        val resultingHead = if (newCursorPosition > 0) {
            replaceStart + finalText.toByteArray(Charsets.UTF_8).size + newCursorPosition - 1
        } else if (newCursorPosition < 0) {
            replaceStart + newCursorPosition
        } else {
            replaceStart + finalText.toByteArray(Charsets.UTF_8).size
        }

        val bridge = pipeline.kernelBridge
        if (bridge != null) {
            val (sessionId, baseRev, generation) = compositionSessionInfo()
            val dto = bridge.commitText(
                replaceStart, replaceEnd, finalText,
                resultingHead, resultingHead,
                sessionId, baseRev, generation,
                EditorTransactionCauseDto.TYPING_COMMIT, mirror.getRevision()
            )
            if (dto != null) {
                val output = pipeline.applyCompositionCommit(dto)
                onPipelineOutput?.invoke(output)
                compositionSessionId = 0L
                compositionBaseRevision = 0L
                compositionGeneration = 0u
                return
            }
        }

        val output = pipeline.clearCompositionAndReplace(replaceStart, replaceEnd, finalText, "", EditorTransactionCauseDto.TYPING_COMMIT)
        onPipelineOutput?.invoke(output)
        compositionSessionId = 0L
        compositionBaseRevision = 0L
        compositionGeneration = 0u
    }

    fun handleCompositionCancel() {
        if (!isComposing) return
        currentCompositionText = ""
        previousCompositionText = ""
        isComposing = false
        compositionReplaceStartUtf8 = 0
        compositionReplaceEndUtf8 = 0
        compositionCursorUtf16 = 0

        sendCancelCompositionToKernel()

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
