package com.xiwei.sujian.editor.v2.mirror

import android.text.SpannableStringBuilder
import android.text.style.UnderlineSpan
import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorVisualIntentDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorOperationKindDto

data class DisplayPatch(
    val baseRevision: Long,
    val newRevision: Long,
    val replaceByteStart: Int,
    val replaceByteEndExclusive: Int,
    val insertedText: String,
    val resultingSelectionStart: Int,
    val resultingSelectionEnd: Int
) {
    companion object {
        fun fromDto(dto: DisplayPatchDto): DisplayPatch = DisplayPatch(
            baseRevision = dto.baseRevision.toLong(),
            newRevision = dto.newRevision.toLong(),
            replaceByteStart = dto.replaceByteStart.toInt(),
            replaceByteEndExclusive = dto.replaceByteEndExclusive.toInt(),
            insertedText = dto.insertedText,
            resultingSelectionStart = dto.resultingSelectionStart.toInt(),
            resultingSelectionEnd = dto.resultingSelectionEnd.toInt()
        )

        fun fromDtoList(dtos: List<DisplayPatchDto>): List<DisplayPatch> =
            dtos.map { fromDto(it) }
    }
}

data class VisualIntent(
    val cause: EditorTransactionCauseDto,
    val operationKind: EditorOperationKindDto,
    val oldAffectedByteRanges: List<Pair<Int, Int>>,
    val newAffectedByteRanges: List<Pair<Int, Int>>,
    val animationMode: AnimationModeDto,
    val durationMs: Long,
    val coordinatedCursor: CoordinatedCursor
) {
    companion object {
        fun fromDto(dto: EditorVisualIntentDto): VisualIntent = VisualIntent(
            cause = dto.cause,
            operationKind = dto.operationKind,
            oldAffectedByteRanges = dto.oldAffectedByteRanges.map { Pair(it.start.toInt(), it.endExclusive.toInt()) },
            newAffectedByteRanges = dto.newAffectedByteRanges.map { Pair(it.start.toInt(), it.endExclusive.toInt()) },
            animationMode = dto.animationMode,
            durationMs = dto.durationMs.toLong(),
            coordinatedCursor = CoordinatedCursor.fromDto(dto.coordinatedCursor)
        )
    }

    fun isInsert(): Boolean = operationKind == EditorOperationKindDto.INSERT
    fun isDelete(): Boolean = operationKind == EditorOperationKindDto.DELETE
    fun isReplace(): Boolean = operationKind == EditorOperationKindDto.REPLACE
    fun isCompositionUpdate(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_UPDATE
    fun isCompositionCommit(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_COMMIT
    fun isCompositionCancel(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_CANCEL
    fun isCursorOnly(): Boolean = operationKind == EditorOperationKindDto.CURSOR_ONLY
}

data class CoordinatedCursor(
    val oldByteOffset: Int,
    val newByteOffset: Int,
    val shouldAnimate: Boolean
) {
    companion object {
        fun fromDto(dto: CoordinatedCursorDto): CoordinatedCursor = CoordinatedCursor(
            oldByteOffset = dto.oldByteOffset.toInt(),
            newByteOffset = dto.newByteOffset.toInt(),
            shouldAnimate = dto.shouldAnimate
        )
    }
}

data class EditResult(
    val outcome: uniffi.writer_core.EditorEditOutcomeDto,
    val transactionId: Long,
    val baseRevision: Long,
    val newRevision: Long,
    val displayPatches: List<DisplayPatch>,
    val oldSelectionStart: Int,
    val oldSelectionEnd: Int,
    val newSelectionStart: Int,
    val newSelectionEnd: Int,
    val visualIntent: VisualIntent
) {
    companion object {
        fun fromDto(dto: EditorEditResultDto): EditResult = EditResult(
            outcome = dto.outcome,
            transactionId = dto.transactionId.toLong(),
            baseRevision = dto.baseRevision.toLong(),
            newRevision = dto.newRevision.toLong(),
            displayPatches = DisplayPatch.fromDtoList(dto.displayPatches),
            oldSelectionStart = dto.oldSelectionStart.toInt(),
            oldSelectionEnd = dto.oldSelectionEnd.toInt(),
            newSelectionStart = dto.newSelectionStart.toInt(),
            newSelectionEnd = dto.newSelectionEnd.toInt(),
            visualIntent = VisualIntent.fromDto(dto.visualIntent)
        )
    }

    fun isApplied(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.APPLIED
    fun isStale(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.STALE_REVISION
    fun isInvalid(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_OFFSET || outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_RANGE
    fun isNoChange(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.NO_CHANGE
}

class DisplayTextMirror {
    private val buffer = SpannableStringBuilder()
    private var currentRevision: Long = 0
    private var cursorUtf8: Int = 0
    private var cursorUtf16: Int = 0
    private var compositionStartUtf16: Int = -1
    private var compositionEndUtf16: Int = -1
    private var selectionAnchorUtf8: Int = 0
    private var selectionHeadUtf8: Int = 0
    private var selectionAnchorUtf16: Int = 0
    private var selectionHeadUtf16: Int = 0
    private var compositionReplaceStartUtf8: Int = 0
    private var compositionReplaceEndUtf8: Int = 0
    private var compositionOriginalText: String = ""
    private var hasActiveComposition: Boolean = false

    fun getText(): String = buffer.toString()

    fun getCursorUtf8(): Int = cursorUtf8

    fun getCursorUtf16(): Int = cursorUtf16

    fun getRevision(): Long = currentRevision

    fun getSpannable(): SpannableStringBuilder = buffer

    fun getLengthUtf16(): Int = buffer.length

    fun getSelectionStartUtf16(): Int = minOf(selectionAnchorUtf16, selectionHeadUtf16)

    fun getSelectionEndUtf16(): Int = maxOf(selectionAnchorUtf16, selectionHeadUtf16)

    fun getSelectionStartUtf8(): Int = minOf(selectionAnchorUtf8, selectionHeadUtf8)

    fun getSelectionEndUtf8(): Int = maxOf(selectionAnchorUtf8, selectionHeadUtf8)

    fun getSelectionAnchorUtf8(): Int = selectionAnchorUtf8

    fun getSelectionHeadUtf8(): Int = selectionHeadUtf8

    fun getSelectionAnchorUtf16(): Int = selectionAnchorUtf16

    fun getSelectionHeadUtf16(): Int = selectionHeadUtf16

    fun hasComposition(): Boolean = hasActiveComposition

    fun applyEditResult(result: EditResult) {
        val hadComposition = hasActiveComposition
        if (hadComposition) {
            removeCompositionOverlay()
        }
        applyPatches(result.displayPatches)
        updateSelectionFromResult(result)
    }

    private fun updateSelectionFromResult(result: EditResult) {
        val indexMap = AndroidTextIndexMap(this)
        val normStart = minOf(result.newSelectionStart, result.newSelectionEnd)
        val normEnd = maxOf(result.newSelectionStart, result.newSelectionEnd)
        cursorUtf8 = normEnd
        cursorUtf16 = indexMap.utf8ToUtf16(normEnd)
        selectionAnchorUtf8 = normStart
        selectionHeadUtf8 = normEnd
        selectionAnchorUtf16 = indexMap.utf8ToUtf16(normStart)
        selectionHeadUtf16 = indexMap.utf8ToUtf16(normEnd)
    }

    fun applyPatches(patches: List<DisplayPatch>) {
        if (patches.isEmpty()) return

        var indexMap = AndroidTextIndexMap(this)
        for (patch in patches) {
            if (patch.baseRevision != currentRevision) {
                throw IllegalStateException(
                    "DisplayTextMirror revision discontinuity: expected baseRevision=$currentRevision, got ${patch.baseRevision}. " +
                    "Must reload from EditorSession."
                )
            }

            val normReplaceStart = minOf(patch.replaceByteStart, patch.replaceByteEndExclusive)
            val normReplaceEnd = maxOf(patch.replaceByteStart, patch.replaceByteEndExclusive)

            val replaceStartUtf16 = indexMap.utf8ToUtf16(normReplaceStart)
            val replaceEndUtf16 = indexMap.utf8ToUtf16(normReplaceEnd)

            buffer.replace(replaceStartUtf16, replaceEndUtf16, patch.insertedText as CharSequence)

            currentRevision = patch.newRevision
            indexMap = AndroidTextIndexMap(this)
        }
    }

    fun applyDtoPatches(patches: List<DisplayPatchDto>) {
        applyPatches(DisplayPatch.fromDtoList(patches))
    }

    fun restoreCompositionBeforePatch() {
        removeCompositionOverlay()
    }

    fun updateComposition(replaceStartUtf8: Int, replaceEndUtf8: Int, preeditText: String) {
        val indexMap = AndroidTextIndexMap(this)
        removeCompositionOverlay()

        compositionReplaceStartUtf8 = replaceStartUtf8
        compositionReplaceEndUtf8 = replaceEndUtf8
        val insertStartUtf16 = indexMap.utf8ToUtf16(replaceStartUtf8)
        val insertEndUtf16 = indexMap.utf8ToUtf16(replaceEndUtf8)

        if (insertStartUtf16 < insertEndUtf16) {
            compositionOriginalText = buffer.substring(insertStartUtf16, insertEndUtf16)
            buffer.replace(insertStartUtf16, insertEndUtf16, preeditText as CharSequence)
        } else {
            compositionOriginalText = ""
            buffer.insert(insertStartUtf16, preeditText as CharSequence)
        }

        compositionStartUtf16 = insertStartUtf16
        compositionEndUtf16 = insertStartUtf16 + preeditText.length
        hasActiveComposition = true

        buffer.setSpan(
            UnderlineSpan(),
            compositionStartUtf16,
            compositionEndUtf16,
            SpannableStringBuilder.SPAN_EXCLUSIVE_EXCLUSIVE
        )
    }

    fun clearComposition() {
        removeCompositionOverlay()
    }

    private fun removeCompositionOverlay() {
        if (!hasActiveComposition) return
        if (compositionStartUtf16 >= 0 && compositionEndUtf16 > compositionStartUtf16) {
            clearCompositionSpans()
            buffer.replace(compositionStartUtf16, compositionEndUtf16, compositionOriginalText)
        }
        compositionStartUtf16 = -1
        compositionEndUtf16 = -1
        hasActiveComposition = false
        compositionOriginalText = ""
    }

    fun getCompositionRangeUtf16(): Pair<Int, Int>? {
        if (compositionStartUtf16 < 0) return null
        return Pair(compositionStartUtf16, compositionEndUtf16)
    }

    private fun clearCompositionSpans() {
        val spans = buffer.getSpans(0, buffer.length, UnderlineSpan::class.java)
        for (span in spans) {
            buffer.removeSpan(span)
        }
    }

    fun loadFromSnapshot(text: String, cursorUtf8: Int, revision: Long, selectionAnchorUtf8: Int = cursorUtf8, selectionHeadUtf8: Int = cursorUtf8) {
        buffer.clear()
        buffer.append(text)
        this.cursorUtf8 = cursorUtf8
        this.currentRevision = revision
        this.compositionStartUtf16 = -1
        this.compositionEndUtf16 = -1
        this.hasActiveComposition = false
        this.compositionOriginalText = ""
        this.selectionAnchorUtf8 = selectionAnchorUtf8
        this.selectionHeadUtf8 = selectionHeadUtf8
        val indexMap = AndroidTextIndexMap(this)
        this.cursorUtf16 = indexMap.utf8ToUtf16(cursorUtf8)
        this.selectionAnchorUtf16 = indexMap.utf8ToUtf16(selectionAnchorUtf8)
        this.selectionHeadUtf16 = indexMap.utf8ToUtf16(selectionHeadUtf8)
    }

    fun loadText(text: String, cursorUtf8: Int) {
        loadFromSnapshot(text, cursorUtf8, 0)
    }

    fun setSelectionInternal(anchorUtf8: Int, headUtf8: Int) {
        val indexMap = AndroidTextIndexMap(this)
        selectionAnchorUtf8 = anchorUtf8
        selectionHeadUtf8 = headUtf8
        selectionAnchorUtf16 = indexMap.utf8ToUtf16(anchorUtf8)
        selectionHeadUtf16 = indexMap.utf8ToUtf16(headUtf8)
        cursorUtf8 = headUtf8
        cursorUtf16 = selectionHeadUtf16
    }
}
