package com.xiwei.sujian.feature.editor.projection

import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorContentDeltaDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto
import uniffi.writer_core.OffsetMapDto
import uniffi.writer_core.OffsetMapEntryDto
import uniffi.writer_core.OffsetMapKindDto

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
    val resultingSelectionEnd: Int,
) {
    companion object {
        fun fromDto(dto: DisplayPatchDto): DisplayPatch =
            DisplayPatch(
                baseRevision = dto.baseRevision.toLong(),
                newRevision = dto.newRevision.toLong(),
                replaceByteStart = dto.replaceByteStart.toInt(),
                replaceByteEndExclusive = dto.replaceByteEndExclusive.toInt(),
                insertedText = dto.insertedText,
                resultingSelectionStart = dto.resultingSelectionStart.toInt(),
                resultingSelectionEnd = dto.resultingSelectionEnd.toInt(),
            )

        fun fromDtoList(dtos: List<DisplayPatchDto>): List<DisplayPatch> = dtos.map { fromDto(it) }
    }
}

data class VisualIntent(
    val cause: EditorTransactionCauseDto,
    val operationKind: EditorOperationKindDto,
    val oldAffectedByteRanges: List<Pair<Int, Int>>,
    val newAffectedByteRanges: List<Pair<Int, Int>>,
    val animationMode: AnimationModeDto,
    val durationMs: Long,
    val coordinatedCursor: CoordinatedCursor,
    val offsetMap: OffsetMap? = null,
) {
    companion object {
        fun fromDto(dto: EditorVisualIntentDto): VisualIntent =
            VisualIntent(
                cause = dto.cause,
                operationKind = dto.operationKind,
                oldAffectedByteRanges =
                    dto.oldAffectedByteRanges.map {
                        Pair(
                            it.start.toInt(),
                            it.endExclusive.toInt(),
                        )
                    },
                newAffectedByteRanges =
                    dto.newAffectedByteRanges.map {
                        Pair(
                            it.start.toInt(),
                            it.endExclusive.toInt(),
                        )
                    },
                animationMode = dto.animationMode,
                durationMs = dto.durationMs.toLong(),
                coordinatedCursor = CoordinatedCursor.fromDto(dto.coordinatedCursor),
                offsetMap = dto.offsetMap?.let { OffsetMap.fromDto(it) },
            )
    }

    fun isInsert(): Boolean = operationKind == EditorOperationKindDto.INSERT

    fun isDelete(): Boolean = operationKind == EditorOperationKindDto.DELETE

    fun isReplace(): Boolean = operationKind == EditorOperationKindDto.REPLACE

    fun isCompositionUpdate(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_UPDATE

    fun isCompositionCommit(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_COMMIT

    fun isCompositionCancel(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_CANCEL

    fun isCursorOnly(): Boolean = operationKind == EditorOperationKindDto.CURSOR_ONLY

    fun isInsertRenderRole(): Boolean = isInsert()

    fun isDeleteRenderRole(): Boolean = isDelete() || isCompositionCancel()

    fun isReplaceRenderRole(): Boolean = isReplace() || isCompositionCommit() || isCompositionUpdate()

    fun isDeleteOrReplaceRenderRole(): Boolean =
        isDelete() || isReplace() || isCompositionCancel() || isCompositionCommit() ||
            isCompositionUpdate()
}

data class OffsetMap(
    val entries: List<OffsetMapEntry>,
) {
    companion object {
        fun fromDto(dto: OffsetMapDto): OffsetMap =
            OffsetMap(
                entries = dto.entries.map { OffsetMapEntry.fromDto(it) },
            )
    }
}

data class OffsetMapEntry(
    val oldByteOffset: Int,
    val newByteOffset: Int,
    val length: Int,
    val kind: OffsetMapKind,
) {
    companion object {
        fun fromDto(dto: OffsetMapEntryDto): OffsetMapEntry =
            OffsetMapEntry(
                oldByteOffset = dto.oldByteOffset.toInt(),
                newByteOffset = dto.newByteOffset.toInt(),
                length = dto.length.toInt(),
                kind = OffsetMapKind.fromDto(dto.kind),
            )
    }
}

enum class OffsetMapKind {
    IDENTITY,
    SHIFTED,
    ;

    companion object {
        fun fromDto(dto: OffsetMapKindDto): OffsetMapKind =
            when (dto) {
                OffsetMapKindDto.IDENTITY -> IDENTITY
                OffsetMapKindDto.SHIFTED -> SHIFTED
            }
    }
}

data class CoordinatedCursor(
    val oldByteOffset: Int,
    val newByteOffset: Int,
    val shouldAnimate: Boolean,
) {
    companion object {
        fun fromDto(dto: CoordinatedCursorDto): CoordinatedCursor =
            CoordinatedCursor(
                oldByteOffset = dto.oldByteOffset.toInt(),
                newByteOffset = dto.newByteOffset.toInt(),
                shouldAnimate = dto.shouldAnimate,
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
    val visualIntent: VisualIntent,
    val contentDelta: EditorContentDeltaDto = EditorContentDeltaDto(0u, 0u, 0u, 0u),
) {
    companion object {
        fun fromDto(dto: EditorEditResultDto): EditResult =
            EditResult(
                outcome = dto.outcome,
                transactionId = dto.transactionId.toLong(),
                baseRevision = dto.baseRevision.toLong(),
                newRevision = dto.newRevision.toLong(),
                displayPatches = DisplayPatch.fromDtoList(dto.displayPatches),
                oldSelectionStart = dto.oldSelectionStart.toInt(),
                oldSelectionEnd = dto.oldSelectionEnd.toInt(),
                newSelectionStart = dto.newSelectionStart.toInt(),
                newSelectionEnd = dto.newSelectionEnd.toInt(),
                visualIntent = VisualIntent.fromDto(dto.visualIntent),
                contentDelta = dto.contentDelta,
            )
    }

    fun isApplied(): Boolean =
        outcome == uniffi.writer_core.EditorEditOutcomeDto.APPLIED ||
            outcome == uniffi.writer_core.EditorEditOutcomeDto.APPLIED_WITH_ADJUSTED_SELECTION

    fun isStale(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.STALE_REVISION

    fun isInvalid(): Boolean =
        outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_OFFSET ||
            outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_RANGE

    fun isNoChange(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.NO_CHANGE
}
