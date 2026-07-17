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
    val cause: String,
    val operationKind: String,
    val oldAffectedByteRanges: List<Pair<Int, Int>>,
    val newAffectedByteRanges: List<Pair<Int, Int>>,
    val animationMode: String,
    val durationMs: Long,
    val coordinatedCursor: CoordinatedCursor
) {
    companion object {
        fun fromDto(dto: EditorVisualIntentDto): VisualIntent = VisualIntent(
            cause = dto.cause.name,
            operationKind = dto.operationKind.name,
            oldAffectedByteRanges = dto.oldAffectedByteRanges.map { Pair(it.start.toInt(), it.endExclusive.toInt()) },
            newAffectedByteRanges = dto.newAffectedByteRanges.map { Pair(it.start.toInt(), it.endExclusive.toInt()) },
            animationMode = dto.animationMode.name,
            durationMs = dto.durationMs.toLong(),
            coordinatedCursor = CoordinatedCursor.fromDto(dto.coordinatedCursor)
        )
    }

    fun isInsert(): Boolean = operationKind == "Insert"
    fun isDelete(): Boolean = operationKind == "Delete"
    fun isReplace(): Boolean = operationKind == "Replace"
    fun isCompositionUpdate(): Boolean = operationKind == "CompositionUpdate"
    fun isCompositionCommit(): Boolean = operationKind == "CompositionCommit"
    fun isCompositionCancel(): Boolean = operationKind == "CompositionCancel"
    fun isCursorOnly(): Boolean = operationKind == "CursorOnly"
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
}

class DisplayTextMirror {
    private val buffer = SpannableStringBuilder()
    private var currentRevision: Long = 0
    private var cursorUtf8: Int = 0
    private var cursorUtf16: Int = 0
    private var compositionStartUtf16: Int = -1
    private var compositionEndUtf16: Int = -1
    private var selectionStartUtf16: Int = 0
    private var selectionEndUtf16: Int = 0

    fun getText(): String = buffer.toString()

    fun getCursorUtf8(): Int = cursorUtf8

    fun getCursorUtf16(): Int = cursorUtf16

    fun getRevision(): Long = currentRevision

    fun getSpannable(): SpannableStringBuilder = buffer

    fun getLengthUtf16(): Int = buffer.length

    fun getSelectionStartUtf16(): Int = selectionStartUtf16

    fun getSelectionEndUtf16(): Int = selectionEndUtf16

    fun applyPatches(patches: List<DisplayPatch>) {
        if (patches.isEmpty()) return

        val hadComposition = compositionStartUtf16 >= 0 && compositionEndUtf16 > compositionStartUtf16
        if (hadComposition) {
            buffer.replace(compositionStartUtf16, compositionEndUtf16, "")
            compositionStartUtf16 = -1
            compositionEndUtf16 = -1
        }

        var indexMap = AndroidTextIndexMap(this)
        for (patch in patches) {
            if (patch.baseRevision != currentRevision) {
                throw IllegalStateException(
                    "DisplayTextMirror revision discontinuity: expected baseRevision=$currentRevision, got ${patch.baseRevision}. " +
                    "Must reload from EditorSession."
                )
            }

            val replaceStartUtf16 = indexMap.utf8ToUtf16(patch.replaceByteStart)
            val replaceEndUtf16 = indexMap.utf8ToUtf16(patch.replaceByteEndExclusive)

            buffer.replace(replaceStartUtf16, replaceEndUtf16, patch.insertedText)

            currentRevision = patch.newRevision
            cursorUtf8 = patch.resultingSelectionEnd
            indexMap = AndroidTextIndexMap(this)
            cursorUtf16 = indexMap.utf8ToUtf16(cursorUtf8)
            selectionStartUtf16 = indexMap.utf8ToUtf16(patch.resultingSelectionStart)
            selectionEndUtf16 = indexMap.utf8ToUtf16(patch.resultingSelectionEnd)
        }
    }

    fun applyDtoPatches(patches: List<DisplayPatchDto>) {
        applyPatches(DisplayPatch.fromDtoList(patches))
    }

    fun updateComposition(replaceStartUtf8: Int, replaceEndUtf8: Int, preeditText: String) {
        val indexMap = AndroidTextIndexMap(this)
        clearCompositionSpans()

        if (compositionStartUtf16 >= 0 && compositionEndUtf16 > compositionStartUtf16) {
            buffer.replace(compositionStartUtf16, compositionEndUtf16, preeditText)
        } else {
            val insertPos = indexMap.utf8ToUtf16(replaceStartUtf8)
            buffer.insert(insertPos, preeditText)
            compositionStartUtf16 = insertPos
        }
        compositionEndUtf16 = compositionStartUtf16 + preeditText.length

        buffer.setSpan(
            UnderlineSpan(),
            compositionStartUtf16,
            compositionEndUtf16,
            SpannableStringBuilder.SPAN_EXCLUSIVE_EXCLUSIVE
        )
    }

    fun clearComposition() {
        if (compositionStartUtf16 >= 0 && compositionEndUtf16 > compositionStartUtf16) {
            buffer.delete(compositionStartUtf16, compositionEndUtf16)
        }
        clearCompositionSpans()
        compositionStartUtf16 = -1
        compositionEndUtf16 = -1
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

    fun loadText(text: String, cursorUtf8: Int) {
        buffer.clear()
        buffer.append(text)
        this.cursorUtf8 = cursorUtf8
        this.currentRevision = 0
        this.compositionStartUtf16 = -1
        this.compositionEndUtf16 = -1
        val indexMap = AndroidTextIndexMap(this)
        this.cursorUtf16 = indexMap.utf8ToUtf16(cursorUtf8)
        this.selectionStartUtf16 = this.cursorUtf16
        this.selectionEndUtf16 = this.cursorUtf16
    }
}
