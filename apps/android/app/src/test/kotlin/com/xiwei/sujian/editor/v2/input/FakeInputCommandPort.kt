package com.xiwei.sujian.editor.v2.input

import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.pipeline.InputCommandPort
import com.xiwei.sujian.editor.v2.pipeline.PipelineOutput
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CompositionSessionDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.EditorEditOutcomeDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

/**
 * Deterministic in-memory stand-in for the Rust `EditorKernel` input contract, used by the
 * JVM input-contract tests so they exercise `AndroidInputAdapter` / `AndroidInputConnection`
 * without a system IME or the native library.
 *
 * The fake mirrors the kernel's UTF-8 byte-offset semantics and its composition session
 * validation (session id + base revision + generation), including:
 * - `beginComposition` replaces any stale session and validates range/offset/char boundary;
 * - `updateComposition` rejects mismatched session id/generation/base revision (stale);
 * - `commitComposition` (kernel `commitText`) validates the session, normalizes reversed
 *   ranges, applies the replacement as a display patch and reports the operation kind
 *   (COMPOSITION_COMMIT / INSERT / DELETE / REPLACE) exactly like the kernel;
 * - `finishComposition` materializes the preedit text at the session range;
 * - `cancelComposition` clears the session without touching the committed text.
 *
 * All text mutation is byte-array based (UTF-8), matching the kernel: offsets from the
 * adapter are UTF-8 byte offsets and must never be interpreted as UTF-16 char indices.
 *
 * The fake's committed text stays in sync with [mirror]: edits are returned as
 * `EditorEditResultDto` display patches and applied to the mirror via
 * [applyCompositionCommit] / [applyEditResult], mirroring `AndroidEditorPipeline`.
 * Every kernel-facing call is recorded (counters + [commitCalls]) for contract assertions.
 */
