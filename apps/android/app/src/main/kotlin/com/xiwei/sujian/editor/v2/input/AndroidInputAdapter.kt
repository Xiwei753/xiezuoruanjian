package com.xiwei.sujian.editor.v2.input

import android.view.View
import com.xiwei.sujian.editor.v2.coordinator.ImeAction
import com.xiwei.sujian.editor.v2.coordinator.NewlinePolicy
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.editor.v2.coordinator.TextInputType
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
    var onPerformEditorAction: ((Int) -> Unit)? = null

    private var hostView: View? = null
    private var currentProfile: TextEditorProfile = TextEditorProfile.DocumentBody

    fun setHostView(view: View) {
        hostView = view
    }

    fun getHostView(): View? = hostView

    fun applyProfile(profile: TextEditorProfile) {
        currentProfile = profile
    }

    private var currentCompositionText: String = ""
    private var previousCompositionText: String = ""
    private var compositionReplaceStartUtf8: Int = 0
    private var compositionReplaceEndUtf8: Int = 0
    private var isComposing: Boolean = false
    private var compositionCursorUtf16: Int = 0

    fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): android.view.inputmethod.InputConnection? {
        val host = hostView ?: return null
        if (outAttrs != null) {
            val inputType = when (currentProfile.inputType) {
                TextInputType.NUMBER -> android.text.InputType.TYPE_CLASS_NUMBER
                TextInputType.EMAIL -> android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
                TextInputType.MULTI_LINE -> android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
                TextInputType.TEXT -> android.text.InputType.TYPE_CLASS_TEXT
            }
            if (currentProfile.singleLine) {
                outAttrs.inputType = inputType and android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE.inv()
            } else {
                outAttrs.inputType = inputType
            }

            val imeAction = when (currentProfile.imeAction) {
                ImeAction.DONE -> android.view.inputmethod.EditorInfo.IME_ACTION_DONE
                ImeAction.SEARCH -> android.view.inputmethod.EditorInfo.IME_ACTION_SEARCH
                ImeAction.NEXT -> android.view.inputmethod.EditorInfo.IME_ACTION_NEXT
                ImeAction.GO -> android.view.inputmethod.EditorInfo.IME_ACTION_GO
                ImeAction.NONE -> android.view.inputmethod.EditorInfo.IME_ACTION_NONE
            }
            outAttrs.imeOptions = imeAction
            if (currentProfile.singleLine) {
                outAttrs.imeOptions = outAttrs.imeOptions or android.view.inputmethod.EditorInfo.IME_FLAG_NO_ENTER_ACTION
            }
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
        invalidateCompositionSession()
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
        if (currentProfile.newlinePolicy == NewlinePolicy.FORBID && preeditText.contains('\n')) {
            return
        }
        if (currentProfile.maxLength > 0) {
            val currentLen = mirror.getCommittedText().toByteArray(Charsets.UTF_8).size
            val preeditLen = preeditText.toByteArray(Charsets.UTF_8).size
            val replaceLen = compositionReplaceEndUtf8 - compositionReplaceStartUtf8
            if (currentLen - replaceLen + preeditLen > currentProfile.maxLength) {
                return
            }
        }
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
        val preeditUtf16Len = AndroidTextIndexMap.countUtf16CodeUnits(preeditText)
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

        val (resultingAnchor, resultingHead) = AndroidTextIndexMap.computeResultingSelectionUtf8(
            mirror.getCommittedText(), newCursorPosition, replaceStart, replaceEnd, finalText
        )

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
