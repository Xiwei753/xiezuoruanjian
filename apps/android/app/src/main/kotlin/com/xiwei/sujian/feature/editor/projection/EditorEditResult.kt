package com.xiwei.sujian.feature.editor.projection

import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorContentDeltaDto
import uniffi.writer_core.EditorEditOutcomeDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
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

/**
 * Issue #735 评论 5771063665：编辑结果 —
 * Core 已删除 `EditorVisualIntentDto` / `CoordinatedCursorDto` / `AnimationModeDto`，
 * `EditorEditResultDto` 直接暴露 `cause`、`operationKind`、`offsetMap`。
 *
 * Android 从这三个字段推导动画策略，不再拿 Core 的 Visual DTO。
 */
data class EditResult(
    val outcome: EditorEditOutcomeDto,
    val transactionId: Long,
    val baseRevision: Long,
    val newRevision: Long,
    val displayPatches: List<DisplayPatch>,
    val oldSelectionAnchor: Int,
    val oldSelectionHead: Int,
    val newSelectionAnchor: Int,
    val newSelectionHead: Int,
    val cause: EditorTransactionCauseDto,
    val operationKind: EditorOperationKindDto,
    val offsetMap: OffsetMap?,
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
                oldSelectionAnchor = dto.oldSelectionAnchor.toInt(),
                oldSelectionHead = dto.oldSelectionHead.toInt(),
                newSelectionAnchor = dto.newSelectionAnchor.toInt(),
                newSelectionHead = dto.newSelectionHead.toInt(),
                cause = dto.cause,
                operationKind = dto.operationKind,
                offsetMap = dto.offsetMap?.let { OffsetMap.fromDto(it) },
                contentDelta = dto.contentDelta,
            )
    }

    fun isApplied(): Boolean =
        outcome == EditorEditOutcomeDto.APPLIED ||
            outcome == EditorEditOutcomeDto.APPLIED_WITH_ADJUSTED_SELECTION

    fun isStale(): Boolean = outcome == EditorEditOutcomeDto.STALE_REVISION

    fun isInvalid(): Boolean =
        outcome == EditorEditOutcomeDto.INVALID_OFFSET ||
            outcome == EditorEditOutcomeDto.INVALID_RANGE

    fun isNoChange(): Boolean = outcome == EditorEditOutcomeDto.NO_CHANGE
}