class FakeInputCommandPort(
    override val mirror: DisplayTextMirror,
    initialText: String = "",
    initialCursorUtf8: Int = 0
) : InputCommandPort {

    data class CommitCall(
        val byteStart: Int,
        val byteEndExclusive: Int,
        val replacementText: String,
        val cause: EditorTransactionCauseDto,
        val compositionSessionId: Long,
        val compositionGeneration: Long,
        val operationKind: EditorOperationKindDto
    )

    private var textBytes: ByteArray = initialText.toByteArray(Charsets.UTF_8)
    private var revision: Long = 0L
    private var selectionAnchorUtf8: Int = initialCursorUtf8
    private var selectionHeadUtf8: Int = initialCursorUtf8
    private var nextTransactionId: Long = 1L
    private var nextCompositionSessionId: Long = 7L

    private var sessionId: Long = 0L
    private var sessionBaseRevision: Long = 0L
    private var sessionGeneration: Long = 0L
    private var sessionReplaceStart: Int = 0
    private var sessionReplaceEndExclusive: Int = 0
    private var sessionPreeditText: String = ""
    private var sessionPreeditCursorUtf16: Int = 0

    var beginCompositionCount: Int = 0
        private set
    var updateCompositionCount: Int = 0
        private set
    var finishCompositionCount: Int = 0
        private set
    var cancelCompositionCount: Int = 0
        private set
    var reloadCount: Int = 0
        private set

    /** Every [commitComposition] call in order, with the kernel-selected operation kind. */
    val commitCalls: MutableList<CommitCall> = mutableListOf()

    private fun currentText(): String = String(textBytes, Charsets.UTF_8)

    private fun isCharBoundary(byteOffset: Int): Boolean {
        if (byteOffset <= 0 || byteOffset >= textBytes.size) return true
        return (textBytes[byteOffset].toInt() and 0xC0) != 0x80
    }

    /** Clamp to the nearest preceding UTF-8 char boundary (kernel `clamp_to_char_boundary`). */
    private fun clampToCharBoundary(byteOffset: Int): Int {
        var offset = byteOffset.coerceIn(0, textBytes.size)
        while (offset > 0 && offset < textBytes.size && (textBytes[offset].toInt() and 0xC0) == 0x80) {
            offset--
        }
        return offset
    }

    private fun isSessionActive(): Boolean = sessionId != 0L

    /** True when the fake still holds a live composition session (kernel-side state). */
    fun hasActiveSession(): Boolean = isSessionActive()

    private fun sessionMatches(id: Long, baseRevision: Long, generation: Long): Boolean {
        return isSessionActive() &&
            sessionId == id &&
            sessionBaseRevision == baseRevision &&
            sessionGeneration == generation
    }

    private fun clearSession() {
        sessionId = 0L
        sessionBaseRevision = 0L
        sessionGeneration = 0L
        sessionReplaceStart = 0
        sessionReplaceEndExclusive = 0
        sessionPreeditText = ""
        sessionPreeditCursorUtf16 = 0
    }

    /**
     * Apply a byte-range replacement exactly like the kernel: mutate the committed text,
     * bump the revision, clamp the resulting selection to char boundaries and produce the
     * single display patch the kernel emits.
     */
    private fun applyReplacement(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        cause: EditorTransactionCauseDto,
        operationKind: EditorOperationKindDto,
        oldAffected: List<EditorByteRangeDto>,
        newAffected: List<EditorByteRangeDto>,
        baseRevision: Long
    ): EditorEditResultDto {
        val oldSelectionAnchor = selectionAnchorUtf8
        val oldSelectionHead = selectionHeadUtf8
        val replacementBytes = replacementText.toByteArray(Charsets.UTF_8)
        textBytes = textBytes.copyOfRange(0, byteStart) + replacementBytes +
            textBytes.copyOfRange(byteEndExclusive, textBytes.size)
        revision++
        val selAnchor = clampToCharBoundary(resultingSelectionAnchor)
        val selHead = clampToCharBoundary(resultingSelectionHead)
        selectionAnchorUtf8 = selAnchor
        selectionHeadUtf8 = selHead
        val patch = DisplayPatchDto(
            baseRevision = baseRevision.toULong(),
            newRevision = revision.toULong(),
            replaceByteStart = byteStart.toUInt(),
            replaceByteEndExclusive = byteEndExclusive.toUInt(),
            insertedText = replacementText,
            resultingSelectionStart = selAnchor.toUInt(),
            resultingSelectionEnd = selHead.toUInt()
        )
        return EditorEditResultDto(
            outcome = EditorEditOutcomeDto.APPLIED,
            transactionId = nextTransactionId++.toULong(),
            baseRevision = baseRevision.toULong(),
            newRevision = revision.toULong(),
            displayPatches = listOf(patch),
            oldSelectionStart = oldSelectionAnchor.toUInt(),
            oldSelectionEnd = oldSelectionHead.toUInt(),
            newSelectionStart = selAnchor.toUInt(),
            newSelectionEnd = selHead.toUInt(),
            visualIntent = EditorVisualIntentDto(
                cause = cause,
                operationKind = operationKind,
                oldAffectedByteRanges = oldAffected,
                newAffectedByteRanges = newAffected,
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor = CoordinatedCursorDto(
                    oldByteOffset = selectionAnchorUtf8.toUInt(),
                    newByteOffset = selHead.toUInt(),
                    shouldAnimate = false
                )
            ),
            compositionSession = null
        )
    }

    private fun noopResult(
        outcome: EditorEditOutcomeDto,
        baseRevision: Long,
        operationKind: EditorOperationKindDto,
        cause: EditorTransactionCauseDto
    ): EditorEditResultDto {
        return EditorEditResultDto(
            outcome = outcome,
            transactionId = nextTransactionId++.toULong(),
            baseRevision = baseRevision.toULong(),
            newRevision = revision.toULong(),
            displayPatches = emptyList(),
            oldSelectionStart = selectionAnchorUtf8.toUInt(),
            oldSelectionEnd = selectionHeadUtf8.toUInt(),
            newSelectionStart = selectionAnchorUtf8.toUInt(),
            newSelectionEnd = selectionHeadUtf8.toUInt(),
            visualIntent = EditorVisualIntentDto(
                cause = cause,
                operationKind = operationKind,
                oldAffectedByteRanges = emptyList(),
                newAffectedByteRanges = emptyList(),
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor = CoordinatedCursorDto(
                    oldByteOffset = selectionAnchorUtf8.toUInt(),
                    newByteOffset = selectionHeadUtf8.toUInt(),
                    shouldAnimate = false
                )
            ),
            compositionSession = null
        )
    }

    private fun staleResult(): EditorEditResultDto = noopResult(
        EditorEditOutcomeDto.STALE_REVISION, revision,
        EditorOperationKindDto.COMPOSITION_UPDATE, EditorTransactionCauseDto.IME_COMPOSITION
    )

    override fun beginComposition(replaceStart: Int, replaceEndExclusive: Int): EditorEditResultDto? {
        beginCompositionCount++
        val baseRevision = revision
        // Kernel contract: a new begin is authoritative — any stale session is dropped.
        clearSession()
        if (replaceStart > replaceEndExclusive) {
            return noopResult(EditorEditOutcomeDto.INVALID_RANGE, baseRevision, EditorOperationKindDto.COMPOSITION_UPDATE, EditorTransactionCauseDto.IME_COMPOSITION)
        }
        if (replaceStart > textBytes.size || replaceEndExclusive > textBytes.size) {
            return noopResult(EditorEditOutcomeDto.INVALID_OFFSET, baseRevision, EditorOperationKindDto.COMPOSITION_UPDATE, EditorTransactionCauseDto.IME_COMPOSITION)
        }
        if (!isCharBoundary(replaceStart) || !isCharBoundary(replaceEndExclusive)) {
            return noopResult(EditorEditOutcomeDto.INVALID_OFFSET, baseRevision, EditorOperationKindDto.COMPOSITION_UPDATE, EditorTransactionCauseDto.IME_COMPOSITION)
        }
        sessionId = nextCompositionSessionId++
        sessionBaseRevision = baseRevision
        sessionGeneration = 0L
        sessionReplaceStart = replaceStart
        sessionReplaceEndExclusive = replaceEndExclusive
        sessionPreeditText = ""
        sessionPreeditCursorUtf16 = 0
        return EditorEditResultDto(
            outcome = EditorEditOutcomeDto.APPLIED,
            transactionId = nextTransactionId++.toULong(),
            baseRevision = baseRevision.toULong(),
            newRevision = revision.toULong(),
            displayPatches = emptyList(),
            oldSelectionStart = selectionAnchorUtf8.toUInt(),
            oldSelectionEnd = selectionHeadUtf8.toUInt(),
            newSelectionStart = selectionAnchorUtf8.toUInt(),
            newSelectionEnd = selectionHeadUtf8.toUInt(),
            visualIntent = EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.IME_COMPOSITION,
                operationKind = EditorOperationKindDto.COMPOSITION_UPDATE,
                oldAffectedByteRanges = emptyList(),
                newAffectedByteRanges = emptyList(),
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor = CoordinatedCursorDto(
                    oldByteOffset = selectionAnchorUtf8.toUInt(),
                    newByteOffset = selectionHeadUtf8.toUInt(),
                    shouldAnimate = false
                )
            ),
            compositionSession = CompositionSessionDto(
                sessionId = sessionId.toULong(),
                baseRevision = sessionBaseRevision.toULong(),
                generation = sessionGeneration.toULong()
            )
        )
    }

    override fun updateComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        newPreeditText: String,
        newPreeditCursorOffset: Int
    ): EditorEditResultDto? {
        val baseRevision = revision
        if (!sessionMatches(compositionSessionId, baseRevision, compositionGeneration)) {
            return staleResult()
        }
        updateCompositionCount++
        val oldPreeditBytes = sessionPreeditText.toByteArray(Charsets.UTF_8).size
        val newPreeditBytes = newPreeditText.toByteArray(Charsets.UTF_8).size
        sessionPreeditText = newPreeditText
        sessionPreeditCursorUtf16 = newPreeditCursorOffset
        sessionGeneration++
        val oldAffected = if (oldPreeditBytes > 0) {
            listOf(EditorByteRangeDto(sessionReplaceStart.toUInt(), (sessionReplaceStart + oldPreeditBytes).toUInt()))
        } else {
            emptyList()
        }
        val newAffected = if (newPreeditBytes > 0) {
            listOf(EditorByteRangeDto(sessionReplaceStart.toUInt(), (sessionReplaceStart + newPreeditBytes).toUInt()))
        } else {
            emptyList()
        }
        return EditorEditResultDto(
            outcome = EditorEditOutcomeDto.APPLIED,
            transactionId = nextTransactionId++.toULong(),
            baseRevision = baseRevision.toULong(),
            newRevision = revision.toULong(),
            displayPatches = emptyList(),
            oldSelectionStart = selectionAnchorUtf8.toUInt(),
            oldSelectionEnd = selectionHeadUtf8.toUInt(),
            newSelectionStart = selectionAnchorUtf8.toUInt(),
            newSelectionEnd = selectionHeadUtf8.toUInt(),
            visualIntent = EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.IME_COMPOSITION,
                operationKind = EditorOperationKindDto.COMPOSITION_UPDATE,
                oldAffectedByteRanges = oldAffected,
                newAffectedByteRanges = newAffected,
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor = CoordinatedCursorDto(
                    oldByteOffset = selectionAnchorUtf8.toUInt(),
                    newByteOffset = selectionHeadUtf8.toUInt(),
                    shouldAnimate = false
                )
            ),
            compositionSession = null
        )
    }

    override fun commitComposition(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        compositionSessionId: Long,
        compositionBaseRevision: Long,
        compositionGeneration: Long,
        cause: EditorTransactionCauseDto
    ): EditorEditResultDto? {
        val baseRevision = revision
        // Kernel contract: a live session must match; a stale session id without a live
        // session is rejected too.
        if (isSessionActive()) {
            if (!sessionMatches(compositionSessionId, compositionBaseRevision, compositionGeneration)) {
                return staleResult()
            }
        } else if (compositionSessionId != 0L) {
            return staleResult()
        }
        val normStart = minOf(byteStart, byteEndExclusive)
        val normEnd = maxOf(byteStart, byteEndExclusive)
        if (isSessionActive() && (normStart != sessionReplaceStart || normEnd != sessionReplaceEndExclusive)) {
            return noopResult(EditorEditOutcomeDto.INVALID_RANGE, baseRevision, EditorOperationKindDto.COMPOSITION_UPDATE, cause)
        }
        if (normStart == normEnd && replacementText.isEmpty() && !isSessionActive()) {
            return noopResult(EditorEditOutcomeDto.NO_CHANGE, baseRevision, EditorOperationKindDto.INSERT, cause)
        }
        if (normStart > textBytes.size || normEnd > textBytes.size) {
            return noopResult(EditorEditOutcomeDto.INVALID_OFFSET, baseRevision, EditorOperationKindDto.INSERT, cause)
        }
        if (!isCharBoundary(normStart) || !isCharBoundary(normEnd)) {
            return noopResult(EditorEditOutcomeDto.INVALID_OFFSET, baseRevision, EditorOperationKindDto.INSERT, cause)
        }
        val isCompositionCommit = isSessionActive()
        val preeditByteLen = if (isCompositionCommit) sessionPreeditText.toByteArray(Charsets.UTF_8).size else 0
        clearSession()
        val operationKind = when {
            isCompositionCommit -> EditorOperationKindDto.COMPOSITION_COMMIT
            normStart == normEnd -> EditorOperationKindDto.INSERT
            replacementText.isEmpty() -> EditorOperationKindDto.DELETE
            else -> EditorOperationKindDto.REPLACE
        }
        commitCalls.add(
            CommitCall(
                byteStart = normStart,
                byteEndExclusive = normEnd,
                replacementText = replacementText,
                cause = cause,
                compositionSessionId = compositionSessionId,
                compositionGeneration = compositionGeneration,
                operationKind = operationKind
            )
        )
        val oldAffected = if (preeditByteLen > 0) {
            listOf(EditorByteRangeDto(normStart.toUInt(), (normStart + preeditByteLen).toUInt()))
        } else {
            listOf(EditorByteRangeDto(normStart.toUInt(), normEnd.toUInt()))
        }
        val newAffected = listOf(EditorByteRangeDto(normStart.toUInt(), (normStart + replacementText.toByteArray(Charsets.UTF_8).size).toUInt()))
        return applyReplacement(
            normStart, normEnd, replacementText,
            resultingSelectionAnchor, resultingSelectionHead,
            cause, operationKind, oldAffected, newAffected, baseRevision
        )
    }

    override fun finishComposition(compositionSessionId: Long, compositionGeneration: Long): EditorEditResultDto? {
        val baseRevision = revision
        if (!sessionMatches(compositionSessionId, baseRevision, compositionGeneration)) {
            return staleResult()
        }
        finishCompositionCount++
        if (sessionPreeditText.isEmpty()) {
            // Empty preedit: just close the session, no text change.
            clearSession()
            return noopResult(
                EditorEditOutcomeDto.APPLIED, baseRevision,
                EditorOperationKindDto.COMPOSITION_COMMIT, EditorTransactionCauseDto.TYPING_COMMIT
            )
        }
        val replaceStart = sessionReplaceStart
        val replaceEnd = sessionReplaceEndExclusive
        val committedText = sessionPreeditText
        val preeditCursorUtf16 = sessionPreeditCursorUtf16

        // Kernel contract: resulting cursor walks the preedit code points by UTF-16 count.
        var utf16Count = 0
        var byteOffset = 0
        var index = 0
        while (index < committedText.length) {
            if (utf16Count >= preeditCursorUtf16) break
            val codePoint = committedText.codePointAt(index)
            utf16Count += Character.charCount(codePoint)
            byteOffset += String(Character.toChars(codePoint)).toByteArray(Charsets.UTF_8).size
            index += Character.charCount(codePoint)
        }
        val resultingCursor = clampToCharBoundary(replaceStart + byteOffset)
        clearSession()

        val committedBytes = committedText.toByteArray(Charsets.UTF_8).size
        val oldAffected = listOf(EditorByteRangeDto(replaceStart.toUInt(), (replaceStart + committedBytes).toUInt()))
        val newAffected = oldAffected
        return applyReplacement(
            replaceStart, replaceEnd, committedText,
            resultingCursor, resultingCursor,
            EditorTransactionCauseDto.TYPING_COMMIT,
            EditorOperationKindDto.COMPOSITION_COMMIT,
            oldAffected, newAffected, baseRevision
        )
    }

    override fun cancelComposition(compositionSessionId: Long, compositionGeneration: Long): EditorEditResultDto? {
        val baseRevision = revision
        if (!sessionMatches(compositionSessionId, baseRevision, compositionGeneration)) {
            return staleResult()
        }
        cancelCompositionCount++
        val preeditByteLen = sessionPreeditText.toByteArray(Charsets.UTF_8).size
        val oldAffected = when {
            preeditByteLen > 0 -> listOf(EditorByteRangeDto(sessionReplaceStart.toUInt(), (sessionReplaceStart + preeditByteLen).toUInt()))
            sessionReplaceStart != sessionReplaceEndExclusive -> listOf(EditorByteRangeDto(sessionReplaceStart.toUInt(), sessionReplaceEndExclusive.toUInt()))
            else -> emptyList()
        }
        clearSession()
        return EditorEditResultDto(
            outcome = EditorEditOutcomeDto.APPLIED,
            transactionId = nextTransactionId++.toULong(),
            baseRevision = baseRevision.toULong(),
            newRevision = revision.toULong(),
            displayPatches = emptyList(),
            oldSelectionStart = selectionAnchorUtf8.toUInt(),
            oldSelectionEnd = selectionHeadUtf8.toUInt(),
            newSelectionStart = selectionAnchorUtf8.toUInt(),
            newSelectionEnd = selectionHeadUtf8.toUInt(),
            visualIntent = EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.IME_COMPOSITION,
                operationKind = EditorOperationKindDto.COMPOSITION_CANCEL,
                oldAffectedByteRanges = oldAffected,
                newAffectedByteRanges = emptyList(),
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor = CoordinatedCursorDto(
                    oldByteOffset = selectionAnchorUtf8.toUInt(),
                    newByteOffset = selectionHeadUtf8.toUInt(),
                    shouldAnimate = false
                )
            ),
            compositionSession = null
        )
    }

    override fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto): PipelineOutput {
        if (byteOffset > textBytes.size || !isCharBoundary(byteOffset)) return PipelineOutput.StaleOrInvalid
        val baseRevision = revision
        val dto = applyReplacement(
            byteOffset, byteOffset, text,
            byteOffset + text.toByteArray(Charsets.UTF_8).size, byteOffset + text.toByteArray(Charsets.UTF_8).size,
            cause, EditorOperationKindDto.INSERT, emptyList(),
            listOf(EditorByteRangeDto(byteOffset.toUInt(), (byteOffset + text.toByteArray(Charsets.UTF_8).size).toUInt())),
            baseRevision
        )
        mirror.applyEditResult(EditResult.fromDto(dto))
        return PipelineOutput.Edited(EditResult.fromDto(dto))
    }

    override fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto): PipelineOutput {
        if (byteStart > byteEndExclusive || byteEndExclusive > textBytes.size ||
            !isCharBoundary(byteStart) || !isCharBoundary(byteEndExclusive)) return PipelineOutput.StaleOrInvalid
        val baseRevision = revision
        val dto = applyReplacement(
            byteStart, byteEndExclusive, "",
            byteStart, byteStart,
            cause, EditorOperationKindDto.DELETE,
            listOf(EditorByteRangeDto(byteStart.toUInt(), byteEndExclusive.toUInt())), emptyList(),
            baseRevision
        )
        mirror.applyEditResult(EditResult.fromDto(dto))
        return PipelineOutput.Edited(EditResult.fromDto(dto))
    }

    override fun replaceRangeTyped(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto,
        beforePatch: (() -> Unit)?,
        source: com.xiwei.sujian.editor.v2.host.EditorEditSource
    ): PipelineOutput {
        if (byteStart > byteEndExclusive || byteEndExclusive > textBytes.size ||
            !isCharBoundary(byteStart) || !isCharBoundary(byteEndExclusive)) return PipelineOutput.StaleOrInvalid
        val baseRevision = revision
        val dto = applyReplacement(
            byteStart, byteEndExclusive, replacementText,
            byteStart + replacementText.toByteArray(Charsets.UTF_8).size, byteStart + replacementText.toByteArray(Charsets.UTF_8).size,
            cause, EditorOperationKindDto.REPLACE,
            listOf(EditorByteRangeDto(byteStart.toUInt(), byteEndExclusive.toUInt())),
            listOf(EditorByteRangeDto(byteStart.toUInt(), (byteStart + replacementText.toByteArray(Charsets.UTF_8).size).toUInt())),
            baseRevision
        )
        beforePatch?.invoke()
        mirror.applyEditResult(EditResult.fromDto(dto))
        return PipelineOutput.Edited(EditResult.fromDto(dto))
    }

    override fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int, source: com.xiwei.sujian.editor.v2.host.EditorEditSource): PipelineOutput {
        if (anchorByteOffset > textBytes.size || headByteOffset > textBytes.size ||
            !isCharBoundary(anchorByteOffset) || !isCharBoundary(headByteOffset)) return PipelineOutput.StaleOrInvalid
        val dto = EditorEditResultDto(
            outcome = EditorEditOutcomeDto.NO_CHANGE,
            transactionId = nextTransactionId++.toULong(),
            baseRevision = revision.toULong(),
            newRevision = revision.toULong(),
            displayPatches = emptyList(),
            oldSelectionStart = selectionAnchorUtf8.toUInt(),
            oldSelectionEnd = selectionHeadUtf8.toUInt(),
            newSelectionStart = anchorByteOffset.toUInt(),
            newSelectionEnd = headByteOffset.toUInt(),
            visualIntent = EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.PROGRAMMATIC,
                operationKind = EditorOperationKindDto.CURSOR_ONLY,
                oldAffectedByteRanges = emptyList(),
                newAffectedByteRanges = emptyList(),
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor = CoordinatedCursorDto(
                    oldByteOffset = selectionAnchorUtf8.toUInt(),
                    newByteOffset = headByteOffset.toUInt(),
                    shouldAnimate = false
                )
            ),
            compositionSession = null
        )
        selectionAnchorUtf8 = anchorByteOffset
        selectionHeadUtf8 = headByteOffset
        mirror.applyEditResult(EditResult.fromDto(dto))
        return PipelineOutput.Edited(EditResult.fromDto(dto))
    }

    override fun deleteSurrounding(
        beforeByteStart: Int,
        beforeByteEndExclusive: Int,
        afterByteStart: Int,
        afterByteEndExclusive: Int,
        cause: EditorTransactionCauseDto
    ): EditorEditResultDto? {
        val baseRevision = revision
        val beforeRange = if (beforeByteStart < beforeByteEndExclusive) Pair(beforeByteStart, beforeByteEndExclusive) else null
        val afterRange = if (afterByteStart < afterByteEndExclusive) Pair(afterByteStart, afterByteEndExclusive) else null
        if (beforeRange == null && afterRange == null) {
            return noopResult(EditorEditOutcomeDto.NO_CHANGE, baseRevision, EditorOperationKindDto.DELETE, cause)
        }
        val ranges = listOfNotNull(beforeRange, afterRange)
        for ((start, end) in ranges) {
            if (start > textBytes.size || end > textBytes.size || !isCharBoundary(start) || !isCharBoundary(end)) {
                return noopResult(EditorEditOutcomeDto.INVALID_OFFSET, baseRevision, EditorOperationKindDto.DELETE, cause)
            }
        }
        // Kernel contract: one display patch per deleted range, applied in order.
        var bytes = textBytes
        val patches = mutableListOf<DisplayPatchDto>()
        for ((start, end) in ranges.sortedByDescending { it.second }) {
            bytes = bytes.copyOfRange(0, start) + bytes.copyOfRange(end, bytes.size)
            revision++
            patches.add(
                DisplayPatchDto(
                    baseRevision = (revision - 1).toULong(),
                    newRevision = revision.toULong(),
                    replaceByteStart = start.toUInt(),
                    replaceByteEndExclusive = end.toUInt(),
                    insertedText = "",
                    resultingSelectionStart = start.toUInt(),
                    resultingSelectionEnd = start.toUInt()
                )
            )
        }
        textBytes = bytes
        val cursor = ranges.minOf { it.first }.coerceAtMost(textBytes.size)
        selectionAnchorUtf8 = cursor
        selectionHeadUtf8 = cursor
        return EditorEditResultDto(
            outcome = EditorEditOutcomeDto.APPLIED,
            transactionId = nextTransactionId++.toULong(),
            baseRevision = baseRevision.toULong(),
            newRevision = revision.toULong(),
            displayPatches = patches,
            oldSelectionStart = selectionAnchorUtf8.toUInt(),
            oldSelectionEnd = selectionHeadUtf8.toUInt(),
            newSelectionStart = cursor.toUInt(),
            newSelectionEnd = cursor.toUInt(),
            visualIntent = EditorVisualIntentDto(
                cause = cause,
                operationKind = EditorOperationKindDto.DELETE,
                oldAffectedByteRanges = ranges.map { EditorByteRangeDto(it.first.toUInt(), it.second.toUInt()) },
                newAffectedByteRanges = emptyList(),
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor = CoordinatedCursorDto(
                    oldByteOffset = selectionAnchorUtf8.toUInt(),
                    newByteOffset = cursor.toUInt(),
                    shouldAnimate = false
                )
            ),
            compositionSession = null
        )
    }

    override fun applyEditResult(result: EditResult, beforePatch: (() -> Unit)?, source: com.xiwei.sujian.editor.v2.host.EditorEditSource): PipelineOutput {
        beforePatch?.invoke()
        mirror.applyEditResult(result)
        return PipelineOutput.Edited(result, source)
    }

    override fun applyCompositionCommit(dto: EditorEditResultDto, preeditText: String): PipelineOutput {
        val result = EditResult.fromDto(dto)
        mirror.applyEditResult(result)
        return PipelineOutput.Edited(result)
    }

    override fun applyCompositionUpdateAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        newPreeditText: String,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)?
    ) {
        mirrorUpdate?.invoke()
    }

    override fun applyCompositionCancelAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)?
    ) {
        mirrorUpdate?.invoke()
    }

    override fun onCompositionUpdated() = Unit

    /**
     * Simulate an out-of-band kernel text reset (external sync / chapter switch while the
     * editor is bound): the kernel replaces the committed text, bumps the revision and
     * drops any composition session — kernel `load_text` semantics — and the mirror is
     * reloaded from the new snapshot, exactly like `EditPipeline.loadText` does after a
     * successful `bridge.loadText`. The adapter is NOT notified (same as the real flow:
     * `SujianEditorView.loadText` never touches the adapter's composition state).
     */
    fun simulateExternalReset(newText: String, cursorUtf8: Int) {
        textBytes = newText.toByteArray(Charsets.UTF_8)
        revision++
        selectionAnchorUtf8 = cursorUtf8
        selectionHeadUtf8 = cursorUtf8
        clearSession()
        mirror.loadFromSnapshot(newText, cursorUtf8, revision, cursorUtf8, cursorUtf8)
    }

    override fun reloadFromKernel(): Boolean {
        reloadCount++
        mirror.loadFromSnapshot(
            currentText(),
            selectionHeadUtf8,
            revision,
            selectionAnchorUtf8,
            selectionHeadUtf8
        )
        return true
    }

    override fun getCursorUtf8(): Int = mirror.getCursorUtf8()

    override fun getRevision(): Long = mirror.getRevision()

    override fun getText(): String = mirror.getText()

    /** Current committed text held by the fake (UTF-8 decoded). */
    fun getKernelText(): String = currentText()

    fun getKernelRevision(): Long = revision

    fun getKernelSelectionAnchorUtf8(): Int = selectionAnchorUtf8

    fun getKernelSelectionHeadUtf8(): Int = selectionHeadUtf8
}
