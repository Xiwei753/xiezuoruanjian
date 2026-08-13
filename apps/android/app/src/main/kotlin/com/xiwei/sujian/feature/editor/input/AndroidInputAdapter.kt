package com.xiwei.sujian.feature.editor.input

import android.view.View
import com.xiwei.sujian.feature.editor.pipeline.InputCommandPort
import com.xiwei.sujian.feature.editor.pipeline.PipelineOutput
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.EditResult
import com.xiwei.sujian.feature.editor.session.AutocorrectPolicy
import com.xiwei.sujian.feature.editor.session.CapitalizationPolicy
import com.xiwei.sujian.feature.editor.session.ImeAction
import com.xiwei.sujian.feature.editor.session.NewlinePolicy
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.TextInputType
import uniffi.writer_core.EditorTransactionCauseDto

class AndroidInputAdapter(
    private val mirror: DisplayTextMirror,
    private val commandPort: InputCommandPort,
    private val projectionProvider: (() -> com.xiwei.sujian.feature.editor.projection.DisplayTextProjection)? = null,
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

    fun onCreateInputConnection(
        outAttrs: android.view.inputmethod.EditorInfo?,
    ): android.view.inputmethod.InputConnection? {
        val host = hostView ?: return null
        // NOTE (Issue #589): composition validity is deliberately NOT tied to
        // InputConnection creation. onCreateInputConnection is invoked by the system on
        // IME rebinding AND spuriously by other callers (Espresso view descriptions,
        // direct connection probing), so cancelling here would destroy live compositions.
        // Orphaned kernel sessions (IME switch mid-composition) are healed by the
        // kernel session/revision validation plus the adapter's stale-session retry paths
        // (see handleCompositionCommitWithText / handleCompositionUpdate).
        if (outAttrs != null) {
            val inputType =
                when (currentProfile.inputType) {
                    TextInputType.NUMBER -> android.text.InputType.TYPE_CLASS_NUMBER
                    TextInputType.EMAIL ->
                        android.text.InputType.TYPE_CLASS_TEXT or
                            android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
                    TextInputType.MULTI_LINE ->
                        android.text.InputType.TYPE_CLASS_TEXT or
                            android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
                    TextInputType.TEXT -> android.text.InputType.TYPE_CLASS_TEXT
                    TextInputType.PASSWORD ->
                        android.text.InputType.TYPE_CLASS_TEXT or
                            android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
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
                CapitalizationPolicy.CHARACTERS ->
                    outAttrs.inputType = outAttrs.inputType or android.text.InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS
                CapitalizationPolicy.WORDS ->
                    outAttrs.inputType = outAttrs.inputType or android.text.InputType.TYPE_TEXT_FLAG_CAP_WORDS
                CapitalizationPolicy.SENTENCES ->
                    outAttrs.inputType = outAttrs.inputType or android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
                CapitalizationPolicy.NONE -> { }
            }

            val imeAction =
                when (currentProfile.imeAction) {
                    ImeAction.DONE -> android.view.inputmethod.EditorInfo.IME_ACTION_DONE
                    ImeAction.SEARCH -> android.view.inputmethod.EditorInfo.IME_ACTION_SEARCH
                    ImeAction.NEXT -> android.view.inputmethod.EditorInfo.IME_ACTION_NEXT
                    ImeAction.GO -> android.view.inputmethod.EditorInfo.IME_ACTION_GO
                    ImeAction.NONE -> android.view.inputmethod.EditorInfo.IME_ACTION_NONE
                }
            outAttrs.imeOptions = imeAction
            if (currentProfile.singleLine && !currentProfile.commitOnImeAction) {
                outAttrs.imeOptions =
                    outAttrs.imeOptions or android.view.inputmethod.EditorInfo.IME_FLAG_NO_ENTER_ACTION
            }
            return AndroidInputConnection(this, mirror, commandPort, host, projectionProvider)
        }
        return null
    }

    fun sendInsertToKernel(
        byteOffset: Int,
        text: String,
        cause: EditorTransactionCauseDto,
    ) {
        val output = commandPort.insertText(byteOffset, text, cause)
        onPipelineOutput?.invoke(output)
    }

    /**
     * #624 评论2：统一换行命令的输出路由 — 软键盘 commitText("\n"/"\r\n") 与
     * 硬件 Enter 共用同一入口；输出必须与其他 send* 路径一样经 [onPipelineOutput]
     * 回到宿主（滚动上限更新、onLocalEdit/onContentChanged 回调、invalidate、
     * 动画帧请求），不能丢弃 — 否则正文进了 mirror 但屏幕不重绘、会话层与
     * ViewModel 内容不更新。
     */
    fun sendLineBreakToKernel(cause: EditorTransactionCauseDto) {
        val output = commandPort.insertLineBreak(cause)
        onPipelineOutput?.invoke(output)
    }

    fun sendDeleteToKernel(
        byteStart: Int,
        byteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
    ) {
        val output = commandPort.deleteRange(byteStart, byteEndExclusive, cause)
        onPipelineOutput?.invoke(output)
    }

    fun sendReplaceToKernel(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto,
    ) {
        val output =
            commandPort.replaceRangeTyped(
                byteStart,
                byteEndExclusive,
                replacementText,
                originalText,
                cause,
                null,
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.NORMAL,
            )
        onPipelineOutput?.invoke(output)
    }

    fun sendSetSelectionToKernel(
        anchorByteOffset: Int,
        headByteOffset: Int,
    ) {
        val output =
            commandPort.setSelectionTyped(
                anchorByteOffset,
                headByteOffset,
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.NORMAL,
            )
        onPipelineOutput?.invoke(output)
    }

    fun sendCommitTextToKernel(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        cause: EditorTransactionCauseDto,
    ) {
        val (sessionId, baseRev, generation) = compositionSessionInfo()
        val dto =
            commandPort.commitComposition(
                byteStart, byteEndExclusive, replacementText,
                resultingSelectionAnchor, resultingSelectionHead,
                sessionId, baseRev, generation,
                cause,
            )
        val result = dto?.let { EditResult.fromDto(it) }
        if (result != null && result.isApplied()) {
            clearCompositionState()
            val output = commandPort.applyCompositionCommit(dto)
            onPipelineOutput?.invoke(output)
            return
        }
        android.util.Log.w(
            "SujianEditorInput",
            "commitText NOT applied (outcome=${result?.outcome}, dto=${dto != null}); reloading from kernel",
        )
        clearCompositionState()
        commandPort.reloadFromKernel()
    }

    fun sendDeleteSurroundingToKernel(
        beforeByteStart: Int,
        beforeByteEndExclusive: Int,
        afterByteStart: Int,
        afterByteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
    ) {
        invalidateCompositionSession()
        val dto =
            commandPort.deleteSurrounding(
                beforeByteStart, beforeByteEndExclusive,
                afterByteStart, afterByteEndExclusive,
                cause,
            ) ?: return
        val result = EditResult.fromDto(dto)
        val output =
            commandPort.applyEditResult(
                result,
                null,
                com.xiwei.sujian.feature.editor.platform.EditorEditSource.NORMAL,
            )
        onPipelineOutput?.invoke(output)
    }

    fun sendBeginCompositionToKernel(
        replaceStart: Int,
        replaceEndExclusive: Int,
    ): Boolean {
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.compositionBegin(replaceStart, replaceEndExclusive)
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

    fun sendUpdateCompositionToKernel(
        newPreeditText: String,
        newPreeditCursorOffset: Int,
    ): Boolean {
        val (sessionId, _baseRev, generation) = compositionSessionInfo()
        if (sessionId == 0L) return false
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.compositionUpdate(
            newPreeditText.toByteArray(Charsets.UTF_8).size,
            newPreeditCursorOffset,
        )
        val dto =
            commandPort.updateComposition(
                sessionId, generation,
                newPreeditText, newPreeditCursorOffset,
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
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.compositionCommit(
            compositionReplaceStartUtf8,
            compositionReplaceStartUtf8 + preeditAtFinish.toByteArray(Charsets.UTF_8).size,
            preeditAtFinish.toByteArray(Charsets.UTF_8).size,
        )
        val dto = commandPort.finishComposition(sessionId, generation)
        if (dto != null) {
            val result = EditResult.fromDto(dto)
            if (result.isApplied()) {
                clearCompositionState()
                val output = commandPort.applyCompositionCommit(dto)
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
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.compositionCancel(
            compositionReplaceStartUtf8,
            compositionReplaceStartUtf8 + currentCompositionText.toByteArray(Charsets.UTF_8).size,
            currentCompositionText.toByteArray(Charsets.UTF_8).size,
        )
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

    fun handleCompositionUpdate(
        preeditText: String,
        newCursorPosition: Int,
    ) {
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
        compositionCursorUtf16 =
            if (newCursorPosition > 0) {
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

        commandPort.applyCompositionUpdateAnimated {
            mirror.updateComposition(compositionReplaceStartUtf8, compositionReplaceEndUtf8, preeditText)
        }
        onCompositionVisualUpdate?.invoke()
    }

    fun handleCompositionCommitWithText(
        finalText: String,
        newCursorPosition: Int,
    ) {
        if (!isComposing) return
        val replaceStart = compositionReplaceStartUtf8
        val replaceEnd = compositionReplaceEndUtf8

        val (resultingAnchor, resultingHead) =
            AndroidTextIndexMap.computeResultingSelectionUtf8(
                mirror.getCommittedText(),
                newCursorPosition,
                replaceStart,
                replaceEnd,
                finalText,
            )

        val (sessionId, baseRev, generation) = compositionSessionInfo()
        val dto =
            commandPort.commitComposition(
                replaceStart, replaceEnd, finalText,
                resultingAnchor, resultingHead,
                sessionId, baseRev, generation,
                EditorTransactionCauseDto.TYPING_COMMIT,
            )
        if (dto != null) {
            val result = EditResult.fromDto(dto)
            if (result.isApplied()) {
                clearCompositionState()
                val output = commandPort.applyCompositionCommit(dto)
                onPipelineOutput?.invoke(output)
                return
            }
        }
        // Orphaned composition session (Issue #589): the kernel session is stale — the
        // IME binding that started the composition is gone (IME switch / soft reset), so
        // the composition-commit path is rejected (STALE_REVISION). The composition's
        // replace range is still known locally, so the same replacement can be replayed as
        // a PLAIN commit (sessionId = 0 after clearCompositionState) — but ONLY while the
        // committed text is unchanged since the composition began. The kernel bumps the
        // revision on every committed-text change (load/reset/sync/other edits) while
        // composition begin/update do NOT, so revision equality between the composition's
        // base revision and the current kernel revision is exactly the invariant "the byte
        // range [replaceStart, replaceEnd) still addresses the same text". Replaying a
        // stale range onto changed text would overwrite the wrong characters (the local
        // offsets are positional, not content-addressed), so on any revision change the
        // only safe resolution is a kernel reload — the adapter state must not paper over
        // the divergence.
        if (commandPort.getRevision() != baseRev) {
            clearCompositionState()
            mirror.clearComposition()
            commandPort.reloadFromKernel()
            onCompositionVisualUpdate?.invoke()
            return
        }
        clearCompositionState()
        val originalText = extractCommittedTextAt(replaceStart, replaceEnd)
        sendCommitTextToKernel(
            replaceStart,
            replaceEnd,
            finalText,
            originalText,
            resultingAnchor,
            resultingHead,
            EditorTransactionCauseDto.TYPING,
        )
    }

    private fun extractCommittedTextAt(
        byteStart: Int,
        byteEndExclusive: Int,
    ): String {
        val bytes = mirror.getCommittedText().toByteArray(Charsets.UTF_8)
        if (byteStart < 0 || byteEndExclusive > bytes.size || byteStart > byteEndExclusive) return ""
        return String(bytes.copyOfRange(byteStart, byteEndExclusive), Charsets.UTF_8)
    }

    fun handleCompositionFinish() {
        if (!isComposing) return
        sendFinishCompositionToKernel()
    }

    fun handleCompositionCancel() {
        if (!isComposing) return

        val cancelOk = sendCancelCompositionToKernel()

        commandPort.applyCompositionCancelAnimated {
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

    fun startComposingRegion(
        byteStart: Int,
        byteEnd: Int,
        selectedText: String,
    ) {
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
