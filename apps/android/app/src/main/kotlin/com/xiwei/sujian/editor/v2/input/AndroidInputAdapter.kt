package com.xiwei.sujian.editor.v2.input

import android.view.View
import com.xiwei.sujian.editor.v2.coordinator.AutocorrectPolicy
import com.xiwei.sujian.editor.v2.coordinator.CapitalizationPolicy
import com.xiwei.sujian.editor.v2.coordinator.CopyPolicy
import com.xiwei.sujian.editor.v2.coordinator.ImeAction
import com.xiwei.sujian.editor.v2.coordinator.NewlinePolicy
import com.xiwei.sujian.editor.v2.coordinator.PastePolicy
import com.xiwei.sujian.editor.v2.coordinator.SecretPolicy
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.editor.v2.coordinator.TextInputType
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.pipeline.InputCommandPort
import com.xiwei.sujian.editor.v2.pipeline.PipelineOutput
import com.xiwei.sujian.editor.v2.mirror.EditResult
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputAdapter(
    private val mirror: DisplayTextMirror,
    private val commandPort: InputCommandPort,
    private val projectionProvider: (() -> com.xiwei.sujian.editor.v2.projection.DisplayTextProjection)? = null
) {

    var onPipelineOutput: ((PipelineOutput) -> Unit)? = null
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

    fun getCurrentProfile(): TextEditorProfile = currentProfile

    fun shouldForbidNewline(text: String): Boolean {
        return currentProfile.newlinePolicy == NewlinePolicy.FORBID && text.contains('\n')
    }

    fun wouldExceedMaxLength(newText: String): Boolean {
        if (currentProfile.maxLength <= 0) return false
        val currentLen = mirror.getCommittedText().toByteArray(Charsets.UTF_8).size
        val newLen = newText.toByteArray(Charsets.UTF_8).size
        val selStart = mirror.getCommittedSelectionStartUtf8()
        val selEnd = mirror.getCommittedSelectionEndUtf8()
        val replaceLen = selEnd - selStart
        return currentLen - replaceLen + newLen > currentProfile.maxLength
    }

    private var currentCompositionText: String = ""
    private var previousCompositionText: String = ""
    private var compositionReplaceStartUtf8: Int = 0
    private var compositionReplaceEndUtf8: Int = 0
    private var isComposing: Boolean = false
    private var compositionCursorUtf16: Int = 0

    fun onCreateInputConnection(outAttrs: android.view.inputmethod.EditorInfo?): android.view.inputmethod.InputConnection? {
        val host = hostView ?: return null
        // InputConnection lifecycle hook (Issue #589): composition state is per-InputConnection,
        // but the adapter outlives connections. A fresh onCreateInputConnection means the
        // previous IME binding is over (IME switch, restartInput, focus regain, soft reset),
        // and Android does not guarantee finishComposingText() before it discards a
        // connection. Without this cleanup the adapter would keep an orphan composition:
        // the kernel session stays live and the overlay stays visible while the new
        // connection's IME knows nothing about them, so the first plain commitText would be
        // misrouted into the composition-commit path, rejected by the kernel as
        // STALE_REVISION and dropped (text loss) — the corruption the removed
        // enabledInputMethodList gate used to guard. Cancelling here aligns the adapter
        // state machine with the connection lifecycle; the kernel-side session is closed
        // best-effort too (kernel begin_composition already replaces stale sessions).
        // No IME enumeration or switch is involved.
        invalidateCompositionSession()
        if (outAttrs != null) {
            val inputType = when (currentProfile.inputType) {
                TextInputType.NUMBER -> android.text.InputType.TYPE_CLASS_NUMBER
                TextInputType.EMAIL -> android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
                TextInputType.MULTI_LINE -> android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
                TextInputType.TEXT -> android.text.InputType.TYPE_CLASS_TEXT
                TextInputType.PASSWORD -> android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
            }
            if (currentProfile.singleLine) {
                outAttrs.inputType = inputType and android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE.inv()
            } else {
                outAttrs.inputType = inputType
            }

            if (currentProfile.autocorrectPolicy == AutocorrectPolicy.DISABLED) {
                outAttrs.inputType = outAttrs.inputType and android.text.InputType.TYPE_TEXT_FLAG_AUTO_CORRECT.inv()
            }

            when (currentProfile.capitalizationPolicy) {
                CapitalizationPolicy.CHARACTERS -> outAttrs.inputType = outAttrs.inputType or android.text.InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS
                CapitalizationPolicy.WORDS -> outAttrs.inputType = outAttrs.inputType or android.text.InputType.TYPE_TEXT_FLAG_CAP_WORDS
                CapitalizationPolicy.SENTENCES -> outAttrs.inputType = outAttrs.inputType or android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
                CapitalizationPolicy.NONE -> { }
            }

            val imeAction = when (currentProfile.imeAction) {
                ImeAction.DONE -> android.view.inputmethod.EditorInfo.IME_ACTION_DONE
                ImeAction.SEARCH -> android.view.inputmethod.EditorInfo.IME_ACTION_SEARCH
                ImeAction.NEXT -> android.view.inputmethod.EditorInfo.IME_ACTION_NEXT
                ImeAction.GO -> android.view.inputmethod.EditorInfo.IME_ACTION_GO
                ImeAction.NONE -> android.view.inputmethod.EditorInfo.IME_ACTION_NONE
            }
            outAttrs.imeOptions = imeAction
            if (currentProfile.singleLine && !currentProfile.commitOnImeAction) {
                outAttrs.imeOptions = outAttrs.imeOptions or android.view.inputmethod.EditorInfo.IME_FLAG_NO_ENTER_ACTION
            }
            return AndroidInputConnection(this, mirror, commandPort, host, projectionProvider)
        }
        return null
    }

    fun sendInsertToKernel(byteOffset: Int, text: String, cause: EditorTransactionCauseDto) {
        val output = commandPort.insertText(byteOffset, text, cause)
        onPipelineOutput?.invoke(output)
    }

    fun sendDeleteToKernel(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto) {
        val output = commandPort.deleteRange(byteStart, byteEndExclusive, cause)
        onPipelineOutput?.invoke(output)
    }

    fun sendReplaceToKernel(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto) {
        val output = commandPort.replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause, null)
        onPipelineOutput?.invoke(output)
    }

    fun sendSetSelectionToKernel(anchorByteOffset: Int, headByteOffset: Int) {
        val output = commandPort.setSelectionTyped(anchorByteOffset, headByteOffset)
        onPipelineOutput?.invoke(output)
    }

    fun sendCommitTextToKernel(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, resultingSelectionAnchor: Int, resultingSelectionHead: Int, cause: EditorTransactionCauseDto) {
        val (sessionId, baseRev, generation) = compositionSessionInfo()
        val preeditAtCommit = currentCompositionText
        val dto = commandPort.commitComposition(
            byteStart, byteEndExclusive, replacementText,
            resultingSelectionAnchor, resultingSelectionHead,
            sessionId, baseRev, generation,
            cause
        )
        val result = dto?.let { EditResult.fromDto(it) }
        if (result != null && result.isApplied()) {
            clearCompositionState()
            val output = commandPort.applyCompositionCommit(dto, preeditAtCommit)
            onPipelineOutput?.invoke(output)
            return
        }
        android.util.Log.w(
            "SujianEditorInput",
            "commitText NOT applied (outcome=${result?.outcome}, dto=${dto != null}); reloading from kernel"
        )
        clearCompositionState()
        commandPort.reloadFromKernel()
    }

    fun sendDeleteSurroundingToKernel(beforeByteStart: Int, beforeByteEndExclusive: Int, afterByteStart: Int, afterByteEndExclusive: Int, cause: EditorTransactionCauseDto) {
        invalidateCompositionSession()
        val dto = commandPort.deleteSurrounding(
            beforeByteStart, beforeByteEndExclusive,
            afterByteStart, afterByteEndExclusive,
            cause
        ) ?: return
        val result = EditResult.fromDto(dto)
        val output = commandPort.applyEditResult(result, null)
        onPipelineOutput?.invoke(output)
    }

    fun sendBeginCompositionToKernel(replaceStart: Int, replaceEndExclusive: Int): Boolean {
        val dto = commandPort.beginComposition(replaceStart, replaceEndExclusive) ?: return false
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
        val (sessionId, _baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return false
        val dto = commandPort.updateComposition(
            sessionId, generation,
            newPreeditText, newPreeditCursorOffset
        ) ?: return false
        val result = EditResult.fromDto(dto)
        if (result.isApplied()) {
            compositionGeneration++
            return true
        }
        return false
    }

    fun sendFinishCompositionToKernel() {
        val (sessionId, _baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return
        val preeditAtFinish = currentCompositionText
        val dto = commandPort.finishComposition(sessionId, generation)
        if (dto != null) {
            val result = EditResult.fromDto(dto)
            if (result.isApplied()) {
                clearCompositionState()
                val output = commandPort.applyCompositionCommit(dto, preeditAtFinish)
                onPipelineOutput?.invoke(output)
                return
            }
        }
        clearCompositionState()
        commandPort.reloadFromKernel()
    }

    fun sendCancelCompositionToKernel(): Boolean {
        val (sessionId, _baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return false
        val dto = commandPort.cancelComposition(sessionId, generation)
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
            // Cancel the kernel-side composition session too: the adapter state and the
            // kernel session must not diverge. If only the adapter state were cleared, a
            // later plain commit would be rejected as StaleRevision by the orphaned kernel
            // session and every subsequent composition begin would fail.
            sendCancelCompositionToKernel()
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
                commandPort.reloadFromKernel()
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

        val updateOk = sendUpdateCompositionToKernel(preeditText, compositionCursorUtf16)
        if (!updateOk) {
            mirror.clearComposition()
            clearCompositionState()
            commandPort.reloadFromKernel()
            onCompositionVisualUpdate?.invoke()
            return
        }

        commandPort.applyCompositionUpdateAnimated(
            compositionReplaceStartUtf8, compositionReplaceEndUtf8,
            preeditText, previousCompositionText
        ) {
            mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
        }
        onCompositionVisualUpdate?.invoke()
    }

    fun handleCompositionCommitWithText(finalText: String, newCursorPosition: Int) {
        if (!isComposing) return
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8

        val (resultingAnchor, resultingHead) = AndroidTextIndexMap.computeResultingSelectionUtf8(
            mirror.getCommittedText(), newCursorPosition, replaceStart, replaceEnd, finalText
        )

        val (sessionId, baseRev, generation) = compositionSessionInfo()
        val preeditAtCommit = currentCompositionText
        val dto = commandPort.commitComposition(
            replaceStart, replaceEnd, finalText,
            resultingAnchor, resultingHead,
            sessionId, baseRev, generation,
            EditorTransactionCauseDto.TYPING_COMMIT
        )
        if (dto != null) {
            val result = EditResult.fromDto(dto)
            if (result.isApplied()) {
                clearCompositionState()
                val output = commandPort.applyCompositionCommit(dto, preeditAtCommit)
                onPipelineOutput?.invoke(output)
                return
            }
        }
        clearCompositionState()
        commandPort.reloadFromKernel()
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        sendFinishCompositionToKernel()
    }

    fun handleCompositionCancel() {
        if (!isComposing) return

        val cancelOk = sendCancelCompositionToKernel()
        val oldPreeditText = currentCompositionText
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8

        commandPort.applyCompositionCancelAnimated(replaceStart, replaceEnd, oldPreeditText) {
            mirror.clearComposition()
        }
        clearCompositionState()

        if (!cancelOk) {
            commandPort.reloadFromKernel()
            onCompositionVisualUpdate?.invoke()
            return
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
