package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.projection.DisplayPatch
import com.xiwei.sujian.feature.editor.projection.EditResult
import com.xiwei.sujian.feature.editor.session.toSessionDelta
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EditResultTest {
    @Test
    fun displayPatchFromDtoRoundTrip() {
        val patch =
            DisplayPatch(
                baseRevision = 0,
                newRevision = 1,
                replaceByteStart = 2,
                replaceByteEndExclusive = 2,
                insertedText = "c",
                resultingSelectionStart = 3,
                resultingSelectionEnd = 3,
            )
        assertEquals(0, patch.baseRevision)
        assertEquals(1, patch.newRevision)
        assertEquals(2, patch.replaceByteStart)
        assertEquals(2, patch.replaceByteEndExclusive)
        assertEquals("c", patch.insertedText)
    }

    @Test
    fun editResultHoldsCorrectFields() {
        val result =
            EditResult(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = 1,
                baseRevision = 0,
                newRevision = 1,
                displayPatches =
                    listOf(
                        DisplayPatch(0, 1, 2, 2, "c", 3, 3),
                    ),
                oldSelectionAnchor = 2,
                oldSelectionHead = 2,
                newSelectionAnchor = 3,
                newSelectionHead = 3,
                cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
                offsetMap = null,
            )
        assertEquals(1, result.transactionId)
        assertEquals(1, result.displayPatches.size)
        assertEquals(3, result.newSelectionHead)
        assertEquals(uniffi.writer_core.EditorTransactionCauseDto.TYPING, result.cause)
        assertEquals(uniffi.writer_core.EditorOperationKindDto.INSERT, result.operationKind)
        assertTrue(result.isApplied())
    }
}

/**
 * #624 评论8/9：Android 直接消费 Core EditorContentDeltaDto 真值 —
 * 不允许用 UTF-8 byte 长度冒充 deletedChars。验证 fromDto 映射 + toSessionDelta 转换。
 *
 * Issue #735 评论 5771063665：Core 已删除 `EditorVisualIntentDto` / `CoordinatedCursorDto`，
 * `EditorEditResultDto` 直接暴露 `cause`、`operationKind`、`offsetMap`。
 */
class EditorContentDeltaConsumptionTest {
    @Test
    fun editResultFromDtoMapsContentDeltaTruthfully() {
        // 删除 2 个 CJK char（6 UTF-8 bytes）+ 插入 1 emoji（4 bytes）：
        // Core 计数按 Unicode scalar，deletedChars=2、insertedChars=1，
        // 不是 UTF-8 byte 数（6/4）。
        val dto =
            uniffi.writer_core.EditorEditResultDto(
                outcome = uniffi.writer_core.EditorEditOutcomeDto.APPLIED,
                transactionId = 1u,
                baseRevision = 0u,
                newRevision = 1u,
                displayPatches = emptyList(),
                oldSelectionAnchor = 6u,
                oldSelectionHead = 12u,
                newSelectionAnchor = 6u,
                newSelectionHead = 10u,
                cause = uniffi.writer_core.EditorTransactionCauseDto.DELETE,
                operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
                offsetMap = null,
                compositionSession = null,
                contentDelta =
                    uniffi.writer_core.EditorContentDeltaDto(
                        insertedChars = 1u,
                        deletedChars = 2u,
                        insertedNonWhitespaceChars = 1u,
                        deletedNonWhitespaceChars = 2u,
                    ),
                composition = null,
            )
        val result = EditResult.fromDto(dto)
        val sessionDelta = result.contentDelta.toSessionDelta()
        // Unicode scalar 计数真值：非 UTF-8 byte 近似
        assertEquals(1, sessionDelta.insertedChars)
        assertEquals(2, sessionDelta.deletedChars)
        assertEquals(1, sessionDelta.insertedNonWhitespaceChars)
        assertEquals(2, sessionDelta.deletedNonWhitespaceChars)
        assertEquals(-1, sessionDelta.netNonWhitespace)
    }

    @Test
    fun contentDeltaWhitespaceOnlyCountsZeroNonWhitespace() {
        // 删除 "\n"（1 char、0 非空白）：Core 真值 deletedChars=1、nonWhitespace=0。
        val dto =
            uniffi.writer_core.EditorContentDeltaDto(
                insertedChars = 0u,
                deletedChars = 1u,
                insertedNonWhitespaceChars = 0u,
                deletedNonWhitespaceChars = 0u,
            )
        val sessionDelta = dto.toSessionDelta()
        assertEquals(1, sessionDelta.deletedChars)
        assertEquals(0, sessionDelta.deletedNonWhitespaceChars)
    }

    @Test
    fun selectionOnlyHasZeroContentDelta() {
        val dto =
            uniffi.writer_core.EditorContentDeltaDto(
                insertedChars = 0u,
                deletedChars = 0u,
                insertedNonWhitespaceChars = 0u,
                deletedNonWhitespaceChars = 0u,
            )
        val sessionDelta = dto.toSessionDelta()
        assertEquals(0, sessionDelta.netNonWhitespace)
        assertEquals(com.xiwei.sujian.feature.editor.session.EditorContentDelta(), sessionDelta)
    }
}
