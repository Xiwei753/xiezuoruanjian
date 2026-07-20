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

/**
 * A single incremental text patch from the Rust kernel.
 * Byte ranges are half-open: [replaceByteStart, replaceByteEndExclusive).
 */
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
    /**
     * Semantic category of the edit, used by the animation planner to select the correct
     * slice-generation path. Composition operations are separate kinds (not just REPLACE)
     * because the preedit text is a virtual overlay on the committed buffer — the planner
     * must treat COMPOSITION_CANCEL as a Delete (preedit text fades out, retained text
     * Moves back) and COMPOSITION_COMMIT/UPDATE as a Replace (old preedit fades out,
     * new text fades in, retained text Moves). Using REPLACE for all three would lose
     * the Delete semantics of cancel and the virtual-overlay semantics of update/commit.
     */
    val operationKind: EditorOperationKindDto,
    /** Byte ranges in the old document affected by this edit. Half-open: [start, end).
     *  For pure Insert, this list is empty (no old bytes were affected). */
    val oldAffectedByteRanges: List<Pair<Int, Int>>,
    /** Byte ranges in the new document affected by this edit. Half-open: [start, end).
     *  For pure Delete, this list is empty (no new bytes were created). */
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

    fun isApplied(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.APPLIED || outcome == uniffi.writer_core.EditorEditOutcomeDto.APPLIED_WITH_ADJUSTED_SELECTION
    fun isStale(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.STALE_REVISION
    fun isInvalid(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_OFFSET || outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_RANGE
    fun isNoChange(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.NO_CHANGE
}

/**
 * Platform-side text buffer that mirrors the Rust EditorKernel's committed text state.
 *
 * Composition overlay model: when an IME composition is active, the preedit text is
 * overlaid on top of the committed text in the SpannableStringBuilder (with an
 * UnderlineSpan). The original text under the preedit range is saved in
 * [compositionOriginalText] and restored when the composition is cleared or committed.
 *
 * Design intent: the overlay model exists because the Rust EditorKernel operates on
 * committed text only — it never sees the preedit. The platform must maintain the
 * committed-text view for the kernel (via [getCommittedText]/[getCommittedCursorUtf8])
 * while simultaneously presenting the preedit to the IME and layout engine. Directly
 * modifying the buffer with preedit text and then reverting on cancel would require
 * the kernel to undo a non-existent edit; the overlay avoids this by keeping the
 * committed buffer untouched and layering the preedit on top.
 *
 * "Committed" accessors ([getCommittedCursorUtf8], [getCommittedText], etc.) return
 * values as if the active composition did not exist — they reflect the state that the
 * Rust kernel sees, which operates on committed text only. The IME sees the full buffer
 * including the preedit overlay.
 *
 * Thread constraint: this class is not thread-safe; all access must be on the UI thread.
 */
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
    /** Committed-text UTF-8 byte offset where the composition replacement starts.
     *  In committed-text coordinates (not virtual/preedit coordinates), matching the
     *  Rust CompositionSession convention. The kernel only knows committed text, so
     *  all composition range parameters sent to the kernel must use these coordinates.
     *  The virtual preedit range in the buffer ([compositionStartUtf16]/[compositionEndUtf16])
     *  is in full-buffer coordinates (including the overlay) and must NOT be sent to the kernel. */
    private var compositionReplaceStartUtf8: Int = 0
    /** Committed-text UTF-8 byte offset where the composition replacement ends (exclusive).
     *  Same coordinate convention as [compositionReplaceStartUtf8] — committed-text space. */
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

    /** Cursor position in the committed text (excluding active composition overlay).
     *  When a composition is active, returns the start of the composition range —
     *  the Rust kernel's cursor is at the composition boundary, not inside the preedit. */
    fun getCommittedCursorUtf8(): Int {
        if (!hasActiveComposition) return cursorUtf8
        return compositionReplaceStartUtf8
    }

    fun getCommittedSelectionStartUtf8(): Int {
        if (!hasActiveComposition) return getSelectionStartUtf8()
        return compositionReplaceStartUtf8
    }

    fun getCommittedSelectionEndUtf8(): Int {
        if (!hasActiveComposition) return getSelectionEndUtf8()
        return compositionReplaceStartUtf8
    }

    /** Full text as seen by the Rust kernel (committed text only, excluding preedit overlay).
     *  Reconstructs the text by replacing the preedit range with [compositionOriginalText]. */
    fun getCommittedText(): String {
        if (!hasActiveComposition) return buffer.toString()
        val indexMap = AndroidTextIndexMap(this)
        val startUtf16 = indexMap.utf8ToUtf16(compositionReplaceStartUtf8)
        val endUtf16 = compositionStartUtf16
        return buffer.substring(0, startUtf16) + compositionOriginalText + buffer.substring(compositionEndUtf16.coerceAtMost(buffer.length))
    }

    fun getCommittedLengthUtf16(): Int {
        if (!hasActiveComposition) return buffer.length
        val indexMap = AndroidTextIndexMap(this)
        val startUtf16 = indexMap.utf8ToUtf16(compositionReplaceStartUtf8)
        return startUtf16 + compositionOriginalText.length + (buffer.length - compositionEndUtf16.coerceAtMost(buffer.length))
    }

    fun applyEditResult(result: EditResult) {
        val hadComposition = hasActiveComposition
        // Overlay removal invariant: patches are generated by the Rust kernel against
        // committed text, so the buffer must be in committed-text state before applying them.
        // If the overlay were left in place, the patch's UTF-8→UTF-16 offset mapping would
        // be wrong because the buffer contains virtual preedit text that the kernel doesn't
        // know about — the kernel's byte offsets map to committed text, not the overlaid text.
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

    /**
     * Apply a sequence of display patches from the Rust kernel.
     *
     * Revision continuity invariant: each patch's [baseRevision] must equal the mirror's
     * current [currentRevision]. A mismatch means patches were generated against an
     * outdated revision and the mirror must be reloaded from the kernel snapshot instead.
     *
     * After each patch, the UTF-8→UTF-16 index map is rebuilt because the buffer
     * content has changed — subsequent patches in the same batch must use updated offsets.
     */
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

    /**
     * Update or begin a composition overlay.
     *
     * Composition lifecycle: begin (first updateComposition) → update (subsequent calls) →
     * commit (applyCompositionCommit) or cancel (clearComposition). The original text under
     * the preedit range is saved in [compositionOriginalText] and restored on commit/cancel
     * before the actual text replacement is applied.
     *
     * Overlay invariant: [removeCompositionOverlay] is always called first to restore the
     * committed text, ensuring the buffer is in a consistent committed-text state before
     * the new preedit is overlaid. Without this, consecutive updateComposition calls would
     * treat the previous preedit as committed text, corrupting [compositionOriginalText].
     */
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
            // SPAN_EXCLUSIVE_EXCLUSIVE: the span does not expand when text is inserted at
            // its boundaries. This is correct for the preedit underline because the IME
            // controls the exact range — adjacent insertions should not extend the underline.
            SpannableStringBuilder.SPAN_EXCLUSIVE_EXCLUSIVE
        )
    }

    fun clearComposition() {
        removeCompositionOverlay()
    }

    /**
     * Remove the composition overlay and restore the original committed text.
     * After removal, [compositionStartUtf16] is set to -1 (sentinel meaning "no active
     * composition") since 0 is a valid UTF-16 offset.
     *
     * Must be called before any edit that modifies the committed buffer (patches, composition
     * update, composition commit, composition cancel) to ensure the buffer reflects committed
     * text only. If the overlay were left in place, the patch would be applied to the virtual
     * text (including preedit), producing incorrect UTF-8→UTF-16 offset mappings and
     * corrupting the committed-text state that the kernel expects.
     */
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
